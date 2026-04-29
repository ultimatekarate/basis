use std::collections::HashMap;
use std::io::Write;

use basis_cli::spec::*;
use basis_cli::watch;

fn make_empty_spec() -> BasisSpec {
    BasisSpec {
        governance: Governance::new("1.0"),
        extends: None,
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
        granularity: None,
    }
}

fn make_spec_with_granularity(max_lines: usize) -> BasisSpec {
    let mut layers = HashMap::new();
    layers.insert(
        "app".to_string(),
        Layer {
            role: "application code".into(),
            packages: vec!["src".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: false,
        },
    );
    BasisSpec {
        governance: Governance::new("1.0"),
        extends: None,
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
        granularity: Some(GranularityConfig {
            enabled: true,
            max_lines,
            per_layer: HashMap::new(),
        }),
    }
}

#[test]
fn timed_check_empty_spec_no_violations() {
    let dir = tempfile::tempdir().unwrap();
    let spec = make_empty_spec();
    let outcome = watch::timed_check(&spec, dir.path(), &None);
    assert!(outcome.violations.is_empty());
    assert!(outcome.identities.is_empty());
    assert!(outcome.elapsed.as_secs() < 5);
}

#[test]
fn timed_check_detects_granularity_violation() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();

    // Create a file that exceeds the max_lines budget
    let mut f = std::fs::File::create(src_dir.join("big.rs")).unwrap();
    for i in 0..20 {
        writeln!(f, "// line {i}").unwrap();
    }
    drop(f);

    let spec = make_spec_with_granularity(10);
    let outcome = watch::timed_check(&spec, dir.path(), &None);

    assert!(!outcome.violations.is_empty(), "expected granularity violation");
    assert!(outcome.violations.iter().any(|v| v.code == "B005"));
    assert!(!outcome.identities.is_empty());
}

#[test]
fn timed_check_respects_axes_filter() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();

    let mut f = std::fs::File::create(src_dir.join("big.rs")).unwrap();
    for i in 0..20 {
        writeln!(f, "// line {i}").unwrap();
    }
    drop(f);

    let spec = make_spec_with_granularity(10);

    // Only check placement — should not see granularity violations
    let axes = Some(vec!["placement".to_string()]);
    let outcome = watch::timed_check(&spec, dir.path(), &axes);
    assert!(
        outcome.violations.iter().all(|v| v.code != "B005"),
        "granularity violations should be filtered out"
    );
}

#[test]
fn identity_set_changes_when_violations_change() {
    let dir = tempfile::tempdir().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir(&src_dir).unwrap();

    let spec = make_spec_with_granularity(10);

    // No files yet — no violations
    let outcome1 = watch::timed_check(&spec, dir.path(), &None);
    assert!(outcome1.identities.is_empty());

    // Add a big file — violation appears
    let mut f = std::fs::File::create(src_dir.join("big.rs")).unwrap();
    for i in 0..20 {
        writeln!(f, "// line {i}").unwrap();
    }
    drop(f);

    let outcome2 = watch::timed_check(&spec, dir.path(), &None);
    assert_ne!(outcome1.identities, outcome2.identities);

    // Shrink the file — violation goes away
    std::fs::write(src_dir.join("big.rs"), "// small\n").unwrap();
    let outcome3 = watch::timed_check(&spec, dir.path(), &None);
    assert!(outcome3.identities.is_empty());
    assert_ne!(outcome2.identities, outcome3.identities);
}

#[test]
fn unified_violation_display_format() {
    use basis_cli::check::output::{UnifiedViolation, ViolationDetails};

    let v = UnifiedViolation {
        code: "B005".into(),
        axis: "granularity".into(),
        file: "src/big.rs".into(),
        line: 1,
        message: "file exceeds 10-line budget (20 lines)".into(),
        identity: "B005::src/big.rs".into(),
        details: ViolationDetails::Granularity {
            actual_lines: 20,
            max_lines: 10,
            layer: "app".into(),
        },
        help: "split this file into smaller modules".into(),
    };

    let text = format!("{v}");
    assert!(text.contains("error[B005]"));
    assert!(text.contains("src/big.rs:1"));
    assert!(text.contains("help: split this file"));
}
