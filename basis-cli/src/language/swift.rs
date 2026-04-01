use super::{Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static SWIFT: LangDef = LangDef {
    name: "swift",
    extensions: &["swift"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    test_file: TestFilePatterns {
        path_contains: &["Tests/"],
        filename_prefixes: &[],
        filename_suffixes: &["Tests.swift", "Test.swift"],
    },
    primitives: &[
        ("string", &["String"]),
        (
            "int",
            &["Int", "Int32", "Int64", "UInt", "UInt32", "UInt64"],
        ),
        ("float", &["Float", "Double", "CGFloat"]),
        ("bool", &["Bool"]),
    ],
    purity_imports: &[
        ("file_io", &["Foundation"]),
        ("network_io", &["Foundation", "Alamofire", "URLSession"]),
        ("system_clock", &["Foundation"]),
        ("subprocess", &["Foundation"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "FileManager.",
                "FileHandle(",
                "Data(contentsOf:",
                "String(contentsOfFile:",
                ".write(toFile:",
                ".write(to:",
            ],
        ),
        (
            "network_io",
            &[
                "URLSession.",
                "URLRequest(",
                "URL(string:",
                "URLComponents(",
            ],
        ),
        ("stdout", &["print(", "debugPrint("]),
        ("stderr", &["fputs(", "FileHandle.standardError"]),
        ("env_vars", &["ProcessInfo.processInfo.environment"]),
        (
            "system_clock",
            &["Date(", "Date.now", "CFAbsoluteTimeGetCurrent("],
        ),
        ("subprocess", &["Process(", "Process.launchedProcess("]),
        ("process", &["exit(", "fatalError(", "preconditionFailure("]),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "String"),
        ("int", "Int"),
        ("float", "Double"),
        ("bool", "Bool"),
    ],
    generate_preamble: Some(swift_preamble),
    generate_newtype: Some(swift_newtype),
    generate_union: Some(swift_union),
    generate_match_scaffold: Some(swift_match_scaffold),
    type_file_name: "Types.swift",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("import ") {
            let module = rest.trim();
            if !module.is_empty() {
                imports.push(Import {
                    module: module.to_string(),
                    line: line_num + 1,
                });
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

        if trimmed.contains("private ") || trimmed.contains("fileprivate ") {
            i += 1;
            continue;
        }

        let is_func = trimmed.starts_with("func ")
            || trimmed.starts_with("public func ")
            || trimmed.starts_with("open func ")
            || trimmed.starts_with("internal func ")
            || trimmed.starts_with("static func ")
            || trimmed.starts_with("class func ")
            || trimmed.starts_with("override func ")
            || trimmed.starts_with("mutating func ");

        if !is_func {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name: between "func " and "("
        let fn_name = {
            let func_pos = trimmed.find("func ").unwrap_or(0) + 5;
            let after_func = &trimmed[func_pos..];
            after_func
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

        // Swift params: label name: Type or _ name: Type or name: Type
        for param in params_text.split(',') {
            let param = param.trim();
            if param.is_empty() {
                continue;
            }

            // Find the colon separating name from type
            let Some(colon_pos) = param.find(':') else {
                continue;
            };
            let before_colon = param[..colon_pos].trim();
            let type_ann = param[colon_pos + 1..]
                .trim()
                .split('=')
                .next()
                .unwrap_or("")
                .trim() // strip default
                .trim_end_matches('?') // strip optional
                .trim_start_matches("inout "); // strip inout

            // The parameter name is the last word before the colon
            // (could be "label name" or just "name" or "_ name")
            let name = before_colon
                .split_whitespace()
                .last()
                .unwrap_or(before_colon);

            if let Some(newtypes) = type_map.get(type_ann) {
                if name == "_" {
                    continue;
                }
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

        // Swift switch: "switch subject {"
        if trimmed.starts_with("switch ") && trimmed.contains('{') {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_default = false;
            let mut brace_depth = 0i32;

            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }

            if brace_depth <= 0 {
                continue;
            }

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

                if sub_trimmed.starts_with("default:") || sub_trimmed.starts_with("default :") {
                    has_default = true;
                    break;
                }
                if sub_trimmed.starts_with("case ") {
                    let rest = sub_trimmed.trim_start_matches("case ");
                    let pattern = rest.split(':').next().unwrap_or("").trim();
                    // Handle comma-separated: "case .Pending, .Confirmed:"
                    for part in pattern.split(',') {
                        let val = part
                            .trim()
                            .trim_start_matches('.')
                            .trim_matches('"')
                            .trim_matches('\'');
                        let variant = val.rsplit('.').next().unwrap_or(val);
                        // Strip associated value patterns: "Pending(let x)"
                        let variant = variant.split('(').next().unwrap_or(variant).trim();
                        if !variant.is_empty() && variant != "_" {
                            cases.insert(variant.to_string());
                        }
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

fn swift_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    "import Foundation\n".to_string()
}

fn swift_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    out.push_str(&format!("struct {name} {{\n"));
    out.push_str(&format!("    let rawValue: {prim}\n"));
    if let Some(val) = validation {
        out.push_str(&format!("\n    /// TODO: implement {val} validation\n"));
        out.push_str(&format!(
            "    static func validated(_ value: {prim}) throws -> {name} {{\n"
        ));
        out.push_str("        fatalError(\"not implemented\")\n");
        out.push_str("    }\n");
    }
    out.push_str("}\n");
    out
}

fn swift_union(name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("enum {name} {{\n"));
    for v in variants {
        out.push_str(&format!("    case {v}\n"));
    }
    out.push_str("}\n");
    out
}

fn swift_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let param = union_name[..1].to_lowercase() + &union_name[1..];
    let mut out = String::new();
    out.push_str("/// Exhaustive switch scaffold — all variants must be handled.\n");
    out.push_str(&format!(
        "func handle{union_name}({param}: {union_name}) {{\n"
    ));
    out.push_str(&format!("    switch {param} {{\n"));
    for v in variants {
        out.push_str(&format!("    case .{v}:\n"));
        out.push_str("        break // TODO\n");
    }
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_simple() {
        let imports = extract_imports("import Foundation\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "Foundation");
    }

    #[test]
    fn import_multiple() {
        let imports = extract_imports("import UIKit\nimport SwiftUI\n");
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "UIKit");
        assert_eq!(imports[1].module, "SwiftUI");
    }

    #[test]
    fn import_skips_non_import() {
        let imports = extract_imports("let x = 1\n// import Fake\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_signatures_finds_raw_param() {
        let content = "func getUser(userId: String) -> User { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.swift", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn scan_signatures_with_label() {
        let content = "func getUser(for userId: String) -> User { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.swift", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn scan_signatures_skips_private() {
        let content = "private func helper(userId: String) { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.swift", &type_map, &HashMap::new(), &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_signatures_handles_optional() {
        let content = "func getUser(userId: String?) -> User? { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.swift", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn scan_switch_incomplete() {
        let content =
            "switch status {\ncase .Pending:\n    handle()\ncase .Confirmed:\n    handle()\n}\n";
        let variants: HashSet<String> =
            ["Pending", "Confirmed", "Shipped", "Delivered", "Cancelled"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let mut vi = HashMap::new();
        for v in &variants {
            vi.insert(v.clone(), ("OrderStatus".to_string(), variants.clone()));
        }
        let mut um = HashMap::new();
        um.insert("OrderStatus".to_string(), variants);
        let mut hits = Vec::new();
        scan_matches(content, "test.swift", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_switch_with_default() {
        let content =
            "switch status {\ncase .Pending:\n    handle()\ndefault:\n    fallback()\n}\n";
        let variants: HashSet<String> = ["Pending", "Confirmed"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut vi = HashMap::new();
        for v in &variants {
            vi.insert(v.clone(), ("OrderStatus".to_string(), variants.clone()));
        }
        let mut um = HashMap::new();
        um.insert("OrderStatus".to_string(), variants);
        let mut hits = Vec::new();
        scan_matches(content, "test.swift", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_switch_string_cases() {
        let content = "switch status {\ncase \"Pending\":\n    handle()\ncase \"Confirmed\":\n    handle()\n}\n";
        let variants: HashSet<String> =
            ["Pending", "Confirmed", "Shipped", "Delivered", "Cancelled"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let mut vi = HashMap::new();
        for v in &variants {
            vi.insert(v.clone(), ("OrderStatus".to_string(), variants.clone()));
        }
        let mut um = HashMap::new();
        um.insert("OrderStatus".to_string(), variants);
        let mut hits = Vec::new();
        scan_matches(content, "test.swift", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
    }
}
