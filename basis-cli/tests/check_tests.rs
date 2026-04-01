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
        layers: HashMap::new(),
        newtypes: Some(NewtypeConfig {
            enabled: true,
            types: vec![
                NewtypeDef {
                    name: "UserId".into(),
                    wraps: "string".into(),
                    validation: None,
                },
                NewtypeDef {
                    name: "OrderId".into(),
                    wraps: "string".into(),
                    validation: None,
                },
                NewtypeDef {
                    name: "PortNumber".into(),
                    wraps: "int".into(),
                    validation: None,
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
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: Some(PurityConfig {
            enabled: true,
            forbidden_in_strict: vec!["file_io".into(), "network_io".into(), "stdout".into()],
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
