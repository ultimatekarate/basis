use super::{Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static RUST: LangDef = LangDef {
    name: "rust",
    extensions: &["rs"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    test_file: TestFilePatterns {
        path_contains: &["tests/"],
        filename_prefixes: &[],
        filename_suffixes: &["_test.rs"],
    },
    primitives: &[
        ("string", &["String", "&str"]),
        ("int", &["i32", "i64", "u32", "u64", "usize", "isize"]),
        ("float", &["f32", "f64"]),
        ("bool", &["bool"]),
    ],
    purity_imports: &[
        ("file_io", &["std::fs", "std::io"]),
        (
            "network_io",
            &["std::net", "tokio::net", "hyper", "reqwest"],
        ),
        ("env_vars", &["std::env"]),
        ("system_clock", &["std::time", "chrono"]),
        ("subprocess", &["std::process::Command"]),
        ("process", &["std::process::exit"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "File::open(",
                "File::create(",
                "read_to_string(",
                "write_all(",
            ],
        ),
        ("stdout", &["println!(", "print!("]),
        ("stderr", &["eprintln!(", "eprint!("]),
        ("system_clock", &["SystemTime::now(", "Instant::now("]),
        ("subprocess", &["Command::new("]),
        ("process", &["process::exit("]),
    ],
    preprocess: Some(strip_rust_test_section),
    preferred_type: &[
        ("string", "String"),
        ("int", "i64"),
        ("float", "f64"),
        ("bool", "bool"),
    ],
    generate_preamble: Some(rust_preamble),
    generate_newtype: Some(rust_newtype),
    generate_union: Some(rust_union),
    generate_match_scaffold: Some(rust_match_scaffold),
    type_file_name: "types.rs",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("use ") {
            let path = rest
                .trim_end_matches(';')
                .split('{')
                .next()
                .unwrap_or("")
                .trim();
            if !path.is_empty() {
                imports.push(Import {
                    module: path.to_string(),
                    line: line_num + 1,
                });
            }
        } else if let Some(rest) = trimmed.strip_prefix("mod ") {
            let module = rest.trim_end_matches(';').trim();
            if !module.is_empty() && !trimmed.contains('{') {
                imports.push(Import {
                    module: module.to_string(),
                    line: line_num + 1,
                });
            }
        }
    }
    imports
}
/// Strip everything after #[cfg(test)] in Rust source code.
pub fn strip_rust_test_section(content: &str) -> String {
    if let Some(pos) = content.find("#[cfg(test)]") {
        content[..pos].to_string()
    } else {
        content.to_string()
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

        if !trimmed.starts_with("pub fn ")
            && !trimmed.starts_with("fn ")
            && !trimmed.starts_with("pub async fn ")
            && !trimmed.starts_with("async fn ")
        {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name: find "fn " then take until "(" or "<"
        let fn_name = {
            let fn_pos = trimmed.find("fn ").unwrap_or(0) + 3;
            let after_fn = &trimmed[fn_pos..];
            after_fn
                .split(&['(', '<'][..])
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        };

        let Some((params_text, end_idx)) = super::collect_params_text(&lines, i) else {
            i += 1;
            continue;
        };
        i = end_idx + 1;

        for param in params_text.split(',') {
            let param = param.trim();
            if param.is_empty() || param == "&self" || param == "&mut self" || param == "self" {
                continue;
            }
            if let Some((name, type_ann)) = param.split_once(':') {
                let name = name.trim();
                let type_ann = type_ann.trim().trim_start_matches('&');

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

        // Check return type: fn foo(...) -> ReturnType {
        let rest = lines[end_idx].split_once(')').map(|x| x.1).unwrap_or("");
        if let Some(arrow_pos) = rest.find("->") {
            let ret = rest[arrow_pos + 2..]
                .trim()
                .trim_end_matches('{')
                .trim_end_matches("where")
                .trim();
            if !ret.is_empty() {
                super::check_return_type(ret, &fn_name, decl_line, type_map, out);
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

        if trimmed.starts_with("match ") && trimmed.contains('{') {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_wildcard = false;

            for subsequent in &lines[line_num + 1..] {
                let sub_trimmed = subsequent.trim();
                if sub_trimmed == "}" {
                    break;
                }
                if sub_trimmed.starts_with('_') && sub_trimmed.contains("=>") {
                    has_wildcard = true;
                    break;
                }
                if sub_trimmed.contains("=>") {
                    let pattern = sub_trimmed.split("=>").next().unwrap_or("").trim();
                    let variant = if pattern.contains("::") {
                        pattern.rsplit("::").next().unwrap_or("").trim()
                    } else {
                        pattern
                            .split('(')
                            .next()
                            .unwrap_or("")
                            .split('{')
                            .next()
                            .unwrap_or("")
                            .trim()
                    };
                    if !variant.is_empty() && variant != "_" {
                        for part in variant.split('|') {
                            let part = part.trim();
                            let clean = if part.contains("::") {
                                part.rsplit("::").next().unwrap_or("").trim()
                            } else {
                                part.split('(')
                                    .next()
                                    .unwrap_or("")
                                    .split('{')
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                            };
                            if !clean.is_empty() && clean != "_" {
                                cases.insert(clean.to_string());
                            }
                        }
                    }
                }
            }

            if !has_wildcard {
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

fn rust_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    String::new()
}

fn rust_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str("#[derive(Debug, Clone, PartialEq, Eq, Hash)]\n");
    out.push_str(&format!("pub struct {name}(pub {prim});\n"));
    if let Some(val) = validation {
        out.push('\n');
        out.push_str(&format!("impl {name} {{\n"));
        out.push_str(&format!("    /// TODO: implement {val} validation\n"));
        out.push_str(&format!(
            "    pub fn new(value: {prim}) -> Result<Self, String> {{ todo!() }}\n"
        ));
        out.push_str("}\n");
    } else {
        out.push('\n');
        out.push_str(&format!("impl From<{prim}> for {name} {{\n"));
        out.push_str(&format!("    fn from(v: {prim}) -> Self {{ Self(v) }}\n"));
        out.push_str("}\n");
    }
    out
}

fn rust_union(name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str("#[derive(Debug, Clone, PartialEq, Eq)]\n");
    out.push_str(&format!("pub enum {name} {{\n"));
    for v in variants {
        out.push_str(&format!("    {v},\n"));
    }
    out.push_str("}\n");
    out
}

fn rust_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let param = to_rust_snake(union_name);
    let mut out = String::new();
    out.push_str("/// Exhaustive match scaffold — all variants must be handled.\n");
    out.push_str(&format!(
        "pub fn handle_{param}({param}: &{union_name}) {{\n"
    ));
    out.push_str(&format!("    match {param} {{\n"));
    for v in variants {
        out.push_str(&format!("        {union_name}::{v} => todo!(),\n"));
    }
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

fn to_rust_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn use_statement() {
        let imports = extract_imports("use std::collections::HashMap;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "std::collections::HashMap");
    }

    #[test]
    fn use_with_braces() {
        let imports = extract_imports("use std::collections::{HashMap, HashSet};\n");
        assert_eq!(imports.len(), 1);
        assert!(imports[0].module.starts_with("std::collections"));
    }

    #[test]
    fn mod_statement() {
        let imports = extract_imports("mod check;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "check");
    }

    #[test]
    fn skips_inline_mod() {
        let imports = extract_imports("mod check {\n    use foo;\n}\n");
        let mod_imports: Vec<_> = imports.iter().filter(|i| i.module == "check").collect();
        assert!(mod_imports.is_empty());
    }

    #[test]
    fn use_imports_native_form() {
        let content = "use std::fs;\nuse crate::spec::BasisSpec;\n";
        let imports = extract_imports(content);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "std::fs");
        assert_eq!(imports[1].module, "crate::spec::BasisSpec");
    }
}
