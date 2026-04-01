use std::collections::HashSet;
use std::path::Path;

use crate::language::{self, LangDef, LangRegistry};
use crate::spec::BasisSpec;

/// A single placement violation.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub import: String,
    pub from_layer: String,
    pub to_layer: String,
    pub reason: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B001]: import '{}' violates boundary ({} -> {})\n  --> {}:{}\n  = note: {}\n  = help: move this code into the '{}' layer, or add '{}' to depends_on in basis.yaml",
            self.import, self.from_layer, self.to_layer,
            self.file, self.line,
            self.reason,
            self.to_layer, self.to_layer
        )
    }
}

/// Check all source files under `root` for placement violations against the spec.
pub fn check_placement(spec: &BasisSpec, root: &Path, registry: &LangRegistry) -> Vec<Violation> {
    // If boundaries section exists and is disabled, skip placement checks entirely.
    if let Some(boundaries) = &spec.boundaries {
        if !boundaries.enabled {
            return Vec::new();
        }
    }

    let layer_map = build_layer_map(spec);
    let mut violations = Vec::new();

    super::walk::walk_source_files(root, &mut |file_path| {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };

        let Some(from_layer) = resolve_layer(&rel, &layer_map) else {
            return; // File not in any layer — skip
        };

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = registry.for_ext(ext) else {
            return;
        };

        // Skip test files — governance applies to production code only
        if language::is_test_file(&rel, lang) {
            return;
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        for import in (lang.extract_imports)(&content) {
            // Normalize language-native module path to slash-separated for layer matching
            let normalized = normalize_module_path(&import.module, lang);

            // Check external deny_patterns for the source layer
            if let Some(boundaries) = &spec.boundaries {
                if let Some(ext_rules) = boundaries.external.get(&from_layer) {
                    for pattern in &ext_rules.deny_patterns {
                        if deny_pattern_matches(&import.module, pattern) {
                            violations.push(Violation {
                                file: rel.clone(),
                                line: import.line,
                                import: import.module.clone(),
                                from_layer: from_layer.clone(),
                                to_layer: "(external)".to_string(),
                                reason: format!(
                                    "import '{}' matches deny pattern '{}' for layer '{}'",
                                    import.module, pattern, from_layer
                                ),
                            });
                            break;
                        }
                    }
                }
            }

            let Some(to_layer) = resolve_layer(&normalized, &layer_map) else {
                continue; // Import not in any governed layer — skip
            };

            if to_layer == from_layer {
                continue; // Same layer — always allowed
            }

            if !is_allowed(&from_layer, &to_layer, spec) {
                violations.push(Violation {
                    file: rel.clone(),
                    line: import.line,
                    import: normalized,
                    from_layer: from_layer.clone(),
                    to_layer: to_layer.clone(),
                    reason: get_deny_reason(spec, &from_layer, &to_layer),
                });
            }
        }
    });

    violations
}

/// Normalize a language-native module path to slash-separated for layer matching.
/// Only converts separators for languages that use them (Python uses `.`, Rust uses `::`).
/// Go and JS imports are already path-like and should not be transformed.
fn normalize_module_path(module: &str, lang: &LangDef) -> String {
    match lang.name {
        "python" | "java" | "kotlin" | "csharp" => module.replace('.', "/"),
        "rust" => module.replace("::", "/"),
        _ => module.to_string(), // Go, JS, Swift — already path-like or single-word
    }
}

// ── Deny pattern matching ───────────────────────────────────────────────

/// Match an import module against a deny pattern.
/// Supports glob-style `*` as a wildcard (matches any substring).
/// Without `*`, falls back to substring matching for backward compatibility.
fn deny_pattern_matches(module: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        // Split on '*' and check that parts appear in order
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut pos = 0;
        // First part must be a prefix
        if !parts[0].is_empty() {
            if !module.starts_with(parts[0]) {
                return false;
            }
            pos = parts[0].len();
        }
        // Last part must be a suffix
        let last = parts.len() - 1;
        if !parts[last].is_empty() {
            if !module.ends_with(parts[last]) {
                return false;
            }
            let suffix_start = module.len() - parts[last].len();
            if suffix_start < pos {
                return false;
            }
        }
        // Middle parts must appear in order
        for &part in &parts[1..last] {
            if part.is_empty() {
                continue;
            }
            if let Some(found) = module[pos..].find(part) {
                pos += found + part.len();
            } else {
                return false;
            }
        }
        true
    } else {
        module.contains(pattern)
    }
}

// ── Layer resolution ────────────────────────────────────────────────────

/// Build a map of package path prefix → layer name.
fn build_layer_map(spec: &BasisSpec) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for (layer_name, layer) in &spec.layers {
        for pkg in &layer.packages {
            entries.push((pkg.clone(), layer_name.clone()));
        }
    }
    // Sort by length descending so longer (more specific) prefixes match first
    entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    entries
}

/// Resolve a file path to its layer by matching against package prefixes.
fn resolve_layer(path: &str, layer_map: &[(String, String)]) -> Option<String> {
    for (prefix, layer_name) in layer_map {
        if path.starts_with(prefix.as_str()) {
            return Some(layer_name.clone());
        }
    }
    None
}

// ── Dependency rules ────────────────────────────────────────────────────

/// Compute the transitive closure of depends_on for a layer (BFS).
fn transitive_deps(layer: &str, spec: &BasisSpec) -> HashSet<String> {
    let mut reachable = HashSet::new();
    let mut stack: Vec<String> = spec
        .layers
        .get(layer)
        .map(|l| l.depends_on.clone())
        .unwrap_or_default();
    while let Some(dep) = stack.pop() {
        if reachable.insert(dep.clone()) {
            if let Some(dep_layer) = spec.layers.get(&dep) {
                stack.extend(dep_layer.depends_on.iter().cloned());
            }
        }
    }
    reachable
}

/// Check if an import from one layer to another is allowed.
fn is_allowed(from: &str, to: &str, spec: &BasisSpec) -> bool {
    // Check explicit boundary rules first
    if let Some(boundaries) = &spec.boundaries {
        for rule in &boundaries.rules {
            if rule.from == from && rule.to == to {
                return rule.action == "allow";
            }
        }
    }

    // Fall back to transitive depends_on
    transitive_deps(from, spec).contains(to)
}

fn get_deny_reason(spec: &BasisSpec, from: &str, to: &str) -> String {
    if let Some(boundaries) = &spec.boundaries {
        for rule in &boundaries.rules {
            if rule.from == from && rule.to == to {
                if let Some(reason) = &rule.reason {
                    return reason.clone();
                }
            }
        }
    }
    format!("layer '{from}' is not allowed to depend on layer '{to}'")
}

#[cfg(test)]
mod tests {
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
                },
            );
            spec.layers.insert(
                "specific".into(),
                crate::spec::Layer {
                    role: "".into(),
                    packages: vec!["src/models".into()],
                    rules: HashMap::new(),
                    depends_on: vec![],
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
            },
        );
        layers.insert(
            "b".to_string(),
            crate::spec::Layer {
                role: "".into(),
                packages: vec![],
                rules: HashMap::new(),
                depends_on: vec!["a".into()],
            },
        );
        let spec = BasisSpec {
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
            },
        );
        layers.insert(
            "b".to_string(),
            crate::spec::Layer {
                role: "".into(),
                packages: vec![],
                rules: HashMap::new(),
                depends_on: vec!["a".into()],
            },
        );
        layers.insert(
            "c".to_string(),
            crate::spec::Layer {
                role: "".into(),
                packages: vec![],
                rules: HashMap::new(),
                depends_on: vec!["b".into()],
            },
        );
        let spec = BasisSpec {
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
}
