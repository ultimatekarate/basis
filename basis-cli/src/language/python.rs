use super::{Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static PYTHON: LangDef = LangDef {
    name: "python",
    extensions: &["py"],
    comment_prefix: "#",
    extract_imports,
    scan_signatures,
    scan_matches,
    test_file: TestFilePatterns {
        path_contains: &["tests/", "__tests__/"],
        filename_prefixes: &["test_"],
        filename_suffixes: &["_test.py"],
    },
    primitives: &[
        ("string", &["str"]),
        ("int", &["int"]),
        ("float", &["float"]),
        ("bool", &["bool"]),
    ],
    purity_imports: &[
        (
            "file_io",
            &[
                "os.path",
                "os.remove",
                "os.mkdir",
                "os.rename",
                "os.makedirs",
                "shutil",
                "tempfile",
            ],
        ),
        (
            "network_io",
            &[
                "requests",
                "httpx",
                "urllib",
                "socket",
                "http.client",
                "aiohttp",
            ],
        ),
        ("stdout", &["sys.stdout"]),
        ("stderr", &["sys.stderr"]),
        ("env_vars", &["os.environ", "os.getenv"]),
        ("system_clock", &["time.time", "time.monotonic", "datetime"]),
        ("subprocess", &["subprocess"]),
        ("process", &["sys.exit", "os._exit"]),
        ("dynamic_execution", &["importlib"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "open(",
                ".read_text(",
                ".write_text(",
                ".read_bytes(",
                ".write_bytes(",
                ".mkdir(",
                ".rmdir(",
                ".unlink(",
                "os.remove(",
                "os.mkdir(",
                "os.makedirs(",
            ],
        ),
        ("stdout", &["print("]),
        ("env_vars", &["os.getenv(", "os.environ"]),
        ("system_clock", &["time.time(", "time.monotonic("]),
        (
            "subprocess",
            &[
                "subprocess.run(",
                "subprocess.call(",
                "subprocess.Popen(",
                "subprocess.check_output(",
                "subprocess.check_call(",
            ],
        ),
        ("process", &["sys.exit(", "os._exit(", "os.abort("]),
        ("dynamic_execution", &["eval(", "exec(", "__import__("]),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "str"),
        ("int", "int"),
        ("float", "float"),
        ("bool", "bool"),
    ],
    generate_preamble: Some(python_preamble),
    generate_newtype: Some(python_newtype),
    generate_union: Some(python_union),
    generate_match_scaffold: Some(python_match_scaffold),
    type_file_name: "types.py",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("from ") {
            if let Some(module) = rest.split_whitespace().next() {
                let module = module.trim();
                if !module.is_empty() && !module.starts_with('.') {
                    imports.push(Import {
                        module: module.to_string(),
                        line: line_num + 1,
                    });
                }
            }
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            for part in rest.split(',') {
                let module = part.split_whitespace().next().unwrap_or("");
                if !module.is_empty() {
                    imports.push(Import {
                        module: module.to_string(),
                        line: line_num + 1,
                    });
                }
            }
        }
    }
    imports
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

        // Skip private/dunder functions
        if trimmed.starts_with("def _") || trimmed.starts_with("async def _") {
            i += 1;
            continue;
        }

        if !trimmed.starts_with("def ") && !trimmed.starts_with("async def ") {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name: between "def "/"async def " and "("
        let fn_name = {
            let after_keyword = if trimmed.starts_with("async def ") {
                &trimmed[10..]
            } else {
                &trimmed[4..]
            };
            after_keyword
                .split('(')
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
            if param.is_empty() || param == "self" || param == "cls" {
                continue;
            }
            if let Some((name, type_ann)) = param.split_once(':') {
                let name = name.trim().trim_start_matches('*');
                let type_ann = type_ann.trim().trim_end_matches('=').trim();

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

        // Check return type: def foo(...) -> ReturnType:
        let rest = lines[end_idx].split_once(')').map(|x| x.1).unwrap_or("");
        if let Some(arrow_pos) = rest.find("->") {
            let ret = rest[arrow_pos + 2..].trim().trim_end_matches(':').trim();
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

        // Python match statement: "match subject:"
        if trimmed.starts_with("match ") && trimmed.ends_with(':') {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_wildcard = false;

            for subsequent in &lines[line_num + 1..] {
                let sub_trimmed = subsequent.trim();
                if sub_trimmed.starts_with("case ") {
                    if sub_trimmed.contains("case _:") {
                        has_wildcard = true;
                        break;
                    }
                    if let Some(val) = extract_python_case_value(sub_trimmed) {
                        cases.insert(val);
                    }
                } else if !sub_trimmed.is_empty()
                    && !sub_trimmed.starts_with('#')
                    && !sub_trimmed.starts_with("case ")
                    && indentation_level(subsequent) <= indentation_level(line)
                {
                    break;
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

        // Python if/elif chains with == comparisons
        if trimmed.starts_with("if ") && trimmed.contains("==") {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_else = false;

            if let Some(val) = extract_comparison_value(trimmed) {
                cases.insert(val);
            }

            for subsequent in &lines[line_num + 1..] {
                let sub_trimmed = subsequent.trim();
                if sub_trimmed.starts_with("elif ") && sub_trimmed.contains("==") {
                    if let Some(val) = extract_comparison_value(sub_trimmed) {
                        cases.insert(val);
                    }
                } else if sub_trimmed.starts_with("else:") {
                    has_else = true;
                    break;
                } else if !sub_trimmed.is_empty()
                    && !sub_trimmed.starts_with('#')
                    && indentation_level(subsequent) <= indentation_level(line)
                {
                    break;
                }
            }

            if !has_else {
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

fn indentation_level(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

// Public wrappers for testing from completeness tests
#[cfg(test)]
pub fn extract_python_case_value_pub(s: &str) -> Option<String> {
    extract_python_case_value(s)
}

#[cfg(test)]
pub fn extract_comparison_value_pub(s: &str) -> Option<String> {
    extract_comparison_value(s)
}

#[cfg(test)]
pub fn indentation_level_pub(s: &str) -> usize {
    indentation_level(s)
}

fn extract_python_case_value(case_line: &str) -> Option<String> {
    let rest = case_line
        .trim()
        .strip_prefix("case ")?
        .split(':')
        .next()?
        .trim();
    if rest.starts_with('"') && rest.ends_with('"') {
        return Some(rest.trim_matches('"').to_string());
    }
    if rest.starts_with('\'') && rest.ends_with('\'') {
        return Some(rest.trim_matches('\'').to_string());
    }
    if rest.contains('.') {
        return Some(rest.rsplit('.').next()?.trim_end_matches("()").to_string());
    }
    if rest.ends_with("()") || rest.ends_with(')') {
        return Some(rest.split('(').next()?.to_string());
    }
    Some(rest.to_string())
}

fn extract_comparison_value(line: &str) -> Option<String> {
    let eq_pos = line.find("==")?;
    let rhs = line[eq_pos + 2..].trim().split(':').next()?.trim();
    if rhs.starts_with('"') {
        return Some(rhs.trim_matches('"').to_string());
    }
    if rhs.starts_with('\'') {
        return Some(rhs.trim_matches('\'').to_string());
    }
    if rhs.contains('.') {
        return Some(rhs.rsplit('.').next()?.to_string());
    }
    Some(rhs.to_string())
}

fn python_preamble(has_newtypes: bool, _has_unions: bool) -> String {
    let mut out = String::new();
    out.push_str("from __future__ import annotations\n");
    if has_newtypes {
        out.push_str("from typing import NewType\n");
    }
    out
}

fn python_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(&format!("{name} = NewType(\"{name}\", {prim})\n"));
    if let Some(val) = validation {
        out.push('\n');
        let fn_name = to_snake_case(name);
        out.push_str(&format!(
            "def validate_{fn_name}(value: {prim}) -> {name}:\n"
        ));
        out.push_str(&format!(
            "    \"\"\"TODO: implement {val} validation\"\"\"\n"
        ));
        out.push_str("    raise NotImplementedError\n");
    }
    out
}

fn python_union(_name: &str, variants: &[String]) -> String {
    let joined = variants.join(", ");
    format!("# {_name} variants: {joined}\n")
}

fn python_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let fn_name = to_snake_case(union_name);
    let mut out = String::new();
    out.push_str(&format!("def handle_{fn_name}(status: str) -> None:\n"));
    out.push_str("    \"\"\"Exhaustive match scaffold — all variants must be handled.\"\"\"\n");
    out.push_str("    match status:\n");
    for v in variants {
        out.push_str(&format!("        case \"{v}\":\n"));
        out.push_str("            pass  # TODO\n");
    }
    out
}

fn to_snake_case(s: &str) -> String {
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
    fn from_import() {
        let imports = extract_imports("from os import path\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "os");
        assert_eq!(imports[0].line, 1);
    }

    #[test]
    fn import_single() {
        let imports = extract_imports("import json\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "json");
    }

    #[test]
    fn import_multiple() {
        let imports = extract_imports("import json, sys, os\n");
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].module, "json");
        assert_eq!(imports[1].module, "sys");
        assert_eq!(imports[2].module, "os");
    }

    #[test]
    fn dotted_import() {
        let imports = extract_imports("from src.models import types\n");
        assert_eq!(imports[0].module, "src.models");
    }

    #[test]
    fn skips_relative_dot() {
        let imports = extract_imports("from . import sibling\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn skips_all_relative_imports() {
        let content = "from .station import StationId\nfrom ..types import Alert\nfrom .utils.helpers import format_id\n";
        let imports = extract_imports(content);
        assert!(imports.is_empty(), "relative imports should be skipped: {:?}", imports.iter().map(|i| &i.module).collect::<Vec<_>>());
    }

    #[test]
    fn skips_non_import_lines() {
        let imports = extract_imports("x = 1\nprint('hello')\n# import os\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn from_and_import() {
        let content = "from os import path\nimport requests\nimport json, sys\n";
        let imports = extract_imports(content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].module, "os");
        assert_eq!(imports[1].module, "requests");
        assert_eq!(imports[2].module, "json");
        assert_eq!(imports[3].module, "sys");
    }
}
