use super::{Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static JAVA: LangDef = LangDef {
    name: "java",
    extensions: &["java"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    test_file: TestFilePatterns {
        path_contains: &["test/"],
        filename_prefixes: &[],
        filename_suffixes: &["Test.java", "Tests.java"],
    },
    primitives: &[
        ("string", &["String"]),
        ("int", &["int", "long", "Integer", "Long"]),
        ("float", &["float", "double", "Float", "Double"]),
        ("bool", &["boolean", "Boolean"]),
    ],
    purity_imports: &[
        ("file_io", &["java.io", "java.nio"]),
        ("network_io", &["java.net"]),
        ("system_clock", &["java.time", "java.util.Date"]),
        ("subprocess", &["java.lang.ProcessBuilder"]),
        ("process", &["System.exit"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "new FileInputStream(",
                "new FileOutputStream(",
                "new FileReader(",
                "new FileWriter(",
                "new BufferedReader(",
                "new BufferedWriter(",
                "Files.readString(",
                "Files.writeString(",
                "Files.readAllBytes(",
                "Files.write(",
            ],
        ),
        (
            "network_io",
            &[
                "new URL(",
                "new Socket(",
                "new ServerSocket(",
                "HttpClient.newHttpClient(",
            ],
        ),
        (
            "stdout",
            &[
                "System.out.println(",
                "System.out.print(",
                "System.out.printf(",
            ],
        ),
        ("stderr", &["System.err.println(", "System.err.print("]),
        ("env_vars", &["System.getenv("]),
        (
            "system_clock",
            &[
                "System.currentTimeMillis(",
                "System.nanoTime(",
                "Instant.now(",
                "LocalDateTime.now(",
                "LocalDate.now(",
            ],
        ),
        (
            "subprocess",
            &["new ProcessBuilder(", "Runtime.getRuntime().exec("],
        ),
        ("process", &["System.exit("]),
        ("dynamic_execution", &["Class.forName(", ".newInstance("]),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "String"),
        ("int", "long"),
        ("float", "double"),
        ("bool", "boolean"),
    ],
    generate_preamble: Some(java_preamble),
    generate_newtype: Some(java_newtype),
    generate_union: Some(java_union),
    generate_match_scaffold: Some(java_match_scaffold),
    type_file_name: "Types.java",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // import com.example.Foo;  or  import static com.example.Foo.bar;
        if let Some(rest) = trimmed.strip_prefix("import ") {
            let rest = rest.strip_prefix("static ").unwrap_or(rest);
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

        let is_method = (trimmed.starts_with("public ")
            || trimmed.starts_with("protected ")
            || trimmed.starts_with("static ")
            || trimmed.starts_with("abstract ")
            || trimmed.starts_with("final ")
            || trimmed.starts_with("synchronized "))
            && trimmed.contains('(')
            && !trimmed.starts_with("public class ")
            && !trimmed.starts_with("public interface ")
            && !trimmed.starts_with("public enum ");

        if !is_method {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name and return type: word immediately before "(" is the name,
        // word before that is the return type (e.g., "public String getUserId(")
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
            let parts: Vec<&str> = param.split_whitespace().collect();
            let (type_ann, name) = if parts.len() >= 2 {
                let name = parts[parts.len() - 1];
                let type_ann = parts[parts.len() - 2];
                (type_ann, name)
            } else {
                continue;
            };

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

        // Check return type (word before function name, skip modifiers/void)
        if let Some(ret) = &return_type {
            let skip = ["void", "public", "protected", "static", "abstract",
                        "final", "synchronized", "default", "native"];
            if !skip.contains(&ret.as_str()) {
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

                if sub_trimmed.starts_with("default:")
                    || sub_trimmed.starts_with("default :")
                    || sub_trimmed.starts_with("default ->")
                {
                    has_default = true;
                    break;
                }
                if sub_trimmed.starts_with("case ") {
                    let rest = sub_trimmed.trim_start_matches("case ");
                    let case_val = rest
                        .split(':')
                        .next()
                        .unwrap_or("")
                        .split("->")
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

fn java_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    String::new()
}

fn java_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(val) = validation {
        out.push_str(&format!("/** TODO: implement {val} validation */\n"));
    }
    out.push_str(&format!("public record {name}({prim} value) {{ }}\n"));
    out
}

fn java_union(name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("public enum {name} {{\n"));
    let joined: Vec<String> = variants.iter().map(|v| format!("    {v}")).collect();
    out.push_str(&joined.join(",\n"));
    out.push_str("\n}\n");
    out
}

fn java_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let param = union_name[..1].to_lowercase() + &union_name[1..];
    let mut out = String::new();
    out.push_str("/** Exhaustive switch scaffold — all variants must be handled. */\n");
    out.push_str(&format!(
        "public static void handle{union_name}(String {param}) {{\n"
    ));
    out.push_str(&format!("    switch ({param}) {{\n"));
    for v in variants {
        out.push_str(&format!("        case \"{v}\":\n"));
        out.push_str("            throw new UnsupportedOperationException(\"TODO\");\n");
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
        let imports = extract_imports("import com.example.models.User;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "com.example.models.User");
    }

    #[test]
    fn import_static() {
        let imports = extract_imports("import static org.junit.Assert.assertEquals;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "org.junit.Assert.assertEquals");
    }

    #[test]
    fn import_wildcard() {
        let imports = extract_imports("import java.util.*;\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "java.util.*");
    }
}
