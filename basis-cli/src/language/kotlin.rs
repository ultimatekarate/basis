use super::{Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static KOTLIN: LangDef = LangDef {
    name: "kotlin",
    extensions: &["kt", "kts"],
    comment_prefix: "//",
    extract_imports,
    scan_signatures,
    scan_matches,
    test_file: TestFilePatterns {
        path_contains: &["test/"],
        filename_prefixes: &[],
        filename_suffixes: &["Test.kt", "Tests.kt", "Test.kts"],
    },
    primitives: &[
        ("string", &["String"]),
        ("int", &["Int", "Long"]),
        ("float", &["Float", "Double"]),
        ("bool", &["Boolean"]),
    ],
    purity_imports: &[
        ("file_io", &["java.io", "java.nio", "kotlin.io"]),
        (
            "network_io",
            &["java.net", "io.ktor", "okhttp3", "retrofit2"],
        ),
        ("system_clock", &["java.time", "kotlinx.datetime"]),
        ("subprocess", &["java.lang.ProcessBuilder"]),
        ("process", &["kotlin.system.exitProcess", "System.exit"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "File(",
                "readText(",
                "writeText(",
                "readLines(",
                "readBytes(",
                "writeBytes(",
                "BufferedReader(",
                "BufferedWriter(",
                "FileInputStream(",
                "FileOutputStream(",
            ],
        ),
        ("network_io", &["URL(", "HttpClient(", "OkHttpClient("]),
        ("stdout", &["println(", "print("]),
        ("stderr", &["System.err.println(", "System.err.print("]),
        ("env_vars", &["System.getenv("]),
        (
            "system_clock",
            &[
                "System.currentTimeMillis(",
                "System.nanoTime(",
                "Clock.System.now(",
                "Instant.now(",
                "LocalDateTime.now(",
            ],
        ),
        (
            "subprocess",
            &["ProcessBuilder(", "Runtime.getRuntime().exec("],
        ),
        ("process", &["exitProcess(", "System.exit("]),
        ("dynamic_execution", &["Class.forName("]),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "String"),
        ("int", "Long"),
        ("float", "Double"),
        ("bool", "Boolean"),
    ],
    generate_preamble: Some(kotlin_preamble),
    generate_newtype: Some(kotlin_newtype),
    generate_union: Some(kotlin_union),
    generate_match_scaffold: Some(kotlin_match_scaffold),
    type_file_name: "Types.kt",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("import ") {
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

        if trimmed.contains("private ") || trimmed.contains("internal ") {
            i += 1;
            continue;
        }

        // fun name(params) or suspend fun name(params)
        let is_fun = trimmed.starts_with("fun ")
            || trimmed.starts_with("suspend fun ")
            || trimmed.starts_with("override fun ")
            || trimmed.starts_with("open fun ")
            || trimmed.starts_with("abstract fun ");

        if !is_fun {
            i += 1;
            continue;
        }

        let decl_line = i;

        // Extract function name: between "fun " and "("
        let fn_name = {
            let fun_pos = trimmed.find("fun ").unwrap_or(0) + 4;
            let after_fun = &trimmed[fun_pos..];
            after_fun
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
            if param.is_empty() {
                continue;
            }
            // Kotlin: name: Type or vararg name: Type
            let param = param.strip_prefix("vararg ").unwrap_or(param);
            if let Some((name, type_ann)) = param.split_once(':') {
                let name = name.trim();
                let type_ann = type_ann
                    .trim()
                    .split('=')
                    .next()
                    .unwrap_or("")
                    .trim() // strip default value
                    .trim_end_matches('?'); // strip nullable marker

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

        // Kotlin when expression: "when (subject) {" or "when {"
        if trimmed.starts_with("when") && trimmed.contains('{') {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_else = false;
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

                if sub_trimmed.starts_with("else") && sub_trimmed.contains("->") {
                    has_else = true;
                    break;
                }

                if sub_trimmed.contains("->") {
                    let pattern = sub_trimmed.split("->").next().unwrap_or("").trim();
                    // Handle comma-separated patterns: "Pending, Confirmed ->"
                    for part in pattern.split(',') {
                        let val = part.trim().trim_matches('"').trim_matches('\'');
                        let variant = val.rsplit('.').next().unwrap_or(val);
                        if !variant.is_empty() && variant != "else" {
                            cases.insert(variant.to_string());
                        }
                    }
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

fn kotlin_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    String::new()
}

fn kotlin_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(val) = validation {
        out.push_str(&format!("/** TODO: implement {val} validation */\n"));
    }
    out.push_str(&format!(
        "@JvmInline\nvalue class {name}(val value: {prim})\n"
    ));
    out
}

fn kotlin_union(name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("sealed class {name} {{\n"));
    for v in variants {
        out.push_str(&format!("    data object {v} : {name}()\n"));
    }
    out.push_str("}\n");
    out
}

fn kotlin_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let param = union_name[..1].to_lowercase() + &union_name[1..];
    let mut out = String::new();
    out.push_str("/** Exhaustive when scaffold — all variants must be handled. */\n");
    out.push_str(&format!("fun handle{union_name}({param}: String) {{\n"));
    out.push_str(&format!("    when ({param}) {{\n"));
    for v in variants {
        out.push_str(&format!("        \"{v}\" -> TODO()\n"));
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
        let imports = extract_imports("import com.example.models.User\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "com.example.models.User");
    }

    #[test]
    fn import_with_alias() {
        let imports = extract_imports("import java.util.Date\nimport kotlin.collections.List\n");
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "java.util.Date");
        assert_eq!(imports[1].module, "kotlin.collections.List");
    }

    #[test]
    fn import_skips_non_import() {
        let imports = extract_imports("val x = 1\n// import fake\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn scan_signatures_finds_raw_param() {
        let content = "fun getUser(userId: String): User { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.kt", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
        assert_eq!(hits[0].raw_type, "String");
    }

    #[test]
    fn scan_signatures_skips_private() {
        let content = "private fun helper(userId: String): Unit { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.kt", &type_map, &HashMap::new(), &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_signatures_handles_nullable() {
        let content = "fun getUser(userId: String?): User? { }\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.kt", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn scan_when_incomplete() {
        let content =
            "when (status) {\n    \"Pending\" -> handle()\n    \"Confirmed\" -> handle()\n}\n";
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
        scan_matches(content, "test.kt", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_when_with_else() {
        let content = "when (status) {\n    \"Pending\" -> handle()\n    else -> fallback()\n}\n";
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
        scan_matches(content, "test.kt", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }
}
