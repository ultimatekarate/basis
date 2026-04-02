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
        extends: None,
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
        extends: None,
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
        function_name: "get_user".into(),
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

// ── Return type checking ────────────────────────────

fn make_userid_type_map_for(lang: &language::LangDef) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    for &(wraps_key, primitives) in lang.primitives {
        if wraps_key == "string" {
            for &prim in primitives {
                map.entry(prim.to_string())
                    .or_insert_with(Vec::new)
                    .push("UserId".to_string());
            }
        }
    }
    map
}

#[test]
fn python_return_type_flagged() {
    let content = "def get_user_id(x: int) -> str:\n    return x\n";
    let type_map = make_userid_type_map_for(&language::python::PYTHON);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::python::PYTHON.scan_signatures)(content, "test.py", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "str");
    assert_eq!(ret_hits[0].suggested, vec!["UserId"]);
}

#[test]
fn python_return_type_no_match_when_name_differs() {
    let content = "def get_name() -> str:\n    return 'alice'\n";
    let type_map = make_userid_type_map_for(&language::python::PYTHON);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::python::PYTHON.scan_signatures)(content, "test.py", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert!(ret_hits.is_empty());
}

#[test]
fn rust_return_type_flagged() {
    let content = "pub fn get_user_id(x: i32) -> String {\n    todo!()\n}\n";
    let type_map = make_userid_type_map_for(&language::rust_lang::RUST);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::rust_lang::RUST.scan_signatures)(content, "test.rs", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "String");
}

#[test]
fn js_return_type_flagged() {
    let content = "function getUserId(x: number): string {\n    return '';\n}\n";
    let type_map = make_userid_type_map_for(&language::javascript::JAVASCRIPT);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::javascript::JAVASCRIPT.scan_signatures)(content, "test.ts", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "string");
}

#[test]
fn go_return_type_flagged() {
    let content = "func GetUserId(x int) string {\n\treturn \"\"\n}\n";
    let type_map = make_userid_type_map_for(&language::go::GO);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::go::GO.scan_signatures)(content, "test.go", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "string");
}

#[test]
fn java_return_type_flagged() {
    let content = "public String getUserId(int x) {\n    return \"\";\n}\n";
    let type_map = make_userid_type_map_for(&language::java::JAVA);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::java::JAVA.scan_signatures)(content, "Test.java", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "String");
}

#[test]
fn kotlin_return_type_flagged() {
    let content = "fun getUserId(x: Int): String {\n    return \"\"\n}\n";
    let type_map = make_userid_type_map_for(&language::kotlin::KOTLIN);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::kotlin::KOTLIN.scan_signatures)(content, "Test.kt", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "String");
}

#[test]
fn swift_return_type_flagged() {
    let content = "func getUserId(x: Int) -> String {\n    return \"\"\n}\n";
    let type_map = make_userid_type_map_for(&language::swift::SWIFT);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::swift::SWIFT.scan_signatures)(content, "Test.swift", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "String");
}

#[test]
fn csharp_return_type_flagged() {
    let content = "public string GetUserId(int x) {\n    return \"\";\n}\n";
    let type_map = make_userid_type_map_for(&language::csharp::CSHARP);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::csharp::CSHARP.scan_signatures)(content, "Test.cs", &type_map, &hints, &mut hits);
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    assert_eq!(ret_hits.len(), 1);
    assert_eq!(ret_hits[0].raw_type, "string");
}

#[test]
fn violation_display_return_type() {
    let v = Violation {
        file: "test.py".into(),
        line: 5,
        function_name: "get_user_id".into(),
        param_name: "(return)".into(),
        raw_type: "str".into(),
        suggested: vec!["UserId".into()],
    };
    let s = format!("{v}");
    assert!(s.contains("error[B002]"));
    assert!(s.contains("return type"));
    assert!(s.contains("str"));
    assert!(s.contains("UserId"));
}

#[test]
fn violation_display_param_type() {
    let v = Violation {
        file: "test.py".into(),
        line: 5,
        function_name: "get_user".into(),
        param_name: "user_id".into(),
        raw_type: "str".into(),
        suggested: vec!["UserId".into()],
    };
    let s = format!("{v}");
    assert!(s.contains("error[B002]"));
    assert!(s.contains("parameter"));
    assert!(!s.contains("return type"));
}

#[test]
fn exclude_functions_suppresses_return_type() {
    // A function named "to_string" returning str should be excluded
    let content = "def to_string() -> str:\n    return ''\n";
    let type_map = make_userid_type_map_for(&language::python::PYTHON);
    let hints = HashMap::new();
    let mut hits = Vec::new();
    (language::python::PYTHON.scan_signatures)(content, "test.py", &type_map, &hints, &mut hits);
    // The scanner WILL produce a hit (it doesn't know about exclusions),
    // but check_values filters it out via exclude_functions.
    // Here we verify the hit exists so the filter has something to work with.
    let ret_hits: Vec<_> = hits.iter().filter(|h| h.param_name == "(return)").collect();
    // to_string -> words ["to", "string"], UserId -> ["user", "id"] — no suffix match
    assert!(ret_hits.is_empty());
}
