use super::{FunctionContract, Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static CSHARP: LangDef = LangDef {
    name: "csharp",
    extensions: &["cs"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    scan_contracts,
    test_file: TestFilePatterns {
        path_contains: &["Tests/", "Test/", "tests/"],
        filename_prefixes: &[],
        filename_suffixes: &["Test.cs", "Tests.cs"],
    },
    primitives: &[
        ("string", &["string", "String"]),
        ("int", &["int", "long", "Int32", "Int64"]),
        (
            "float",
            &["float", "double", "decimal", "Single", "Double", "Decimal"],
        ),
        ("bool", &["bool", "Boolean"]),
    ],
    purity_imports: &[
        ("file_io", &["System.IO"]),
        ("network_io", &["System.Net", "System.Net.Http"]),
        (
            "system_clock",
            &["System.DateTime", "System.DateTimeOffset"],
        ),
        ("subprocess", &["System.Diagnostics.Process"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "File.ReadAllText(",
                "File.WriteAllText(",
                "File.ReadAllBytes(",
                "File.WriteAllBytes(",
                "File.ReadAllLines(",
                "File.WriteAllLines(",
                "File.Open(",
                "File.Create(",
                "File.Delete(",
                "File.Exists(",
                "File.Copy(",
                "File.Move(",
                "Directory.CreateDirectory(",
                "Directory.Delete(",
                "new StreamReader(",
                "new StreamWriter(",
                "new FileStream(",
            ],
        ),
        (
            "network_io",
            &[
                "HttpClient(",
                "new HttpClient(",
                ".GetAsync(",
                ".PostAsync(",
                ".PutAsync(",
                ".DeleteAsync(",
                ".GetStringAsync(",
                ".SendAsync(",
                "WebRequest.Create(",
            ],
        ),
        ("stdout", &["Console.Write(", "Console.WriteLine("]),
        (
            "stderr",
            &["Console.Error.Write(", "Console.Error.WriteLine("],
        ),
        ("env_vars", &["Environment.GetEnvironmentVariable("]),
        (
            "system_clock",
            &[
                "DateTime.Now",
                "DateTime.UtcNow",
                "DateTimeOffset.Now",
                "DateTimeOffset.UtcNow",
            ],
        ),
        ("subprocess", &["Process.Start(", "new Process("]),
        ("process", &["Environment.Exit("]),
        (
            "dynamic_execution",
            &["Activator.CreateInstance(", "Type.GetType("],
        ),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "string"),
        ("int", "long"),
        ("float", "double"),
        ("bool", "bool"),
    ],
    generate_preamble: Some(csharp_preamble),
    generate_newtype: Some(csharp_newtype),
    generate_union: Some(csharp_union),
    generate_match_scaffold: Some(csharp_match_scaffold),
    type_file_name: "Types.cs",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // using System.IO;  or  using static System.Math;
        if let Some(rest) = trimmed.strip_prefix("using ") {
            // Skip "using (" which is a using statement, not an import
            if rest.starts_with('(') {
                continue;
            }
            let rest = rest.strip_prefix("static ").unwrap_or(rest);
            // Skip alias: using Foo = Bar;
            if rest.contains('=') {
                // Still extract the target: "using Alias = Namespace.Type;"
                if let Some((_alias, target)) = rest.split_once('=') {
                    let module = target.trim().trim_end_matches(';').trim();
                    if !module.is_empty() {
                        imports.push(Import {
                            module: module.to_string(),
                            line: line_num + 1,
                        });
                    }
                }
                continue;
            }
            let module = rest.trim_end_matches(';').trim();
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

        if trimmed.contains("private ") {
            i += 1;
            continue;
        }

        // C# methods: [access] [static] ReturnType Name(params)
        let is_method = (trimmed.starts_with("public ")
            || trimmed.starts_with("protected ")
            || trimmed.starts_with("internal ")
            || trimmed.starts_with("static ")
            || trimmed.starts_with("virtual ")
            || trimmed.starts_with("override ")
            || trimmed.starts_with("abstract ")
            || trimmed.starts_with("async "))
            && trimmed.contains('(')
            && !trimmed.starts_with("public class ")
            && !trimmed.starts_with("public interface ")
            && !trimmed.starts_with("public enum ")
            && !trimmed.starts_with("public struct ")
            && !trimmed.starts_with("public record ");

        if !is_method {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name and return type: word before "(" is the name,
        // word before that is the return type
        let before_paren = trimmed.split('(').next().unwrap_or("");
        let words: Vec<&str> = before_paren.split_whitespace().collect();
        let fn_name = words.last().unwrap_or(&"").to_string();
        let return_type = if words.len() >= 2 {
            Some(words[words.len() - 2].to_string())
        } else {
            None
        };

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
            // Strip modifiers: ref, out, in, params, this
            let param = param.strip_prefix("ref ").unwrap_or(param);
            let param = param.strip_prefix("out ").unwrap_or(param);
            let param = param.strip_prefix("in ").unwrap_or(param);
            let param = param.strip_prefix("params ").unwrap_or(param);
            let param = param.strip_prefix("this ").unwrap_or(param);

            // C# params: Type name or Type? name
            let parts: Vec<&str> = param.split_whitespace().collect();
            if parts.len() >= 2 {
                let type_ann = parts[0].trim_end_matches('?');
                let name = parts[1].split('=').next().unwrap_or("").trim(); // strip default

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

        // Check return type (word before function name, skip modifiers/void)
        if let Some(ret) = &return_type {
            let ret = ret.trim_end_matches('?');
            let skip = ["void", "public", "protected", "internal", "static",
                        "virtual", "override", "abstract", "async", "partial"];
            if !skip.contains(&ret) {
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

        if trimmed.starts_with("switch") && trimmed.contains('(') {
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
                    let case_val = rest
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

fn scan_contracts(_content: &str) -> Vec<FunctionContract> {
    Vec::new()
}

fn csharp_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    String::new()
}

fn csharp_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(val) = validation {
        out.push_str(&format!("/// TODO: implement {val} validation\n"));
    }
    out.push_str(&format!(
        "public readonly record struct {name}({prim} Value);\n"
    ));
    out
}

fn csharp_union(name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("public enum {name} {{\n"));
    let joined: Vec<String> = variants.iter().map(|v| format!("    {v}")).collect();
    out.push_str(&joined.join(",\n"));
    out.push_str("\n}\n");
    out
}

fn csharp_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let param = union_name[..1].to_lowercase() + &union_name[1..];
    let mut out = String::new();
    out.push_str("/// Exhaustive switch scaffold — all variants must be handled.\n");
    out.push_str(&format!(
        "public static void Handle{union_name}(string {param}) {{\n"
    ));
    out.push_str(&format!("    switch ({param}) {{\n"));
    for v in variants {
        out.push_str(&format!("        case \"{v}\":\n"));
        out.push_str("            throw new NotImplementedException(\"TODO\");\n");
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
        let imports = extract_imports("using System.IO;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "System.IO");
    }

    #[test]
    fn import_static() {
        let imports = extract_imports("using static System.Math;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "System.Math");
    }

    #[test]
    fn import_alias() {
        let imports = extract_imports("using Env = System.Environment;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "System.Environment");
    }

    #[test]
    fn import_skips_using_statement() {
        let imports = extract_imports("using (var stream = File.Open()) { }\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn import_multiple() {
        let imports = extract_imports("using System;\nusing System.IO;\nusing System.Net.Http;\n");
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].module, "System");
        assert_eq!(imports[1].module, "System.IO");
        assert_eq!(imports[2].module, "System.Net.Http");
    }

    #[test]
    fn scan_signatures_finds_raw_param() {
        let content = "public User GetUser(string userId) {\n}\n";
        let mut type_map = HashMap::new();
        type_map.insert("string".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "Test.cs", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
        assert_eq!(hits[0].raw_type, "string");
    }

    #[test]
    fn scan_signatures_skips_private() {
        let content = "private void Helper(string userId) { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("string".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "Test.cs", &type_map, &HashMap::new(), &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_signatures_handles_nullable() {
        let content = "public User GetUser(string? userId) {\n}\n";
        let mut type_map = HashMap::new();
        type_map.insert("string".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "Test.cs", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn scan_switch_incomplete() {
        let content = "switch (status) {\n    case \"Pending\":\n        break;\n    case \"Confirmed\":\n        break;\n}\n";
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
        scan_matches(content, "Test.cs", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_switch_with_default() {
        let content = "switch (status) {\n    case \"Pending\":\n        break;\n    default:\n        break;\n}\n";
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
        scan_matches(content, "Test.cs", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }
}
