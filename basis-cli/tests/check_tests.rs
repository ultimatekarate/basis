use std::collections::HashMap;
use std::io::Write;

use basis_cli::check::{completeness, purity, values};
use basis_cli::spec::*;

fn make_spec_with_newtypes() -> BasisSpec {
    BasisSpec {
        governance: Governance {
            version: "1.0".into(),
            model: None,
        },
        extends: None,
        layers: HashMap::new(),
        newtypes: Some(NewtypeConfig {
            enabled: true,
            types: vec![
                NewtypeDef {
                    name: "UserId".into(),
                    wraps: "string".into(),
                    validation: None,
                    languages: None,
                },
                NewtypeDef {
                    name: "OrderId".into(),
                    wraps: "string".into(),
                    validation: None,
                    languages: None,
                },
                NewtypeDef {
                    name: "PortNumber".into(),
                    wraps: "int".into(),
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
    }
}

fn make_spec_with_unions() -> BasisSpec {
    BasisSpec {
        governance: Governance {
            version: "1.0".into(),
            model: None,
        },
        extends: None,
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![UnionDef {
                name: "OrderStatus".into(),
                variants: vec![
                    "Pending".into(),
                    "Confirmed".into(),
                    "Shipped".into(),
                    "Delivered".into(),
                    "Cancelled".into(),
                ],
                languages: None,
            }],
        }),
        purity: None,
        boundaries: None,
    }
}

fn make_spec_with_purity() -> BasisSpec {
    let mut layers = HashMap::new();
    layers.insert(
        "laboratory".to_string(),
        Layer {
            role: "Pure logic".into(),
            packages: vec!["src/logic".into()],
            rules: {
                let mut r = HashMap::new();
                r.insert("purity".into(), "strict".into());
                r
            },
            depends_on: vec![],
        },
    );
    layers.insert(
        "hands".to_string(),
        Layer {
            role: "IO".into(),
            packages: vec!["src/api".into()],
            rules: HashMap::new(),
            depends_on: vec!["laboratory".into()],
        },
    );

    BasisSpec {
        governance: Governance {
            version: "1.0".into(),
            model: None,
        },
        extends: None,
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: Some(PurityConfig {
            enabled: true,
            forbidden_in_strict: vec!["file_io".into(), "network_io".into(), "stdout".into()],
            per_layer: std::collections::HashMap::new(),
        }),
        boundaries: None,
    }
}

fn write_file(dir: &std::path::Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
}

// ── Values tests ──────────────────────────────────────────────────────

#[test]
fn values_detects_raw_str_param() {
    let spec = make_spec_with_newtypes();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "example.py",
        "def get_user(user_id: str) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(!violations.is_empty(), "Should detect raw str for user_id");
    assert!(violations.iter().any(|v| v.param_name == "user_id"));
    assert!(violations
        .iter()
        .any(|v| v.suggested.contains(&"UserId".to_string())));
}

#[test]
fn values_ignores_private_functions() {
    let spec = make_spec_with_newtypes();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "example.py",
        "def _helper(user_id: str) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(violations.is_empty(), "Private functions should be skipped");
}

#[test]
fn values_no_violation_for_branded_type() {
    let spec = make_spec_with_newtypes();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "example.py",
        "def get_user(user_id: UserId) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(
        violations.is_empty(),
        "Branded type should not trigger violation"
    );
}

#[test]
fn values_no_match_for_unrelated_param() {
    let spec = make_spec_with_newtypes();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "example.py",
        "def process(data: str) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(
        violations.is_empty(),
        "Unrelated param name should not match"
    );
}

#[test]
fn values_disabled_returns_empty() {
    let mut spec = make_spec_with_newtypes();
    spec.newtypes.as_mut().unwrap().enabled = false;
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "example.py",
        "def get_user(user_id: str) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(violations.is_empty());
}

// ── Completeness tests ──────────────────────────────────────────────

#[test]
fn completeness_detects_missing_variants_match() {
    let spec = make_spec_with_unions();
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "handler.py",
        "def handle(status):\n    match status:\n        case \"Pending\":\n            pass\n        case \"Confirmed\":\n            pass\n");

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert!(!violations.is_empty(), "Should detect missing variants");
    let v = &violations[0];
    assert_eq!(v.union_name, "OrderStatus");
    assert!(v.missing_variants.contains(&"Shipped".to_string()));
    assert!(v.missing_variants.contains(&"Delivered".to_string()));
    assert!(v.missing_variants.contains(&"Cancelled".to_string()));
}

#[test]
fn completeness_wildcard_is_exhaustive() {
    let spec = make_spec_with_unions();
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "handler.py",
        "def handle(status):\n    match status:\n        case \"Pending\":\n            pass\n        case _:\n            pass\n");

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert!(
        violations.is_empty(),
        "Wildcard should be considered exhaustive"
    );
}

#[test]
fn completeness_all_variants_no_violation() {
    let spec = make_spec_with_unions();
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "handler.py",
        "def handle(status):\n    match status:\n        case \"Pending\":\n            pass\n        case \"Confirmed\":\n            pass\n        case \"Shipped\":\n            pass\n        case \"Delivered\":\n            pass\n        case \"Cancelled\":\n            pass\n");

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert!(violations.is_empty(), "All variants covered — no violation");
}

#[test]
fn completeness_elif_with_else_is_exhaustive() {
    let spec = make_spec_with_unions();
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "handler.py",
        "if status == \"Pending\":\n    pass\nelif status == \"Confirmed\":\n    pass\nelse:\n    pass\n");

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert!(
        violations.is_empty(),
        "else clause should be considered exhaustive"
    );
}

#[test]
fn completeness_disabled_returns_empty() {
    let mut spec = make_spec_with_unions();
    spec.exhaustive_matching.as_mut().unwrap().enabled = false;
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "handler.py",
        "def handle(status):\n    match status:\n        case \"Pending\":\n            pass\n",
    );

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert!(violations.is_empty());
}

// ── Purity tests ──────────────────────────────────────────────────────

#[test]
fn purity_detects_forbidden_import_in_strict_layer() {
    let spec = make_spec_with_purity();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "src/logic/compute.py",
        "import requests\n\ndef compute(): pass\n",
    );

    let violations =
        purity::check_purity(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(
        !violations.is_empty(),
        "Should detect forbidden import in strict layer"
    );
    assert!(violations.iter().any(|v| v.forbidden.contains("requests")));
    assert!(violations.iter().any(|v| v.layer == "laboratory"));
}

#[test]
fn purity_allows_imports_in_non_strict_layer() {
    let spec = make_spec_with_purity();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "src/api/server.py",
        "import requests\n\ndef serve(): pass\n",
    );

    let violations =
        purity::check_purity(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(
        violations.is_empty(),
        "Non-strict layer should allow IO imports"
    );
}

#[test]
fn purity_clean_strict_layer() {
    let spec = make_spec_with_purity();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "src/logic/pure.py",
        "def add(a, b):\n    return a + b\n",
    );

    let violations =
        purity::check_purity(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(violations.is_empty(), "Pure code should have no violations");
}

#[test]
fn purity_disabled_returns_empty() {
    let mut spec = make_spec_with_purity();
    spec.purity.as_mut().unwrap().enabled = false;
    let dir = tempfile::tempdir().unwrap();
    write_file(dir.path(), "src/logic/compute.py", "import requests\n");

    let violations =
        purity::check_purity(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(violations.is_empty());
}

// ── Polyglot language scoping tests ──────────────────────────────────
//
// These tests simulate a Python/TypeScript polyglot repo (like the
// Electron example) and verify that language-scoped newtypes and unions
// only produce violations in files of the matching language.

fn make_polyglot_spec() -> BasisSpec {
    BasisSpec {
        governance: Governance {
            version: "1.0".into(),
            model: None,
        },
        extends: None,
        layers: HashMap::new(),
        newtypes: Some(NewtypeConfig {
            enabled: true,
            types: vec![
                // Shared across all languages
                NewtypeDef {
                    name: "UserId".into(),
                    wraps: "string".into(),
                    validation: None,
                    languages: None,
                },
                // TypeScript only
                NewtypeDef {
                    name: "WindowId".into(),
                    wraps: "int".into(),
                    validation: None,
                    languages: Some(vec!["js".into()]),
                },
                // Python only
                NewtypeDef {
                    name: "ApiKey".into(),
                    wraps: "string".into(),
                    validation: None,
                    languages: Some(vec!["python".into()]),
                },
            ],
            exclude_params: vec![],
            exclude_functions: vec![],
        }),
        exhaustive_matching: Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![
                // Shared across all languages
                UnionDef {
                    name: "DocumentState".into(),
                    variants: vec![
                        "Draft".into(),
                        "Review".into(),
                        "Published".into(),
                        "Archived".into(),
                    ],
                    languages: None,
                },
                // Python only
                UnionDef {
                    name: "TaskStatus".into(),
                    variants: vec![
                        "Queued".into(),
                        "Running".into(),
                        "Completed".into(),
                        "Failed".into(),
                        "Cancelled".into(),
                    ],
                    languages: Some(vec!["python".into()]),
                },
            ],
        }),
        purity: None,
        boundaries: None,
    }
}

// ── Values: language scoping ─────────────────────────────────────────

#[test]
fn polyglot_shared_newtype_flags_python() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "backend/handler.py",
        "def get_user(user_id: str) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].param_name, "user_id");
    assert!(violations[0].suggested.contains(&"UserId".to_string()));
}

#[test]
fn polyglot_shared_newtype_flags_typescript() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    write_file(
        dir.path(),
        "frontend/handler.ts",
        "function getUser(userId: string): void {}\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].param_name, "userId");
    assert!(violations[0].suggested.contains(&"UserId".to_string()));
}

#[test]
fn polyglot_js_only_newtype_does_not_flag_python() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // WindowId is scoped to [js] — a Python function with window_id: int should NOT be flagged
    write_file(
        dir.path(),
        "backend/window.py",
        "def get_window(window_id: int) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(
        violations.is_empty(),
        "JS-only WindowId should not flag Python files, got: {:?}",
        violations.iter().map(|v| &v.param_name).collect::<Vec<_>>()
    );
}

#[test]
fn polyglot_js_only_newtype_flags_typescript() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // WindowId is scoped to [js] — a TS function with windowId: number should be flagged
    write_file(
        dir.path(),
        "frontend/window.ts",
        "function getWindow(windowId: number): void {}\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].param_name, "windowId");
    assert!(violations[0].suggested.contains(&"WindowId".to_string()));
}

#[test]
fn polyglot_python_only_newtype_does_not_flag_typescript() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // ApiKey is scoped to [python] — a TS function with apiKey: string should NOT be flagged
    write_file(
        dir.path(),
        "frontend/auth.ts",
        "function authenticate(apiKey: string): void {}\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert!(
        violations.is_empty(),
        "Python-only ApiKey should not flag TypeScript files, got: {:?}",
        violations.iter().map(|v| &v.param_name).collect::<Vec<_>>()
    );
}

#[test]
fn polyglot_python_only_newtype_flags_python() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // ApiKey is scoped to [python] — a Python function with api_key: str should be flagged
    write_file(
        dir.path(),
        "backend/auth.py",
        "def authenticate(api_key: str) -> None: ...\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].param_name, "api_key");
    assert!(violations[0].suggested.contains(&"ApiKey".to_string()));
}

#[test]
fn polyglot_mixed_repo_only_flags_correct_languages() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();

    // Python file: should flag user_id (shared) and api_key (python-only), NOT window_id (js-only)
    write_file(
        dir.path(),
        "backend/service.py",
        "def handle(user_id: str, api_key: str, window_id: int) -> None: ...\n",
    );

    // TypeScript file: should flag userId (shared) and windowId (js-only), NOT apiKey (python-only)
    write_file(
        dir.path(),
        "frontend/service.ts",
        "function handle(userId: string, windowId: number, apiKey: string): void {}\n",
    );

    let violations =
        values::check_values(&spec, dir.path(), &basis_cli::language::LangRegistry::new());

    let py_violations: Vec<_> = violations.iter().filter(|v| v.file.contains("backend")).collect();
    let ts_violations: Vec<_> = violations.iter().filter(|v| v.file.contains("frontend")).collect();

    // Python: user_id and api_key flagged, window_id not
    assert_eq!(
        py_violations.len(),
        2,
        "Python should have 2 violations (user_id, api_key), got: {:?}",
        py_violations.iter().map(|v| &v.param_name).collect::<Vec<_>>()
    );
    assert!(py_violations.iter().any(|v| v.param_name == "user_id"));
    assert!(py_violations.iter().any(|v| v.param_name == "api_key"));
    assert!(!py_violations.iter().any(|v| v.param_name == "window_id"));

    // TypeScript: userId and windowId flagged, apiKey not
    assert_eq!(
        ts_violations.len(),
        2,
        "TypeScript should have 2 violations (userId, windowId), got: {:?}",
        ts_violations.iter().map(|v| &v.param_name).collect::<Vec<_>>()
    );
    assert!(ts_violations.iter().any(|v| v.param_name == "userId"));
    assert!(ts_violations.iter().any(|v| v.param_name == "windowId"));
    assert!(!ts_violations.iter().any(|v| v.param_name == "apiKey"));
}

// ── Completeness: language scoping ───────────────────────────────────

#[test]
fn polyglot_shared_union_flags_python() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // DocumentState is shared — incomplete match in Python should be flagged
    write_file(
        dir.path(),
        "backend/handler.py",
        "def handle(state):\n    match state:\n        case \"Draft\":\n            pass\n        case \"Review\":\n            pass\n",
    );

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].union_name, "DocumentState");
    assert!(violations[0].missing_variants.contains(&"Published".to_string()));
    assert!(violations[0].missing_variants.contains(&"Archived".to_string()));
}

#[test]
fn polyglot_shared_union_flags_typescript() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // DocumentState is shared — incomplete switch in TS should be flagged
    write_file(
        dir.path(),
        "frontend/handler.ts",
        "switch (state) {\n    case \"Draft\":\n        break;\n    case \"Review\":\n        break;\n}\n",
    );

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].union_name, "DocumentState");
}

#[test]
fn polyglot_python_only_union_does_not_flag_typescript() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // TaskStatus is scoped to [python] — incomplete switch in TS should NOT be flagged
    write_file(
        dir.path(),
        "frontend/tasks.ts",
        "switch (status) {\n    case \"Queued\":\n        break;\n    case \"Running\":\n        break;\n}\n",
    );

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert!(
        violations.is_empty(),
        "Python-only TaskStatus should not flag TypeScript files, got: {:?}",
        violations.iter().map(|v| &v.union_name).collect::<Vec<_>>()
    );
}

#[test]
fn polyglot_python_only_union_flags_python() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();
    // TaskStatus is scoped to [python] — incomplete match in Python should be flagged
    write_file(
        dir.path(),
        "backend/tasks.py",
        "def handle(status):\n    match status:\n        case \"Queued\":\n            pass\n        case \"Running\":\n            pass\n",
    );

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].union_name, "TaskStatus");
    assert!(violations[0].missing_variants.contains(&"Completed".to_string()));
    assert!(violations[0].missing_variants.contains(&"Failed".to_string()));
    assert!(violations[0].missing_variants.contains(&"Cancelled".to_string()));
}

#[test]
fn polyglot_mixed_repo_completeness() {
    let spec = make_polyglot_spec();
    let dir = tempfile::tempdir().unwrap();

    // Python: incomplete matches on both DocumentState (shared) and TaskStatus (python-only)
    write_file(
        dir.path(),
        "backend/handler.py",
        concat!(
            "def handle_doc(state):\n",
            "    match state:\n",
            "        case \"Draft\":\n",
            "            pass\n",
            "\n",
            "def handle_task(status):\n",
            "    match status:\n",
            "        case \"Queued\":\n",
            "            pass\n",
        ),
    );

    // TypeScript: incomplete switch on DocumentState (shared), and
    // incomplete switch on TaskStatus variants (should NOT be flagged — python-only)
    write_file(
        dir.path(),
        "frontend/handler.ts",
        concat!(
            "switch (state) {\n",
            "    case \"Draft\":\n",
            "        break;\n",
            "}\n",
            "\n",
            "switch (taskStatus) {\n",
            "    case \"Queued\":\n",
            "        break;\n",
            "}\n",
        ),
    );

    let violations = completeness::check_completeness(
        &spec,
        dir.path(),
        &basis_cli::language::LangRegistry::new(),
    );

    let py_violations: Vec<_> = violations.iter().filter(|v| v.file.contains("backend")).collect();
    let ts_violations: Vec<_> = violations.iter().filter(|v| v.file.contains("frontend")).collect();

    // Python: both DocumentState and TaskStatus flagged
    assert_eq!(
        py_violations.len(),
        2,
        "Python should have 2 completeness violations, got: {:?}",
        py_violations.iter().map(|v| &v.union_name).collect::<Vec<_>>()
    );
    assert!(py_violations.iter().any(|v| v.union_name == "DocumentState"));
    assert!(py_violations.iter().any(|v| v.union_name == "TaskStatus"));

    // TypeScript: only DocumentState flagged, NOT TaskStatus
    assert_eq!(
        ts_violations.len(),
        1,
        "TypeScript should have 1 completeness violation (DocumentState only), got: {:?}",
        ts_violations.iter().map(|v| &v.union_name).collect::<Vec<_>>()
    );
    assert_eq!(ts_violations[0].union_name, "DocumentState");
}
