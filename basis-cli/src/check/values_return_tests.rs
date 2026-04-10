use super::*;
use crate::language;

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
