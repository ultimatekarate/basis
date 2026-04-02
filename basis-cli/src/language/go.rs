use super::{FunctionContract, Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static GO: LangDef = LangDef {
    name: "go",
    extensions: &["go"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    scan_contracts,
    test_file: TestFilePatterns {
        path_contains: &[],
        filename_prefixes: &[],
        filename_suffixes: &["_test.go"],
    },
    primitives: &[
        ("string", &["string"]),
        (
            "int",
            &["int", "int32", "int64", "uint", "uint32", "uint64"],
        ),
        ("float", &["float32", "float64"]),
        ("bool", &["bool"]),
    ],
    purity_imports: &[
        ("file_io", &["os", "io/ioutil", "bufio"]),
        ("network_io", &["net/http", "net"]),
        ("system_clock", &["time"]),
        ("subprocess", &["os/exec"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "os.Open(",
                "os.Create(",
                "os.ReadFile(",
                "os.WriteFile(",
                "os.Remove(",
                "os.Mkdir(",
                "os.MkdirAll(",
                "ioutil.ReadFile(",
                "ioutil.WriteFile(",
            ],
        ),
        (
            "network_io",
            &["http.Get(", "http.Post(", "http.ListenAndServe("],
        ),
        ("stdout", &["fmt.Print(", "fmt.Println(", "fmt.Printf("]),
        ("stderr", &["fmt.Fprint(os.Stderr"]),
        ("env_vars", &["os.Getenv(", "os.Setenv("]),
        ("system_clock", &["time.Now("]),
        ("subprocess", &["exec.Command("]),
        ("process", &["os.Exit("]),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "string"),
        ("int", "int64"),
        ("float", "float64"),
        ("bool", "bool"),
    ],
    generate_preamble: Some(go_preamble),
    generate_newtype: Some(go_newtype),
    generate_union: Some(go_union),
    generate_match_scaffold: Some(go_match_scaffold),
    type_file_name: "types.go",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    let mut in_import_block = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // import "fmt"
        if trimmed.starts_with("import \"") {
            if let Some(module) = extract_quoted(trimmed) {
                imports.push(Import {
                    module,
                    line: line_num + 1,
                });
            }
            continue;
        }

        // import ( ... )
        if trimmed == "import (" {
            in_import_block = true;
            continue;
        }
        if in_import_block {
            if trimmed == ")" {
                in_import_block = false;
                continue;
            }
            if let Some(module) = extract_quoted(trimmed) {
                imports.push(Import {
                    module,
                    line: line_num + 1,
                });
            }
        }
    }

    imports
}

fn extract_quoted(line: &str) -> Option<String> {
    let start = line.find('"')? + 1;
    let end = start + line[start..].find('"')?;
    let s = &line[start..end];
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
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

        if !trimmed.starts_with("func ") {
            i += 1;
            continue;
        }

        let func_part = &trimmed[5..]; // after "func "
                                       // For methods with receivers, we need to find the params paren, not the receiver paren.
                                       // Rewrite the line to skip past the receiver so collect_params_text finds the right paren.
        let effective_start = if func_part.starts_with('(') {
            // Method: func (r *Type) Name(params)
            let receiver_end = func_part.find(')').unwrap_or(0) + 1;
            let rest = &func_part[receiver_end..];
            if rest.find('(').is_none() {
                i += 1;
                continue;
            }
            // Find the params paren position in the original line
            let receiver_abs_end = 5 + receiver_end;
            trimmed[receiver_abs_end..].find('(').unwrap() + receiver_abs_end
        } else {
            match trimmed.find('(') {
                Some(p) => p,
                None => {
                    i += 1;
                    continue;
                }
            }
        };

        let decl_line = i;

        // Extract function name: between receiver (if any) and "("
        let fn_name = {
            let name_part = &trimmed[5..effective_start]; // between "func " and params "("
                                                          // For methods: "(r *Type) Name" — take the last word
            name_part
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_string()
        };

        // Build a synthetic line starting from the params paren for collect_params_text
        let synthetic = &trimmed[effective_start..];
        let synthetic_lines = [synthetic];
        // First try single-line
        let (params_text, _) = if let Some(result) = super::collect_params_text(&synthetic_lines, 0)
        {
            (result.0, decl_line)
        } else {
            // Multi-line: build remaining lines
            let mut ml_lines: Vec<&str> = vec![synthetic];
            for line in &lines[(i + 1)..lines.len().min(i + 21)] {
                ml_lines.push(line);
            }
            if let Some((text, offset)) = super::collect_params_text(&ml_lines, 0) {
                i = decl_line + offset + 1;
                (text, decl_line)
            } else {
                i += 1;
                continue;
            }
        };

        // If we didn't jump forward from multi-line, advance past this line
        if i == decl_line {
            i += 1;
        }

        for param in params_text.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }
            let parts: Vec<&str> = param.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0];
                let type_ann = parts[parts.len() - 1];
                if name.starts_with('_') {
                    continue;
                }

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

        // Check return type: func foo() string {
        // Find last ')' on the declaration line and extract the type between ')' and '{'
        if let Some(close_paren) = trimmed.rfind(')') {
            let after = trimmed[close_paren + 1..].trim();
            // Skip tuple returns like (string, error) — only check single returns
            if !after.starts_with('(') {
                let ret = after
                    .trim_end_matches('{')
                    .trim();
                if !ret.is_empty() {
                    super::check_return_type(ret, &fn_name, decl_line, type_map, out);
                }
            }
        }
    }
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

        if trimmed.starts_with("switch ") && trimmed.contains('{') {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_default = false;
            let mut brace_depth = 1i32;

            for subsequent in &lines[line_num + 1..] {
                let sub_trimmed = subsequent.trim();

                for ch in sub_trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
                if brace_depth <= 0 {
                    break;
                }

                if sub_trimmed.starts_with("default:") {
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
                        .trim_matches('"');
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

fn scan_contracts(content: &str) -> Vec<FunctionContract> {
    let lines: Vec<&str> = content.lines().collect();
    let mut results = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if !trimmed.starts_with("func ") {
            i += 1;
            continue;
        }

        let decl_line = i;
        // All Go exported functions start with uppercase
        let func_part = &trimmed[5..];
        let is_public = func_part
            .chars()
            .find(|c| c.is_alphabetic())
            .map_or(false, |c| c.is_uppercase());

        // Extract function name (skip receiver)
        let fn_name = {
            let name_start = if func_part.starts_with('(') {
                // Method with receiver: func (r *Type) Name(
                let receiver_end = func_part.find(')').unwrap_or(0) + 1;
                &func_part[receiver_end..]
            } else {
                func_part
            };
            name_start
                .trim()
                .split(&['(', '<'][..])
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        };

        let is_constructor = fn_name.starts_with("New");

        // Find params
        let effective_start = match trimmed.rfind('(') {
            Some(p) => p,
            None => { i += 1; continue; }
        };
        let _ = effective_start; // We'll use collect_params_text

        let Some((params_text, end_idx)) = super::collect_params_text(&lines, i) else {
            i += 1;
            continue;
        };
        i = end_idx + 1;

        let mut params = Vec::new();
        for param in params_text.split(',') {
            let param = param.trim();
            if param.is_empty() { continue; }
            let parts: Vec<&str> = param.split_whitespace().collect();
            if !parts.is_empty() && !parts[0].starts_with('_') {
                params.push(parts[0].to_string());
            }
        }

        if params.is_empty() { continue; }

        // Scan first ~10 body lines for guards
        let mut guarded_params = Vec::new();
        let mut body_lines_seen = 0;

        for j in (end_idx + 1)..lines.len().min(end_idx + 20) {
            let line_trimmed = lines[j].trim();
            if line_trimmed.is_empty() || line_trimmed.starts_with("//") { continue; }
            if line_trimmed == "}" { break; }

            body_lines_seen += 1;
            if body_lines_seen > 10 { break; }

            // Go guard patterns: if x { return err }, if x == nil { return }
            let is_guard = line_trimmed.starts_with("if ")
                && (line_trimmed.contains("return") || line_trimmed.contains("panic("));

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
            is_public,
            is_constructor,
            params,
            guarded_params,
        });
    }

    results
}

fn go_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    "package types\n".to_string()
}

fn go_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(&format!("type {name} {prim}\n"));
    if let Some(val) = validation {
        out.push('\n');
        out.push_str(&format!(
            "// New{name} creates a {name}. TODO: implement {val} validation.\n"
        ));
        out.push_str(&format!(
            "func New{name}(value {prim}) ({name}, error) {{ panic(\"not implemented\") }}\n"
        ));
    }
    out
}

fn go_union(name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("type {name} int\n\n"));
    out.push_str("const (\n");
    for (i, v) in variants.iter().enumerate() {
        if i == 0 {
            out.push_str(&format!("\t{v} {name} = iota\n"));
        } else {
            out.push_str(&format!("\t{v}\n"));
        }
    }
    out.push_str(")\n");
    out
}

fn go_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let param = union_name[..1].to_lowercase() + &union_name[1..];
    let mut out = String::new();
    out.push_str(&format!(
        "// Handle{union_name} — exhaustive switch scaffold.\n"
    ));
    out.push_str(&format!(
        "func Handle{union_name}({param} {union_name}) {{\n"
    ));
    out.push_str(&format!("\tswitch {param} {{\n"));
    for v in variants {
        out.push_str(&format!("\tcase {v}:\n"));
        out.push_str("\t\tpanic(\"TODO\")\n");
    }
    out.push_str("\t}\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_single() {
        let imports = extract_imports("import \"fmt\"\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "fmt");
    }

    #[test]
    fn import_block() {
        let imports = extract_imports("import (\n\t\"fmt\"\n\t\"net/http\"\n)\n");
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "fmt");
        assert_eq!(imports[1].module, "net/http");
    }

    #[test]
    fn import_aliased() {
        let imports = extract_imports("import (\n\tmux \"github.com/gorilla/mux\"\n)\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "github.com/gorilla/mux");
    }
}
