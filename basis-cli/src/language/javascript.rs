use super::{FunctionContract, Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static JAVASCRIPT: LangDef = LangDef {
    name: "js",
    extensions: &["ts", "tsx", "js", "jsx", "mjs"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    scan_contracts,
    test_file: TestFilePatterns {
        path_contains: &["__tests__/"],
        filename_prefixes: &[],
        filename_suffixes: &[".test.ts", ".test.js", ".spec.ts", ".spec.js"],
    },
    primitives: &[
        ("string", &["string"]),
        ("int", &["number"]),
        ("float", &["number"]),
        ("bool", &["boolean"]),
    ],
    purity_imports: &[
        ("file_io", &["fs", "fs/promises"]),
        ("network_io", &["http", "https", "node-fetch", "axios"]),
        ("env_vars", &["process.env"]),
        ("system_clock", &["Date.now"]),
        ("subprocess", &["child_process"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &["readFileSync(", "writeFileSync(", "readFile(", "writeFile("],
        ),
        ("network_io", &["fetch("]),
        (
            "stdout",
            &["console.log(", "console.info(", "console.warn("],
        ),
        ("stderr", &["console.error("]),
        ("env_vars", &["process.env"]),
        (
            "system_clock",
            &["Date.now(", "new Date(", "performance.now("],
        ),
        (
            "subprocess",
            &["exec(", "execSync(", "spawn(", "spawnSync("],
        ),
        ("process", &["process.exit(", "process.abort("]),
        ("dynamic_execution", &["Function("]),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "string"),
        ("int", "number"),
        ("float", "number"),
        ("bool", "boolean"),
    ],
    generate_preamble: Some(ts_preamble),
    generate_newtype: Some(ts_newtype),
    generate_union: Some(ts_union),
    generate_match_scaffold: Some(ts_match_scaffold),
    type_file_name: "types.ts",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // import ... from "module"  or  import "module"
        if trimmed.starts_with("import ") {
            if let Some(module) = extract_module_path(trimmed) {
                imports.push(Import {
                    module,
                    line: line_num + 1,
                });
            }
        }
        // const X = require("module")
        else if trimmed.contains("require(") {
            if let Some(module) = extract_require_path(trimmed) {
                imports.push(Import {
                    module,
                    line: line_num + 1,
                });
            }
        }
    }
    imports
}

fn scan_contracts(content: &str) -> Vec<FunctionContract> {
    let lines: Vec<&str> = content.lines().collect();
    let mut results = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Detect function declarations
        let is_export = trimmed.starts_with("export ");
        let has_function = trimmed.contains("function ");

        if !has_function
            && !trimmed.starts_with("async ")
            && !(trimmed.contains('(')
                && (trimmed.starts_with("export ") || trimmed.starts_with("const ") || trimmed.starts_with("let ")))
        {
            i += 1;
            continue;
        }

        let decl_line = i;
        let fn_name = extract_js_function_name(trimmed);
        if fn_name.is_empty() {
            i += 1;
            continue;
        }

        let is_constructor = fn_name == "constructor";

        let Some((params_text, end_idx)) = super::collect_params_text(&lines, i) else {
            i += 1;
            continue;
        };
        i = end_idx + 1;

        let mut params = Vec::new();
        for param in params_text.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }
            let name = param.split(':').next().unwrap_or("").trim();
            // Strip default values
            let name = name.split('=').next().unwrap_or("").trim();
            if !name.is_empty() && name != "this" {
                params.push(name.to_string());
            }
        }

        if params.is_empty() {
            continue;
        }

        // Scan first ~10 body lines for guards
        let mut guarded_params = Vec::new();
        let mut body_lines_seen = 0;

        for j in (end_idx + 1)..lines.len().min(end_idx + 20) {
            let line_trimmed = lines[j].trim();
            if line_trimmed.is_empty() || line_trimmed.starts_with("//") {
                continue;
            }
            if line_trimmed == "}" {
                break;
            }

            body_lines_seen += 1;
            if body_lines_seen > 10 {
                break;
            }

            // Guard patterns: if (!x) throw, console.assert, assert(
            let is_guard = (line_trimmed.starts_with("if ")
                && (line_trimmed.contains("throw ") || line_trimmed.contains("return")))
                || line_trimmed.starts_with("console.assert(")
                || line_trimmed.starts_with("assert(");

            if is_guard {
                for param in &params {
                    if line_trimmed.contains(param.as_str()) && !guarded_params.contains(param) {
                        guarded_params.push(param.clone());
                    }
                }
            }
        }

        results.push(FunctionContract {
            name: fn_name,
            line: decl_line + 1,
            is_public: is_export,
            is_constructor,
            params,
            guarded_params,
        });
    }

    results
}

fn extract_module_path(line: &str) -> Option<String> {
    for quote in ['"', '\''] {
        if let Some(start) = line.rfind(quote) {
            let before = &line[..start];
            if let Some(prev_start) = before.rfind(quote) {
                let module = &line[prev_start + 1..start];
                if !module.is_empty() {
                    return Some(module.to_string());
                }
            }
        }
    }
    None
}

fn extract_require_path(line: &str) -> Option<String> {
    let start = line.find("require(")? + 8;
    let rest = &line[start..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)?;
    let module = &rest[1..end + 1];
    if module.is_empty() {
        return None;
    }
    Some(module.to_string())
}

fn scan_signatures(
    content: &str,
    _file: &str,
    type_map: &HashMap<String, Vec<String>>,
    _name_hints: &HashMap<String, Vec<String>>,
    out: &mut Vec<SignatureHit>,
) {
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        let is_declaration = trimmed.contains("function ")
            || trimmed.contains("function(")
            || trimmed.contains("=>")
            || trimmed.starts_with("export ")
            || trimmed.starts_with("async ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("var ")
            // class/interface method shorthand: name(params): Type; or name(params) {
            || (trimmed.contains('(')
                && (trimmed.contains('{') || trimmed.ends_with(';'))
                && !trimmed.starts_with("if ")
                && !trimmed.starts_with("if(")
                && !trimmed.starts_with("for ")
                && !trimmed.starts_with("for(")
                && !trimmed.starts_with("while ")
                && !trimmed.starts_with("while(")
                && !trimmed.starts_with("switch ")
                && !trimmed.starts_with("switch(")
                && !trimmed.starts_with("new ")
                && !trimmed.starts_with("return ")
                && !trimmed.starts_with("throw "));

        if !is_declaration {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name
        let fn_name = extract_js_function_name(trimmed);

        let Some((params_text, end_idx)) = super::collect_params_text(&lines, i) else {
            i += 1;
            continue;
        };
        i = end_idx + 1;

        for param in params_text.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }
            if let Some((name, type_ann)) = param.split_once(':') {
                let name = name.trim();
                let type_ann = type_ann.trim();

                if let Some(newtypes) = type_map.get(type_ann) {
                    for nt in newtypes {
                        if super::name_matches_newtype(name, nt) {
                            out.push(SignatureHit {
                                line: decl_line + 1,
                                function_name: fn_name.clone(),
                                param_name: name.to_string(),
                                raw_type: type_ann.to_string(),
                                suggested: vec![nt.clone()],
                            });
                        }
                    }
                }
            }
        }

        // Check return type: function foo(...): ReturnType {
        let rest = lines[end_idx].split_once(')').map(|x| x.1).unwrap_or("");
        if let Some(colon_pos) = rest.find(':') {
            let ret = rest[colon_pos + 1..]
                .trim()
                .trim_end_matches('{')
                .trim_end_matches("=>")
                .trim_end_matches(';')
                .trim();
            if !ret.is_empty() {
                super::check_return_type(ret, &fn_name, decl_line, type_map, out);
            }
        }
    }
}

/// Extract function name from a JS/TS declaration line.
/// Handles: `function name(`, `async function name(`, `const name =`, `name(` (method shorthand)
fn extract_js_function_name(line: &str) -> String {
    // "function name(" or "async function name("
    if let Some(pos) = line.find("function ") {
        let after = &line[pos + 9..];
        if let Some(name) = after.split(&['(', '<'][..]).next() {
            let name = name.trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    // "const name =" or "let name =" or "var name ="
    for kw in &["const ", "let ", "var "] {
        if let Some(pos) = line.find(kw) {
            let after = &line[pos + kw.len()..];
            if let Some(name) = after.split(&['=', ':', ' '][..]).next() {
                let name = name.trim();
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }
    // Method shorthand: "name(" — take the word before "("
    if let Some(paren_pos) = line.find('(') {
        let before = line[..paren_pos].trim();
        if let Some(name) = before.split_whitespace().last() {
            return name.to_string();
        }
    }
    String::new()
}

fn scan_matches(
    content: &str,
    _file: &str,
    variant_index: &HashMap<String, (String, HashSet<String>)>,
    union_map: &HashMap<String, HashSet<String>>,
    out: &mut Vec<MatchHit>,
) {
    let lines: Vec<&str> = content.lines().collect();

    for (line_num, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if trimmed.starts_with("switch") && trimmed.contains('(') {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_default = false;

            for subsequent in &lines[line_num + 1..] {
                let sub_trimmed = subsequent.trim();
                if sub_trimmed == "}" {
                    break;
                }
                if sub_trimmed.starts_with("default:") || sub_trimmed.starts_with("default :") {
                    has_default = true;
                    break;
                }
                if sub_trimmed.starts_with("case ") {
                    let case_val = sub_trimmed
                        .trim_start_matches("case ")
                        .split(':')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'');
                    let variant = case_val.rsplit('.').next().unwrap_or(case_val);
                    if !variant.is_empty() {
                        cases.insert(variant.to_string());
                    }
                }
            }

            if !has_default {
                if let Some((union_name, mut missing)) =
                    super::find_union_for_cases(&cases, variant_index, union_map)
                {
                    missing.sort();
                    out.push(MatchHit {
                        line: line_num + 1,
                        union_name,
                        missing_variants: missing,
                    });
                }
            }
        }
    }
}

fn ts_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    String::new()
}

fn ts_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "export type {name} = {prim} & {{ readonly __brand: \"{name}\" }};\n"
    ));
    out.push('\n');
    let fn_name = format!("create{name}");
    if let Some(val) = validation {
        out.push_str(&format!("/** TODO: implement {val} validation */\n"));
        out.push_str(&format!("export function {fn_name}(value: {prim}): {name} {{ throw new Error(\"not implemented\"); }}\n"));
    } else {
        out.push_str(&format!(
            "export function {fn_name}(value: {prim}): {name} {{ return value as {name}; }}\n"
        ));
    }
    out
}

fn ts_union(name: &str, variants: &[String]) -> String {
    let parts: Vec<String> = variants.iter().map(|v| format!("\"{v}\"")).collect();
    format!("export type {name} = {};\n", parts.join(" | "))
}

fn ts_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let fn_name = format!("handle{union_name}");
    let param = union_name[..1].to_lowercase() + &union_name[1..];
    let mut out = String::new();
    out.push_str("/** Exhaustive switch scaffold — all variants must be handled. */\n");
    out.push_str(&format!(
        "export function {fn_name}({param}: {union_name}): void {{\n"
    ));
    out.push_str(&format!("    switch ({param}) {{\n"));
    for v in variants {
        out.push_str(&format!("        case \"{v}\":\n"));
        out.push_str("            throw new Error(\"TODO\");\n");
    }
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_from() {
        let imports = extract_imports("import { Foo } from \"./models\";\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "./models");
    }

    #[test]
    fn require() {
        let imports = extract_imports("const fs = require('fs');\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "fs");
    }

    #[test]
    fn import_no_from() {
        let imports = extract_imports("import \"side-effect-module\";\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "side-effect-module");
    }

    #[test]
    fn import_quoted() {
        let content = "import axios from \"axios\";\nconst fs = require('fs');\n";
        let imports = extract_imports(content);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "axios");
        assert_eq!(imports[1].module, "fs");
    }
}
