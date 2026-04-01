use super::{Import, LangDef, MatchHit, SignatureHit, TestFilePatterns};
use std::collections::{HashMap, HashSet};

pub static RUBY: LangDef = LangDef {
    name: "ruby",
    extensions: &["rb"],
    comment_prefix: "#",
    extract_imports,
    scan_signatures,
    scan_matches,
    test_file: TestFilePatterns {
        path_contains: &["test/", "spec/"],
        filename_prefixes: &["test_"],
        filename_suffixes: &["_test.rb", "_spec.rb"],
    },
    primitives: &[
        // Ruby is dynamically typed — Sorbet/RBS type annotations use these
        ("string", &["String"]),
        ("int", &["Integer"]),
        ("float", &["Float"]),
        ("bool", &["T::Boolean"]),
    ],
    purity_imports: &[
        ("file_io", &["fileutils", "tempfile", "pathname", "csv"]),
        (
            "network_io",
            &[
                "net/http",
                "net/https",
                "open-uri",
                "httparty",
                "faraday",
                "rest-client",
                "typhoeus",
            ],
        ),
        ("system_clock", &["time", "date"]),
        ("subprocess", &["open3"]),
        ("process", &["kernel"]),
    ],
    purity_calls: &[
        (
            "file_io",
            &[
                "File.open(",
                "File.read(",
                "File.write(",
                "File.delete(",
                "File.exist",
                "File.new(",
                "File.readlines(",
                "IO.read(",
                "IO.write(",
                "FileUtils.",
                "Dir.mkdir(",
                "Dir.glob(",
            ],
        ),
        (
            "network_io",
            &[
                "Net::HTTP",
                "URI.parse(",
                "URI(",
                "HTTParty.",
                "Faraday.",
                "RestClient.",
            ],
        ),
        (
            "stdout",
            &["puts(", "puts ", "print(", "print ", "pp(", "pp ", "p("],
        ),
        ("stderr", &["warn(", "warn ", "$stderr.puts", "STDERR.puts"]),
        ("env_vars", &["ENV["]),
        (
            "system_clock",
            &["Time.now", "Time.new", "Date.today", "DateTime.now"],
        ),
        ("subprocess", &["system(", "exec(", "`", "Open3.", "spawn("]),
        ("process", &["exit(", "exit!(", "abort(", "abort "]),
        (
            "dynamic_execution",
            &[
                "eval(",
                "class_eval(",
                "instance_eval(",
                "send(",
                "public_send(",
            ],
        ),
    ],
    preprocess: None,
    preferred_type: &[
        ("string", "String"),
        ("int", "Integer"),
        ("float", "Float"),
        ("bool", "T::Boolean"),
    ],
    generate_preamble: Some(ruby_preamble),
    generate_newtype: Some(ruby_newtype),
    generate_union: Some(ruby_union),
    generate_match_scaffold: Some(ruby_match_scaffold),
    type_file_name: "types.rb",
};

pub fn extract_imports(content: &str) -> Vec<Import> {
    let mut imports = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // require "module" or require 'module'
        if let Some(rest) = trimmed.strip_prefix("require ") {
            if let Some(module) = extract_quoted(rest) {
                imports.push(Import {
                    module,
                    line: line_num + 1,
                });
            }
        }
        // require_relative "path"
        else if let Some(rest) = trimmed.strip_prefix("require_relative ") {
            if let Some(module) = extract_quoted(rest) {
                imports.push(Import {
                    module,
                    line: line_num + 1,
                });
            }
        }
        // Bundler: gem "name" in Gemfile (less common in source, but useful)
    }
    imports
}

fn extract_quoted(s: &str) -> Option<String> {
    let s = s.trim();
    let (quote, rest) = if let Some(stripped) = s.strip_prefix('"') {
        ('"', stripped)
    } else if let Some(stripped) = s.strip_prefix('\'') {
        ('\'', stripped)
    } else {
        return None;
    };
    let end = rest.find(quote)?;
    let module = &rest[..end];
    if module.is_empty() {
        None
    } else {
        Some(module.to_string())
    }
}

fn scan_signatures(
    content: &str,
    _file: &str,
    type_map: &HashMap<String, Vec<String>>,
    _name_hints: &HashMap<String, Vec<String>>,
    out: &mut Vec<SignatureHit>,
) {
    // Ruby is dynamically typed, so signature scanning is limited.
    // We look for Sorbet sig blocks: sig { params(name: Type).returns(Type) }
    // and for YARD @param annotations.
    let lines: Vec<&str> = content.lines().collect();

    for (line_num, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Sorbet inline: sig { params(user_id: String).returns(User) }
        if trimmed.starts_with("sig {") || trimmed.starts_with("sig do") {
            // Find the def line after this sig block to extract the function name
            let fn_name = {
                let mut name = String::new();
                for subsequent in &lines[line_num + 1..] {
                    let sub = subsequent.trim();
                    if let Some(rest) = sub.strip_prefix("def ") {
                        let rest = rest.strip_prefix("self.").unwrap_or(rest);
                        name = rest.split(&['(', ' '][..]).next().unwrap_or("").to_string();
                        break;
                    }
                }
                name
            };

            if let Some(params_start) = trimmed.find("params(") {
                let rest = &trimmed[params_start + 7..];
                let params_text = rest.split(')').next().unwrap_or("");
                scan_sorbet_params(params_text, line_num + 1, &fn_name, type_map, out);
            }
            // Multi-line sig block: look at subsequent lines for params
            if !trimmed.contains("params(") {
                for subsequent in &lines[line_num + 1..] {
                    let sub = subsequent.trim();
                    if sub.contains("params(") {
                        if let Some(params_start) = sub.find("params(") {
                            let rest = &sub[params_start + 7..];
                            let params_text = rest.split(')').next().unwrap_or("");
                            scan_sorbet_params(params_text, line_num + 1, &fn_name, type_map, out);
                        }
                        break;
                    }
                    if sub.contains('}') || sub.starts_with("end") || sub.starts_with("def ") {
                        break;
                    }
                }
            }
        }
    }
}

fn scan_sorbet_params(
    params_text: &str,
    line: usize,
    fn_name: &str,
    type_map: &HashMap<String, Vec<String>>,
    out: &mut Vec<SignatureHit>,
) {
    // params_text: "user_id: String, order_id: String"
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
                            line,
                            function_name: fn_name.to_string(),
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

        // Ruby case/when: "case subject"
        if trimmed.starts_with("case ") || trimmed == "case" {
            let mut cases: HashSet<String> = HashSet::new();
            let mut has_else = false;

            for subsequent in &lines[line_num + 1..] {
                let sub_trimmed = subsequent.trim();

                if sub_trimmed == "end" {
                    break;
                }

                if sub_trimmed.starts_with("else") {
                    has_else = true;
                    break;
                }

                if sub_trimmed.starts_with("when ") {
                    let rest = sub_trimmed.trim_start_matches("when ");
                    // Handle comma-separated: "when :pending, :confirmed"
                    // and "when" followed by "then"
                    let pattern = rest.split("then").next().unwrap_or(rest).trim();
                    for part in pattern.split(',') {
                        let val = part.trim();
                        // Symbol: :pending
                        let val = val.trim_start_matches(':');
                        // String: "Pending" or 'Pending'
                        let val = val.trim_matches('"').trim_matches('\'');
                        // Constant: OrderStatus::Pending or OrderStatus::PENDING
                        let variant = val.rsplit("::").next().unwrap_or(val);
                        // Convert Ruby snake_case to PascalCase for variant matching
                        let variant = snake_to_pascal(variant);
                        if !variant.is_empty() {
                            cases.insert(variant);
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

/// Convert snake_case to PascalCase for matching Ruby conventions against spec variants.
/// "pending" → "Pending", "order_status" → "OrderStatus", "Pending" → "Pending"
fn snake_to_pascal(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    // If already PascalCase (starts with uppercase, no underscores), pass through
    if s.chars().next().is_some_and(|c| c.is_uppercase()) && !s.contains('_') {
        return s.to_string();
    }
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.extend(chars);
                    s
                }
                None => String::new(),
            }
        })
        .collect()
}

fn ruby_preamble(_has_newtypes: bool, _has_unions: bool) -> String {
    "# frozen_string_literal: true\n".to_string()
}

fn ruby_newtype(name: &str, prim: &str, validation: Option<&str>) -> String {
    let _ = prim; // Ruby is dynamically typed, primitive name is for documentation
    let mut out = String::new();
    out.push_str(&format!("{name} = Data.define(:value)\n"));
    if let Some(val) = validation {
        out.push('\n');
        let fn_name = to_ruby_snake(name);
        out.push_str(&format!("# TODO: implement {val} validation\n"));
        out.push_str(&format!("def validate_{fn_name}(value)\n"));
        out.push_str("  raise NotImplementedError\n");
        out.push_str("end\n");
    }
    out
}

fn ruby_union(_name: &str, variants: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&format!("module {_name}\n"));
    for v in variants {
        out.push_str(&format!("  {v} = \"{v}\"\n"));
    }
    out.push_str("end\n");
    out
}

fn ruby_match_scaffold(union_name: &str, variants: &[String]) -> String {
    let fn_name = to_ruby_snake(union_name);
    let mut out = String::new();
    out.push_str("# Exhaustive case scaffold — all variants must be handled.\n");
    out.push_str(&format!("def handle_{fn_name}(status)\n"));
    out.push_str("  case status\n");
    for v in variants {
        out.push_str(&format!("  when \"{v}\"\n"));
        out.push_str("    raise NotImplementedError # TODO\n");
    }
    out.push_str("  end\n");
    out.push_str("end\n");
    out
}

fn to_ruby_snake(s: &str) -> String {
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

    // ── Imports ──────────────────────────────────────────────

    #[test]
    fn require_double_quotes() {
        let imports = extract_imports("require \"json\"\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "json");
    }

    #[test]
    fn require_single_quotes() {
        let imports = extract_imports("require 'net/http'\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "net/http");
    }

    #[test]
    fn require_relative() {
        let imports = extract_imports("require_relative 'models/user'\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module, "models/user");
    }

    #[test]
    fn multiple_requires() {
        let imports =
            extract_imports("require 'json'\nrequire 'net/http'\nrequire_relative 'config'\n");
        assert_eq!(imports.len(), 3);
        assert_eq!(imports[0].module, "json");
        assert_eq!(imports[1].module, "net/http");
        assert_eq!(imports[2].module, "config");
    }

    #[test]
    fn skips_non_require_lines() {
        let imports = extract_imports("x = 1\n# require 'fake'\nputs 'hello'\n");
        assert!(imports.is_empty());
    }

    // ── snake_to_pascal ─────────────────────────────────────

    #[test]
    fn snake_to_pascal_simple() {
        assert_eq!(snake_to_pascal("pending"), "Pending");
        assert_eq!(snake_to_pascal("order_status"), "OrderStatus");
    }

    #[test]
    fn snake_to_pascal_already_pascal() {
        assert_eq!(snake_to_pascal("Pending"), "Pending");
        assert_eq!(snake_to_pascal("OrderStatus"), "OrderStatus");
    }

    #[test]
    fn snake_to_pascal_empty() {
        assert_eq!(snake_to_pascal(""), "");
    }

    // ── Sorbet signatures ───────────────────────────────────

    #[test]
    fn sorbet_sig_inline() {
        let content = "sig { params(user_id: String).returns(User) }\ndef get_user(user_id)\nend\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.rb", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
        assert_eq!(hits[0].raw_type, "String");
    }

    #[test]
    fn sorbet_sig_multiline() {
        let content = "sig do\n  params(user_id: String)\n  .returns(User)\nend\ndef get_user(user_id)\nend\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.rb", &type_map, &HashMap::new(), &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
    }

    #[test]
    fn no_sig_no_hit() {
        let content = "def get_user(user_id)\n  User.find(user_id)\nend\n";
        let mut type_map = HashMap::new();
        type_map.insert("String".to_string(), vec!["UserId".to_string()]);
        let mut hits = Vec::new();
        scan_signatures(content, "test.rb", &type_map, &HashMap::new(), &mut hits);
        assert!(hits.is_empty());
    }

    // ── case/when ───────────────────────────────────────────

    #[test]
    fn case_when_string_incomplete() {
        let content =
            "case status\nwhen \"Pending\"\n  handle\nwhen \"Confirmed\"\n  handle\nend\n";
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
        scan_matches(content, "test.rb", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
        assert!(hits[0].missing_variants.contains(&"Shipped".to_string()));
    }

    #[test]
    fn case_when_symbol_incomplete() {
        let content = "case status\nwhen :pending\n  handle\nwhen :confirmed\n  handle\nend\n";
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
        scan_matches(content, "test.rb", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn case_when_with_else() {
        let content = "case status\nwhen \"Pending\"\n  handle\nelse\n  fallback\nend\n";
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
        scan_matches(content, "test.rb", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn case_when_constant_pattern() {
        let content = "case status\nwhen OrderStatus::Pending\n  handle\nwhen OrderStatus::Confirmed\n  handle\nend\n";
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
        scan_matches(content, "test.rb", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn case_when_comma_separated() {
        let content = "case status\nwhen \"Pending\", \"Confirmed\"\n  handle\nend\n";
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
        scan_matches(content, "test.rb", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        // Both Pending and Confirmed matched, so 3 missing
        assert_eq!(hits[0].missing_variants.len(), 3);
    }
}
