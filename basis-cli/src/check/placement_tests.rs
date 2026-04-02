use super::*;
use crate::language;
use std::collections::HashMap;

// ── import extraction + normalization ──────────────────

fn norm(module: &str, lang: &LangDef) -> String {
    normalize_module_path(module, lang)
}

#[test]
fn go_import_single() {
    let imports = (language::go::extract_imports)("import \"fmt\"\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(norm(&imports[0].module, &language::go::GO), "fmt");
}

#[test]
fn go_import_block() {
    let imports = (language::go::extract_imports)("import (\n\t\"fmt\"\n\t\"net/http\"\n)\n");
    assert_eq!(imports.len(), 2);
    assert_eq!(norm(&imports[0].module, &language::go::GO), "fmt");
    assert_eq!(norm(&imports[1].module, &language::go::GO), "net/http");
}

#[test]
fn go_import_aliased() {
    let imports =
        (language::go::extract_imports)("import (\n\tmux \"github.com/gorilla/mux\"\n)\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::go::GO),
        "github.com/gorilla/mux"
    );
}

#[test]
fn java_import_simple() {
    let imports = (language::java::extract_imports)("import com.example.models.User;\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::java::JAVA),
        "com/example/models/User"
    );
}

#[test]
fn java_import_static() {
    let imports =
        (language::java::extract_imports)("import static org.junit.Assert.assertEquals;\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::java::JAVA),
        "org/junit/Assert/assertEquals"
    );
}

#[test]
fn java_import_wildcard() {
    let imports = (language::java::extract_imports)("import java.util.*;\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::java::JAVA),
        "java/util/*"
    );
}

#[test]
fn python_from_import() {
    let imports = (language::python::extract_imports)("from os import path\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(norm(&imports[0].module, &language::python::PYTHON), "os");
    assert_eq!(imports[0].line, 1);
}

#[test]
fn python_import_single() {
    let imports = (language::python::extract_imports)("import json\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(norm(&imports[0].module, &language::python::PYTHON), "json");
}

#[test]
fn python_import_multiple() {
    let imports = (language::python::extract_imports)("import json, sys, os\n");
    assert_eq!(imports.len(), 3);
    assert_eq!(norm(&imports[0].module, &language::python::PYTHON), "json");
    assert_eq!(norm(&imports[1].module, &language::python::PYTHON), "sys");
    assert_eq!(norm(&imports[2].module, &language::python::PYTHON), "os");
}

#[test]
fn python_dotted_import() {
    let imports = (language::python::extract_imports)("from src.models import types\n");
    assert_eq!(
        norm(&imports[0].module, &language::python::PYTHON),
        "src/models"
    );
}

#[test]
fn python_skips_relative_dot() {
    let imports = (language::python::extract_imports)("from . import sibling\n");
    assert!(imports.is_empty());
}

#[test]
fn python_skips_non_import_lines() {
    let imports = (language::python::extract_imports)("x = 1\nprint('hello')\n# import os\n");
    assert!(imports.is_empty());
}

#[test]
fn rust_use_statement() {
    let imports = (language::rust_lang::extract_imports)("use std::collections::HashMap;\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::rust_lang::RUST),
        "std/collections/HashMap"
    );
}

#[test]
fn rust_use_with_braces() {
    let imports =
        (language::rust_lang::extract_imports)("use std::collections::{HashMap, HashSet};\n");
    assert_eq!(imports.len(), 1);
    assert!(norm(&imports[0].module, &language::rust_lang::RUST).starts_with("std/collections"));
}

#[test]
fn rust_mod_statement() {
    let imports = (language::rust_lang::extract_imports)("mod check;\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::rust_lang::RUST),
        "check"
    );
}

#[test]
fn rust_skips_inline_mod() {
    let imports = (language::rust_lang::extract_imports)("mod check {\n    use foo;\n}\n");
    let mod_imports: Vec<_> = imports.iter().filter(|i| i.module == "check").collect();
    assert!(mod_imports.is_empty());
}

#[test]
fn js_import_from() {
    let imports =
        (language::javascript::extract_imports)("import { Foo } from \"./models\";\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::javascript::JAVASCRIPT),
        "./models"
    );
}

#[test]
fn js_require() {
    let imports = (language::javascript::extract_imports)("const fs = require('fs');\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::javascript::JAVASCRIPT),
        "fs"
    );
}

#[test]
fn js_import_no_from() {
    let imports = (language::javascript::extract_imports)("import \"side-effect-module\";\n");
    assert_eq!(imports.len(), 1);
    assert_eq!(
        norm(&imports[0].module, &language::javascript::JAVASCRIPT),
        "side-effect-module"
    );
}

// ── normalization ─────────────────────────────────────

#[test]
fn normalize_dots_to_slashes() {
    assert_eq!(
        normalize_module_path("com.example.Foo", &language::java::JAVA),
        "com/example/Foo"
    );
}

#[test]
fn normalize_double_colon_to_slashes() {
    assert_eq!(
        normalize_module_path("std::fs::File", &language::rust_lang::RUST),
        "std/fs/File"
    );
}

#[test]
fn normalize_already_slashes() {
    assert_eq!(
        normalize_module_path("net/http", &language::go::GO),
        "net/http"
    );
}

// ── layer resolution ───────────────────────────────────

#[test]
fn resolve_layer_exact_prefix() {
    let map = vec![
        ("src/models".into(), "dictionary".into()),
        ("src/logic".into(), "laboratory".into()),
    ];
    assert_eq!(
        resolve_layer("src/models/types.py", &map),
        Some("dictionary".into())
    );
    assert_eq!(
        resolve_layer("src/logic/validate.py", &map),
        Some("laboratory".into())
    );
}

#[test]
fn resolve_layer_no_match() {
    let map = vec![("src/models".into(), "dictionary".into())];
    assert_eq!(resolve_layer("tests/test_foo.py", &map), None);
}

#[test]
fn resolve_layer_longest_prefix_wins() {
    let map = build_layer_map(&{
        let mut spec = BasisSpec {
        extends: None,
            governance: crate::spec::Governance {
                version: "1.0".into(),
                model: None,
            },
            layers: HashMap::new(),
            newtypes: None,
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        };
        spec.layers.insert(
            "broad".into(),
            crate::spec::Layer {
                role: "".into(),
                packages: vec!["src".into()],
                rules: HashMap::new(),
                depends_on: vec![],
                external: false,
            },
        );
        spec.layers.insert(
            "specific".into(),
            crate::spec::Layer {
                role: "".into(),
                packages: vec!["src/models".into()],
                rules: HashMap::new(),
                depends_on: vec![],
                external: false,
            },
        );
        spec
    });
    assert_eq!(
        resolve_layer("src/models/types.py", &map),
        Some("specific".into())
    );
    assert_eq!(
        resolve_layer("src/other/file.py", &map),
        Some("broad".into())
    );
}

// ── boundary checking ──────────────────────────────────

#[test]
fn no_deps_means_denied() {
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };
    assert!(!is_allowed("a", "b", &spec));
}

#[test]
fn explicit_allow_rule() {
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: Some(crate::spec::BoundaryConfig {
            enabled: true,
            rules: vec![crate::spec::BoundaryRule {
                from: "hands".into(),
                to: "dictionary".into(),
                action: "allow".into(),
                reason: None,
            }],
            external: HashMap::new(),
        }),
    };
    assert!(is_allowed("hands", "dictionary", &spec));
}

#[test]
fn explicit_deny_rule() {
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: Some(crate::spec::BoundaryConfig {
            enabled: true,
            rules: vec![crate::spec::BoundaryRule {
                from: "dictionary".into(),
                to: "hands".into(),
                action: "deny".into(),
                reason: Some("no".into()),
            }],
            external: HashMap::new(),
        }),
    };
    assert!(!is_allowed("dictionary", "hands", &spec));
}

#[test]
fn depends_on_fallback() {
    let mut layers = HashMap::new();
    layers.insert(
        "a".to_string(),
        crate::spec::Layer {
            role: "".into(),
            packages: vec![],
            rules: HashMap::new(),
            depends_on: vec![],
            external: false,
        },
    );
    layers.insert(
        "b".to_string(),
        crate::spec::Layer {
            role: "".into(),
            packages: vec![],
            rules: HashMap::new(),
            depends_on: vec!["a".into()],
            external: false,
        },
    );
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };
    assert!(is_allowed("b", "a", &spec));
    assert!(!is_allowed("a", "b", &spec));
}

#[test]
fn transitive_deps_computed() {
    let mut layers = HashMap::new();
    layers.insert(
        "a".to_string(),
        crate::spec::Layer {
            role: "".into(),
            packages: vec![],
            rules: HashMap::new(),
            depends_on: vec![],
            external: false,
        },
    );
    layers.insert(
        "b".to_string(),
        crate::spec::Layer {
            role: "".into(),
            packages: vec![],
            rules: HashMap::new(),
            depends_on: vec!["a".into()],
            external: false,
        },
    );
    layers.insert(
        "c".to_string(),
        crate::spec::Layer {
            role: "".into(),
            packages: vec![],
            rules: HashMap::new(),
            depends_on: vec!["b".into()],
            external: false,
        },
    );
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };
    // c -> b -> a: c can reach a transitively
    assert!(is_allowed("c", "a", &spec));
    assert!(is_allowed("c", "b", &spec));
    // a cannot reach c
    assert!(!is_allowed("a", "c", &spec));
}

#[test]
fn get_deny_reason_from_rule() {
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: Some(crate::spec::BoundaryConfig {
            enabled: true,
            rules: vec![crate::spec::BoundaryRule {
                from: "a".into(),
                to: "b".into(),
                action: "deny".into(),
                reason: Some("not allowed".into()),
            }],
            external: HashMap::new(),
        }),
    };
    assert_eq!(get_deny_reason(&spec, "a", "b"), "not allowed");
}

#[test]
fn get_deny_reason_default() {
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers: HashMap::new(),
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };
    let reason = get_deny_reason(&spec, "x", "y");
    assert!(reason.contains("x"));
    assert!(reason.contains("y"));
}

#[test]
fn violation_display_format() {
    let v = Violation {
        file: "src/models/types.py".into(),
        line: 3,
        import: "src/logic".into(),
        from_layer: "dictionary".into(),
        to_layer: "laboratory".into(),
        reason: "not allowed".into(),
    };
    let s = format!("{v}");
    assert!(s.contains("error[B001]"));
    assert!(s.contains("--> src/models/types.py:3"));
    assert!(s.contains("src/logic"));
    assert!(s.contains("dictionary"));
    assert!(s.contains("help:"));
}

// ── deny_pattern_matches ──────────────────────────────

#[test]
fn deny_pattern_no_wildcard_substring() {
    assert!(deny_pattern_matches("tokio::runtime", "tokio"));
    assert!(deny_pattern_matches("async-std", "async"));
    assert!(!deny_pattern_matches("requests", "tokio"));
}

#[test]
fn deny_pattern_trailing_wildcard() {
    assert!(deny_pattern_matches("async-std", "async-*"));
    assert!(deny_pattern_matches("async-trait", "async-*"));
    assert!(deny_pattern_matches("async-", "async-*"));
    assert!(!deny_pattern_matches("sync-std", "async-*"));
}

#[test]
fn deny_pattern_leading_wildcard() {
    assert!(deny_pattern_matches("node:fs", "*:fs"));
    assert!(deny_pattern_matches("something:fs", "*:fs"));
    assert!(!deny_pattern_matches("node:http", "*:fs"));
}

#[test]
fn deny_pattern_prefix_wildcard() {
    assert!(deny_pattern_matches("node:fs", "node:*"));
    assert!(deny_pattern_matches("node:child_process", "node:*"));
    assert!(!deny_pattern_matches("deno:fs", "node:*"));
}

#[test]
fn deny_pattern_middle_wildcard() {
    assert!(deny_pattern_matches("std::io::File", "std::*::File"));
    assert!(deny_pattern_matches("std::fs::File", "std::*::File"));
    assert!(!deny_pattern_matches("std::io::Read", "std::*::File"));
}

#[test]
fn deny_pattern_lone_wildcard() {
    // "*" matches everything
    assert!(deny_pattern_matches("anything", "*"));
    assert!(deny_pattern_matches("", "*"));
}

// ── external_entry_matches ───────────────────────────────

#[test]
fn external_entry_exact_match() {
    assert!(external_entry_matches("serde", "serde"));
    assert!(external_entry_matches("tokio", "tokio"));
}

#[test]
fn external_entry_subpath_match() {
    assert!(external_entry_matches("serde/Serialize", "serde"));
    assert!(external_entry_matches("tokio/runtime", "tokio"));
    assert!(external_entry_matches("std/io/Read", "std/io"));
}

#[test]
fn external_entry_no_false_prefix() {
    // "serde" must not match "serde_json" — different package
    assert!(!external_entry_matches("serde_json", "serde"));
    assert!(!external_entry_matches("tokio_util", "tokio"));
}

#[test]
fn external_entry_wildcard() {
    assert!(external_entry_matches("anything", "*"));
    assert!(external_entry_matches("serde/Serialize", "*"));
}

#[test]
fn external_entry_no_match() {
    assert!(!external_entry_matches("clap", "serde"));
    assert!(!external_entry_matches("reqwest", "tokio"));
}

// ── build_external_map ───────────────────────────────────

#[test]
fn build_external_map_only_external_layers() {
    let mut layers = HashMap::new();
    layers.insert(
        "serialization".to_string(),
        crate::spec::Layer {
            role: "serde tools".into(),
            packages: vec!["serde".into(), "thiserror".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: true,
        },
    );
    layers.insert(
        "dictionary".to_string(),
        crate::spec::Layer {
            role: "data".into(),
            packages: vec!["src/models".into()],
            rules: HashMap::new(),
            depends_on: vec!["serialization".into()],
            external: false,
        },
    );
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };

    let ext_map = build_external_map(&spec);
    // Only external layer entries should appear
    assert!(ext_map.iter().any(|(pkg, _)| pkg == "serde"));
    assert!(ext_map.iter().any(|(pkg, _)| pkg == "thiserror"));
    // Internal layer package should NOT appear
    assert!(!ext_map.iter().any(|(pkg, _)| pkg == "src/models"));
}

#[test]
fn resolve_external_layer_finds_match() {
    let ext_map = vec![
        ("serde".into(), "serialization".into()),
        ("thiserror".into(), "serialization".into()),
        ("clap".into(), "cli".into()),
    ];
    assert_eq!(
        resolve_external_layer("serde/Serialize", &ext_map),
        Some("serialization".into())
    );
    assert_eq!(
        resolve_external_layer("clap", &ext_map),
        Some("cli".into())
    );
    assert_eq!(resolve_external_layer("tokio", &ext_map), None);
}

#[test]
fn resolve_external_layer_longest_match() {
    let ext_map = vec![
        ("tokio/sync".into(), "safe-async".into()),
        ("tokio".into(), "async-runtime".into()),
    ];
    // tokio/sync/Mutex should match safe-async (more specific)
    assert_eq!(
        resolve_external_layer("tokio/sync/Mutex", &ext_map),
        Some("safe-async".into())
    );
    // tokio/runtime should match async-runtime
    assert_eq!(
        resolve_external_layer("tokio/runtime", &ext_map),
        Some("async-runtime".into())
    );
}

// ── has_wildcard_external_access ─────────────────────────

#[test]
fn wildcard_external_access_detected() {
    let mut layers = HashMap::new();
    layers.insert(
        "all-external".to_string(),
        crate::spec::Layer {
            role: "everything".into(),
            packages: vec!["*".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: true,
        },
    );
    layers.insert(
        "runner".to_string(),
        crate::spec::Layer {
            role: "runner".into(),
            packages: vec!["src/main".into()],
            rules: HashMap::new(),
            depends_on: vec!["all-external".into()],
            external: false,
        },
    );
    layers.insert(
        "dict".to_string(),
        crate::spec::Layer {
            role: "data".into(),
            packages: vec!["src/models".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: false,
        },
    );
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };

    assert!(has_wildcard_external_access("runner", &spec));
    assert!(!has_wildcard_external_access("dict", &spec));
}

// ── internal layers excluded from external map ───────────

#[test]
fn internal_layer_excluded_from_layer_map_when_external() {
    let mut layers = HashMap::new();
    layers.insert(
        "ext".to_string(),
        crate::spec::Layer {
            role: "ext".into(),
            packages: vec!["serde".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: true,
        },
    );
    let spec = BasisSpec {
        extends: None,
        governance: crate::spec::Governance {
            version: "1.0".into(),
            model: None,
        },
        layers,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
    };

    let layer_map = build_layer_map(&spec);
    // External layer should NOT appear in the internal layer map
    assert!(layer_map.is_empty());
}

// ── is_lang_internal_import ────────────────────────────────

#[test]
fn rust_internal_imports_detected() {
    let rust = &language::rust_lang::RUST;
    assert!(is_lang_internal_import("crate/spec/BasisSpec", rust));
    assert!(is_lang_internal_import("super/something", rust));
    assert!(is_lang_internal_import("self/module", rust));
    assert!(is_lang_internal_import("std/collections/HashMap", rust));
    assert!(is_lang_internal_import("core/fmt", rust));
    assert!(is_lang_internal_import("alloc/vec", rust));
}

#[test]
fn rust_external_imports_not_internal() {
    let rust = &language::rust_lang::RUST;
    assert!(!is_lang_internal_import("serde/Serialize", rust));
    assert!(!is_lang_internal_import("tokio/runtime", rust));
    assert!(!is_lang_internal_import("clap", rust));
}

#[test]
fn python_imports_not_internal() {
    let py = &language::python::PYTHON;
    assert!(!is_lang_internal_import("os/path", py));
    assert!(!is_lang_internal_import("django/db", py));
}
