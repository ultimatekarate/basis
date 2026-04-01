use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::language::LangRegistry;
use crate::spec::{applies_to_lang, BasisSpec};

/// A value axis violation — raw primitive used where a branded newtype should be.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub param_name: String,
    pub raw_type: String,
    pub suggested: Vec<String>,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B002]: parameter '{}' uses raw '{}' instead of branded newtype\n  --> {}:{}\n  = help: use {} instead of {}",
            self.param_name, self.raw_type,
            self.file, self.line,
            self.suggested.join(" or "), self.raw_type
        )
    }
}

/// Build a map from primitive type name → list of newtype names that wrap it,
/// filtered to only include newtypes that apply to the given language.
fn build_type_map_for_lang(
    spec: &BasisSpec,
    lang_name: &str,
    lang_primitives: &[(&str, &[&str])],
) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(newtypes) = &spec.newtypes {
        if !newtypes.enabled {
            return map;
        }
        for nt in &newtypes.types {
            if !applies_to_lang(&nt.languages, lang_name) {
                continue;
            }
            for &(wraps_key, primitives) in lang_primitives {
                if wraps_key == nt.wraps {
                    for &prim in primitives {
                        map.entry(prim.to_string())
                            .or_default()
                            .push(nt.name.clone());
                    }
                }
            }
        }
    }
    // Deduplicate
    for names in map.values_mut() {
        names.sort();
        names.dedup();
    }
    map
}

/// Build a map from primitive type name → list of newtype names that wrap it.
/// Includes all newtypes across all languages (used by tests that don't need filtering).
#[cfg(test)]
fn build_type_map(spec: &BasisSpec, registry: &LangRegistry) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(newtypes) = &spec.newtypes {
        if !newtypes.enabled {
            return map;
        }
        for nt in &newtypes.types {
            for lang in registry.all() {
                for &(wraps_key, primitives) in lang.primitives {
                    if wraps_key == nt.wraps {
                        for &prim in primitives {
                            map.entry(prim.to_string())
                                .or_default()
                                .push(nt.name.clone());
                        }
                    }
                }
            }
        }
    }
    for names in map.values_mut() {
        names.sort();
        names.dedup();
    }
    map
}

/// Build snake_case → newtypes lookup, filtered to a specific language.
fn build_name_hints_for_lang(
    spec: &BasisSpec,
    lang_name: &str,
) -> HashMap<String, Vec<String>> {
    let mut name_hints: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(newtypes) = &spec.newtypes {
        for nt in &newtypes.types {
            if !applies_to_lang(&nt.languages, lang_name) {
                continue;
            }
            let snake = to_snake_case(&nt.name);
            name_hints.entry(snake).or_default().push(nt.name.clone());
        }
    }
    name_hints
}

/// Convert PascalCase newtype name to snake_case for matching.
fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }
    result
}

/// Check all source files for raw primitive usage at function boundaries.
pub fn check_values(spec: &BasisSpec, root: &Path, registry: &LangRegistry) -> Vec<Violation> {
    // Quick check: are newtypes enabled at all?
    let newtypes_enabled = spec
        .newtypes
        .as_ref()
        .is_some_and(|n| n.enabled && !n.types.is_empty());
    if !newtypes_enabled {
        return Vec::new();
    }

    // Build per-language type maps and name hints
    let mut type_maps: HashMap<&str, HashMap<String, Vec<String>>> = HashMap::new();
    let mut name_hints_map: HashMap<&str, HashMap<String, Vec<String>>> = HashMap::new();
    for lang in registry.all() {
        let tm = build_type_map_for_lang(spec, lang.name, lang.primitives);
        let nh = build_name_hints_for_lang(spec, lang.name);
        type_maps.insert(lang.name, tm);
        name_hints_map.insert(lang.name, nh);
    }

    // Build exclusion sets from spec
    let exclude_params: HashSet<&str> = spec
        .newtypes
        .as_ref()
        .map(|n| n.exclude_params.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    let exclude_functions: HashSet<&str> = spec
        .newtypes
        .as_ref()
        .map(|n| n.exclude_functions.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

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

        // Get the per-language type map and name hints
        let Some(type_map) = type_maps.get(lang.name) else {
            return;
        };
        if type_map.is_empty() {
            return;
        }
        let empty_hints = HashMap::new();
        let name_hints = name_hints_map.get(lang.name).unwrap_or(&empty_hints);

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let mut hits = Vec::new();
        (lang.scan_signatures)(&content, &rel, type_map, name_hints, &mut hits);

        // Filter: exclude_params, exclude_functions, inline basis:allow(B002)
        let lines: Vec<&str> = content.lines().collect();
        hits.retain(|hit| {
            // Spec-level: exclude by param name
            if exclude_params.contains(hit.param_name.as_str()) {
                return false;
            }
            // Spec-level: exclude by function name
            if !hit.function_name.is_empty()
                && exclude_functions.contains(hit.function_name.as_str())
            {
                return false;
            }
            // Inline: basis:allow(B002) on the declaration line or the line above
            let line_idx = hit.line.saturating_sub(1);
            if let Some(current) = lines.get(line_idx) {
                if current.contains("basis:allow(B002)") {
                    return false;
                }
            }
            if line_idx > 0 {
                if let Some(above) = lines.get(line_idx - 1) {
                    if above.contains("basis:allow(B002)") {
                        return false;
                    }
                }
            }
            true
        });

        for hit in hits {
            violations.push(Violation {
                file: rel.clone(),
                line: hit.line,
                param_name: hit.param_name,
                raw_type: hit.raw_type,
                suggested: hit.suggested,
            });
        }
    });

    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language;

    #[test]
    fn to_snake_case_simple() {
        assert_eq!(to_snake_case("UserId"), "user_id");
        assert_eq!(to_snake_case("OrderId"), "order_id");
        assert_eq!(to_snake_case("EmailAddress"), "email_address");
        assert_eq!(to_snake_case("PortNumber"), "port_number");
    }

    #[test]
    fn wraps_to_primitives_string() {
        // Verify Python's primitives table has "str" for "string"
        let python = &language::python::PYTHON;
        let string_prims: Vec<&&str> = python
            .primitives
            .iter()
            .filter(|(k, _)| *k == "string")
            .flat_map(|(_, v)| v.iter())
            .collect();
        assert!(string_prims.contains(&&"str"));
        // Verify Rust's primitives table has "String" for "string"
        let rust = &language::rust_lang::RUST;
        let string_prims: Vec<&&str> = rust
            .primitives
            .iter()
            .filter(|(k, _)| *k == "string")
            .flat_map(|(_, v)| v.iter())
            .collect();
        assert!(string_prims.contains(&&"String"));
        assert!(string_prims.contains(&&"&str"));
    }

    #[test]
    fn wraps_to_primitives_int() {
        let python = &language::python::PYTHON;
        let int_prims: Vec<&&str> = python
            .primitives
            .iter()
            .filter(|(k, _)| *k == "int")
            .flat_map(|(_, v)| v.iter())
            .collect();
        assert!(int_prims.contains(&&"int"));
        let rust = &language::rust_lang::RUST;
        let int_prims: Vec<&&str> = rust
            .primitives
            .iter()
            .filter(|(k, _)| *k == "int")
            .flat_map(|(_, v)| v.iter())
            .collect();
        assert!(int_prims.contains(&&"i32"));
    }

    #[test]
    fn wraps_to_primitives_unknown() {
        let python = &language::python::PYTHON;
        let custom_prims: Vec<&&str> = python
            .primitives
            .iter()
            .filter(|(k, _)| *k == "custom")
            .flat_map(|(_, v)| v.iter())
            .collect();
        assert!(custom_prims.is_empty());
    }

    #[test]
    fn build_type_map_groups_by_primitive() {
        let registry = language::LangRegistry::new();
        let spec = crate::spec::BasisSpec {
            governance: crate::spec::Governance {
                version: "1.0".into(),
                model: None,
            },
            layers: HashMap::new(),
            newtypes: Some(crate::spec::NewtypeConfig {
                enabled: true,
                types: vec![
                    crate::spec::NewtypeDef {
                        name: "UserId".into(),
                        wraps: "string".into(),
                        validation: None,
                        languages: None,
                    },
                    crate::spec::NewtypeDef {
                        name: "OrderId".into(),
                        wraps: "string".into(),
                        validation: None,
                        languages: None,
                    },
                ],
                exclude_params: vec![],
                exclude_functions: vec![],
            }),
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        };
        let map = build_type_map(&spec, &registry);
        assert!(map.contains_key("str"));
        let names = &map["str"];
        assert!(names.contains(&"UserId".to_string()));
        assert!(names.contains(&"OrderId".to_string()));
    }

    #[test]
    fn build_type_map_disabled() {
        let registry = language::LangRegistry::new();
        let spec = crate::spec::BasisSpec {
            governance: crate::spec::Governance {
                version: "1.0".into(),
                model: None,
            },
            layers: HashMap::new(),
            newtypes: Some(crate::spec::NewtypeConfig {
                enabled: false,
                types: vec![crate::spec::NewtypeDef {
                    name: "UserId".into(),
                    wraps: "string".into(),
                    validation: None,
                    languages: None,
                }],
                exclude_params: vec![],
                exclude_functions: vec![],
            }),
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        };
        assert!(build_type_map(&spec, &registry).is_empty());
    }

    fn make_type_map() -> HashMap<String, Vec<String>> {
        let mut m = HashMap::new();
        m.insert("str".to_string(), vec!["UserId".to_string()]);
        m.insert("String".to_string(), vec!["UserId".to_string()]);
        m.insert("string".to_string(), vec!["UserId".to_string()]);
        m
    }

    #[test]
    fn scan_python_finds_raw_param() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
        assert_eq!(hits[0].suggested, vec!["UserId"]);
    }

    #[test]
    fn scan_python_skips_private() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def _helper(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_python_skips_self_cls() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get(self, user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
    }

    #[test]
    fn scan_python_no_match_unrelated_name() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def process(data: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_rust_finds_raw_param() {
        let mut hits = Vec::new();
        (language::rust_lang::RUST.scan_signatures)(
            "pub fn get_user(user_id: String) -> () {}\n",
            "test.rs",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
    }

    #[test]
    fn scan_rust_skips_self() {
        let mut hits = Vec::new();
        (language::rust_lang::RUST.scan_signatures)(
            "fn process(&self, data: str) {}\n",
            "test.rs",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty()); // data doesn't match UserId
    }

    #[test]
    fn scan_ts_finds_raw_param() {
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_signatures)(
            "function getUser(userId: string): void {}\n",
            "test.ts",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn scan_go_finds_raw_param() {
        let mut hits = Vec::new();
        (language::go::GO.scan_signatures)(
            "func GetUser(userId string) error {\n",
            "test.go",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn scan_go_skips_unexported() {
        let mut hits = Vec::new();
        (language::go::GO.scan_signatures)(
            "func GetUser(_userId string) error {\n",
            "test.go",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_go_method_receiver() {
        let mut hits = Vec::new();
        (language::go::GO.scan_signatures)(
            "func (s *Service) GetUser(userId string) error {\n",
            "test.go",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn scan_java_finds_raw_param() {
        let mut hits = Vec::new();
        (language::java::JAVA.scan_signatures)(
            "public void getUser(String userId) {\n",
            "Test.java",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn scan_java_skips_private() {
        let mut hits = Vec::new();
        (language::java::JAVA.scan_signatures)(
            "private void getUser(String userId) {\n",
            "Test.java",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn scan_java_final_param() {
        let mut hits = Vec::new();
        (language::java::JAVA.scan_signatures)(
            "public void getUser(final String userId) {\n",
            "Test.java",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn violation_display() {
        let v = Violation {
            file: "test.py".into(),
            line: 5,
            param_name: "user_id".into(),
            raw_type: "str".into(),
            suggested: vec!["UserId".into()],
        };
        let s = format!("{v}");
        assert!(s.contains("error[B002]"));
        assert!(s.contains("--> test.py:5"));
        assert!(s.contains("user_id"));
        assert!(s.contains("help: use UserId instead of str"));
    }

    // --- False-positive regression tests ---

    fn make_id_type_map() -> HashMap<String, Vec<String>> {
        let mut m = HashMap::new();
        m.insert("str".to_string(), vec!["Id".to_string()]);
        m.insert("string".to_string(), vec!["Id".to_string()]);
        m.insert("String".to_string(), vec!["Id".to_string()]);
        m
    }

    #[test]
    fn no_false_positive_provider_vs_id() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def process(provider: str) -> None: ...\n",
            "test.py",
            &make_id_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty(), "provider should not match newtype Id");
    }

    #[test]
    fn no_false_positive_valid_vs_id() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def check(valid: str) -> None: ...\n",
            "test.py",
            &make_id_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty(), "valid should not match newtype Id");
    }

    #[test]
    fn no_false_positive_grid_vs_id() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def render(grid: str) -> None: ...\n",
            "test.py",
            &make_id_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(hits.is_empty(), "grid should not match newtype Id");
    }

    #[test]
    fn suffix_match_order_id_vs_id() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def process(order_id: str) -> None: ...\n",
            "test.py",
            &make_id_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1, "order_id should match newtype Id");
    }

    // --- Multi-line signature tests ---

    #[test]
    fn python_multiline_signature() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(\n    user_id: str,\n    name: str\n) -> None:\n    pass\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
        assert_eq!(hits[0].line, 1); // reported on the declaration line
    }

    #[test]
    fn rust_multiline_signature() {
        let mut hits = Vec::new();
        (language::rust_lang::RUST.scan_signatures)(
            "pub fn get_user(\n    user_id: String,\n    name: String,\n) -> () {\n}\n",
            "test.rs",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn ts_multiline_signature() {
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_signatures)(
            "function getUser(\n    userId: string,\n    name: string\n): void {\n}\n",
            "test.ts",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn go_multiline_signature() {
        let mut hits = Vec::new();
        (language::go::GO.scan_signatures)(
            "func GetUser(\n    userId string,\n    name string,\n) error {\n}\n",
            "test.go",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
        assert_eq!(hits[0].line, 1);
    }

    #[test]
    fn java_multiline_signature() {
        let mut hits = Vec::new();
        (language::java::JAVA.scan_signatures)(
            "public void getUser(\n    String userId,\n    String name\n) {\n}\n",
            "Test.java",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
        assert_eq!(hits[0].line, 1);
    }

    // --- JS/TS declaration filter tests ---

    #[test]
    fn ts_interface_method_detected() {
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_signatures)(
            "  getUser(userId: string): User;\n",
            "test.ts",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "userId");
    }

    #[test]
    fn ts_if_statement_not_detected() {
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_signatures)(
            "if (userId === \"abc\") { return; }\n",
            "test.ts",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert!(
            hits.is_empty(),
            "if-statement should not be scanned for signatures"
        );
    }

    // --- Inline basis:allow(B002) suppression tests ---

    #[test]
    fn inline_allow_above_line_suppresses() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "# basis:allow(B002)\ndef get_user(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1); // Scanner still finds it
                                   // Simulate the filtering that check_values does
        let content = "# basis:allow(B002)\ndef get_user(user_id: str) -> None: ...\n";
        let lines: Vec<&str> = content.lines().collect();
        hits.retain(|hit| {
            let line_idx = hit.line.saturating_sub(1);
            let current = lines.get(line_idx).unwrap_or(&"");
            let above = if line_idx > 0 {
                lines.get(line_idx - 1).unwrap_or(&"")
            } else {
                &""
            };
            !current.contains("basis:allow(B002)") && !above.contains("basis:allow(B002)")
        });
        assert!(hits.is_empty(), "inline allow above should suppress hit");
    }

    #[test]
    fn inline_allow_same_line_suppresses() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(user_id: str) -> None: ...  # basis:allow(B002)\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        let content = "def get_user(user_id: str) -> None: ...  # basis:allow(B002)\n";
        let lines: Vec<&str> = content.lines().collect();
        hits.retain(|hit| {
            let line_idx = hit.line.saturating_sub(1);
            let current = lines.get(line_idx).unwrap_or(&"");
            !current.contains("basis:allow(B002)")
        });
        assert!(
            hits.is_empty(),
            "inline allow on same line should suppress hit"
        );
    }

    #[test]
    fn no_inline_allow_keeps_hit() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1, "without allow comment, hit should remain");
    }

    // --- exclude_params tests ---

    #[test]
    fn exclude_params_filters_hit() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def process(user_id: str, index: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        // user_id matches UserId, index does not match UserId — only 1 hit
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].param_name, "user_id");

        // Now simulate exclude_params filtering
        let exclude: HashSet<&str> = ["user_id"].iter().copied().collect();
        hits.retain(|hit| !exclude.contains(hit.param_name.as_str()));
        assert!(hits.is_empty(), "excluded param should be filtered");
    }

    #[test]
    fn exclude_params_keeps_non_excluded() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        let exclude: HashSet<&str> = ["index"].iter().copied().collect();
        hits.retain(|hit| !exclude.contains(hit.param_name.as_str()));
        assert_eq!(hits.len(), 1, "non-excluded param should remain");
    }

    // --- exclude_functions tests ---

    #[test]
    fn exclude_functions_filters_hit() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def len(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].function_name, "len");

        let exclude: HashSet<&str> = ["len"].iter().copied().collect();
        hits.retain(|hit| {
            hit.function_name.is_empty() || !exclude.contains(hit.function_name.as_str())
        });
        assert!(hits.is_empty(), "excluded function should be filtered");
    }

    #[test]
    fn exclude_functions_keeps_non_excluded() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        let exclude: HashSet<&str> = ["len"].iter().copied().collect();
        hits.retain(|hit| {
            hit.function_name.is_empty() || !exclude.contains(hit.function_name.as_str())
        });
        assert_eq!(hits.len(), 1, "non-excluded function should remain");
    }

    // --- function_name extraction tests ---

    #[test]
    fn python_extracts_function_name() {
        let mut hits = Vec::new();
        (language::python::PYTHON.scan_signatures)(
            "def get_user(user_id: str) -> None: ...\n",
            "test.py",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "get_user");
    }

    #[test]
    fn rust_extracts_function_name() {
        let mut hits = Vec::new();
        (language::rust_lang::RUST.scan_signatures)(
            "pub fn get_user(user_id: String) -> () {}\n",
            "test.rs",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "get_user");
    }

    #[test]
    fn go_extracts_function_name() {
        let mut hits = Vec::new();
        (language::go::GO.scan_signatures)(
            "func GetUser(userId string) error {\n",
            "test.go",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "GetUser");
    }

    #[test]
    fn ts_extracts_function_name() {
        let mut hits = Vec::new();
        (language::javascript::JAVASCRIPT.scan_signatures)(
            "function getUser(userId: string): void {}\n",
            "test.ts",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "getUser");
    }

    #[test]
    fn java_extracts_function_name() {
        let mut hits = Vec::new();
        (language::java::JAVA.scan_signatures)(
            "public void getUser(String userId) {\n",
            "Test.java",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "getUser");
    }

    #[test]
    fn kotlin_extracts_function_name() {
        let mut hits = Vec::new();
        (language::kotlin::KOTLIN.scan_signatures)(
            "fun getUser(userId: String): User { }\n",
            "test.kt",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "getUser");
    }

    #[test]
    fn swift_extracts_function_name() {
        let mut hits = Vec::new();
        (language::swift::SWIFT.scan_signatures)(
            "func getUser(userId: String) -> User { }\n",
            "test.swift",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "getUser");
    }

    #[test]
    fn csharp_extracts_function_name() {
        let mut hits = Vec::new();
        (language::csharp::CSHARP.scan_signatures)(
            "public User GetUser(string userId) {\n}\n",
            "Test.cs",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "GetUser");
    }

    #[test]
    fn ruby_extracts_function_name() {
        let mut hits = Vec::new();
        (language::ruby::RUBY.scan_signatures)(
            "sig { params(user_id: String).returns(User) }\ndef get_user(user_id)\nend\n",
            "test.rb",
            &make_type_map(),
            &HashMap::new(),
            &mut hits,
        );
        assert_eq!(hits[0].function_name, "get_user");
    }

    // --- Spec deserialization tests ---

    #[test]
    fn spec_exclude_fields_deserialize() {
        let yaml = r#"
governance:
  version: "1.0"
newtypes:
  enabled: true
  exclude_params: [index, count, offset]
  exclude_functions: [len, to_string]
  types:
    - name: UserId
      wraps: string
"#;
        let spec: crate::spec::BasisSpec = serde_yaml::from_str(yaml).unwrap();
        let nt = spec.newtypes.unwrap();
        assert_eq!(nt.exclude_params, vec!["index", "count", "offset"]);
        assert_eq!(nt.exclude_functions, vec!["len", "to_string"]);
    }

    #[test]
    fn spec_exclude_fields_default_empty() {
        let yaml = r#"
governance:
  version: "1.0"
newtypes:
  enabled: true
  types:
    - name: UserId
      wraps: string
"#;
        let spec: crate::spec::BasisSpec = serde_yaml::from_str(yaml).unwrap();
        let nt = spec.newtypes.unwrap();
        assert!(nt.exclude_params.is_empty());
        assert!(nt.exclude_functions.is_empty());
    }
}
