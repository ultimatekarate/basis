use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::language::LangRegistry;
use crate::spec::BasisSpec;

/// A completeness violation — match/switch doesn't cover all union variants.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub union_name: String,
    pub missing_variants: Vec<String>,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B003]: match on '{}' is not exhaustive\n  --> {}:{}\n  = note: missing variants: {}\n  = help: add case arms for {} or add a wildcard (_) pattern",
            self.union_name,
            self.file, self.line,
            self.missing_variants.join(", "),
            self.missing_variants.join(", ")
        )
    }
}

/// Build a map from variant name → (union_name, all variants in that union).
fn build_variant_index(spec: &BasisSpec) -> HashMap<String, (String, HashSet<String>)> {
    let mut index: HashMap<String, (String, HashSet<String>)> = HashMap::new();
    if let Some(exhaustive) = &spec.exhaustive_matching {
        if !exhaustive.enabled {
            return index;
        }
        for union_def in &exhaustive.unions {
            let variant_set: HashSet<String> = union_def.variants.iter().cloned().collect();
            for variant in &union_def.variants {
                index.insert(
                    variant.clone(),
                    (union_def.name.clone(), variant_set.clone()),
                );
            }
        }
    }
    index
}

/// Check all source files for non-exhaustive match/switch statements.
pub fn check_completeness(
    spec: &BasisSpec,
    root: &Path,
    registry: &LangRegistry,
) -> Vec<Violation> {
    let variant_index = build_variant_index(spec);
    if variant_index.is_empty() {
        return Vec::new();
    }

    // Also build union_name → variants for direct lookup
    let mut union_map: HashMap<String, HashSet<String>> = HashMap::new();
    if let Some(exhaustive) = &spec.exhaustive_matching {
        for union_def in &exhaustive.unions {
            union_map.insert(
                union_def.name.clone(),
                union_def.variants.iter().cloned().collect(),
            );
        }
    }

    let mut violations = Vec::new();

    super::walk::walk_source_files(root, &mut |file_path| {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = registry.for_ext(ext) else {
            return;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let mut hits = Vec::new();
        (lang.scan_matches)(&content, &rel, &variant_index, &union_map, &mut hits);

        for hit in hits {
            violations.push(Violation {
                file: rel.clone(),
                line: hit.line,
                union_name: hit.union_name,
                missing_variants: hit.missing_variants,
            });
        }
    });

    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language;

    fn make_variant_index() -> HashMap<String, (String, HashSet<String>)> {
        let variants: HashSet<String> =
            ["Pending", "Confirmed", "Shipped", "Delivered", "Cancelled"]
                .iter()
                .map(|s| s.to_string())
                .collect();
        let mut index = HashMap::new();
        for v in &variants {
            index.insert(v.clone(), ("OrderStatus".to_string(), variants.clone()));
        }
        index
    }

    fn make_union_map() -> HashMap<String, HashSet<String>> {
        let mut map = HashMap::new();
        map.insert(
            "OrderStatus".to_string(),
            ["Pending", "Confirmed", "Shipped", "Delivered", "Cancelled"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        map
    }

    // ── Python helpers ─────────────────────────────────────

    #[test]
    fn extract_python_case_string_literal() {
        assert_eq!(
            language::python::extract_python_case_value_pub("        case \"Pending\":"),
            Some("Pending".into())
        );
    }

    #[test]
    fn extract_python_case_dotted() {
        assert_eq!(
            language::python::extract_python_case_value_pub("        case OrderStatus.Pending:"),
            Some("Pending".into())
        );
    }

    #[test]
    fn extract_python_case_class_pattern() {
        assert_eq!(
            language::python::extract_python_case_value_pub("        case Pending():"),
            Some("Pending".into())
        );
    }

    #[test]
    fn extract_python_case_simple_name() {
        assert_eq!(
            language::python::extract_python_case_value_pub("        case Pending:"),
            Some("Pending".into())
        );
    }

    #[test]
    fn extract_comparison_string() {
        assert_eq!(
            language::python::extract_comparison_value_pub("    if status == \"Pending\":"),
            Some("Pending".into())
        );
    }

    #[test]
    fn extract_comparison_dotted() {
        assert_eq!(
            language::python::extract_comparison_value_pub("    if status == OrderStatus.Pending:"),
            Some("Pending".into())
        );
    }

    #[test]
    fn extract_comparison_name() {
        assert_eq!(
            language::python::extract_comparison_value_pub("    if status == Pending:"),
            Some("Pending".into())
        );
    }

    #[test]
    fn indentation_level_works() {
        assert_eq!(language::python::indentation_level_pub("hello"), 0);
        assert_eq!(language::python::indentation_level_pub("    hello"), 4);
        assert_eq!(language::python::indentation_level_pub("        hello"), 8);
    }

    // ── find_union_for_cases ──────────────────────────────

    #[test]
    fn find_union_partial_match() {
        let vi = make_variant_index();
        let um = make_union_map();
        let cases: HashSet<String> = ["Pending", "Confirmed"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = language::find_union_for_cases(&cases, &vi, &um);
        assert!(result.is_some());
        let (name, missing) = result.unwrap();
        assert_eq!(name, "OrderStatus");
        assert!(missing.contains(&"Shipped".to_string()));
        assert!(missing.contains(&"Delivered".to_string()));
        assert!(missing.contains(&"Cancelled".to_string()));
    }

    #[test]
    fn find_union_complete_match() {
        let vi = make_variant_index();
        let um = make_union_map();
        let cases: HashSet<String> = ["Pending", "Confirmed", "Shipped", "Delivered", "Cancelled"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let result = language::find_union_for_cases(&cases, &vi, &um);
        assert!(result.is_none());
    }

    #[test]
    fn find_union_no_match() {
        let vi = make_variant_index();
        let um = make_union_map();
        let cases: HashSet<String> = ["Foo", "Bar"].iter().map(|s| s.to_string()).collect();
        let result = language::find_union_for_cases(&cases, &vi, &um);
        assert!(result.is_none());
    }

    #[test]
    fn find_union_empty_cases() {
        let vi = make_variant_index();
        let um = make_union_map();
        let cases: HashSet<String> = HashSet::new();
        let result = language::find_union_for_cases(&cases, &vi, &um);
        assert!(result.is_none());
    }

    // ── Language-specific scan tests ──────────────────────

    #[test]
    fn scan_python_match_incomplete() {
        let content = "def handle(s):\n    match s:\n        case \"Pending\":\n            pass\n        case \"Confirmed\":\n            pass\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_matches)(content, "test.py", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
        assert!(hits[0].missing_variants.contains(&"Shipped".to_string()));
    }

    #[test]
    fn scan_python_match_wildcard() {
        let content = "def handle(s):\n    match s:\n        case \"Pending\":\n            pass\n        case _:\n            pass\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_matches)(content, "test.py", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_python_elif_incomplete() {
        let content =
            "if status == \"Pending\":\n    pass\nelif status == \"Confirmed\":\n    pass\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_matches)(content, "test.py", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn scan_python_elif_with_else() {
        let content = "if status == \"Pending\":\n    pass\nelif status == \"Confirmed\":\n    pass\nelse:\n    pass\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_matches)(content, "test.py", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_rust_match_incomplete() {
        let content = "match status {\n    OrderStatus::Pending => {},\n    OrderStatus::Confirmed => {},\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::rust_lang::RUST.scan_matches)(content, "test.rs", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_rust_match_wildcard() {
        let content = "match status {\n    OrderStatus::Pending => {},\n    _ => {},\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::rust_lang::RUST.scan_matches)(content, "test.rs", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_js_switch_incomplete() {
        let content = "switch (status) {\n    case \"Pending\":\n        break;\n    case \"Confirmed\":\n        break;\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_matches)(content, "test.ts", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_js_switch_with_default() {
        let content = "switch (status) {\n    case \"Pending\":\n        break;\n    default:\n        break;\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_matches)(content, "test.ts", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_go_switch_incomplete() {
        let content =
            "switch status {\ncase \"Pending\":\n\treturn 1\ncase \"Confirmed\":\n\treturn 2\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::go::GO.scan_matches)(content, "test.go", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_go_switch_with_default() {
        let content = "switch status {\ncase \"Pending\":\n\treturn 1\ndefault:\n\treturn 0\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::go::GO.scan_matches)(content, "test.go", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_java_switch_incomplete() {
        let content = "switch (status) {\n    case \"Pending\":\n        return 1;\n    case \"Confirmed\":\n        return 2;\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::java::JAVA.scan_matches)(content, "Test.java", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn scan_java_switch_with_default() {
        let content = "switch (status) {\n    case \"Pending\":\n        return 1;\n    default:\n        return 0;\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::java::JAVA.scan_matches)(content, "Test.java", &vi, &um, &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_java_switch_arrow_syntax() {
        let content =
            "switch (status) {\n    case \"Pending\" -> 1;\n    case \"Confirmed\" -> 2;\n}\n";
        let vi = make_variant_index();
        let um = make_union_map();
        let mut hits = Vec::new();
        (language::java::JAVA.scan_matches)(content, "Test.java", &vi, &um, &mut hits);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].union_name, "OrderStatus");
    }

    #[test]
    fn violation_display() {
        let v = Violation {
            file: "test.py".into(),
            line: 10,
            union_name: "OrderStatus".into(),
            missing_variants: vec!["Shipped".into(), "Delivered".into()],
        };
        let s = format!("{v}");
        assert!(s.contains("error[B003]"));
        assert!(s.contains("--> test.py:10"));
        assert!(s.contains("OrderStatus"));
        assert!(s.contains("Shipped"));
        assert!(s.contains("help: add case arms"));
    }
}
