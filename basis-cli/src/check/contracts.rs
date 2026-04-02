use std::path::Path;

use crate::language::{self, LangRegistry};
use crate::spec::BasisSpec;

/// A contract violation — a function is missing precondition guards for one or more parameters.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub function_name: String,
    pub missing_param: String,
    pub layer: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B005]: function '{}' has no precondition referencing parameter '{}'\n  --> {}:{}\n  = note: layer '{}' requires precondition guards for public function parameters\n  = help: add a guard clause (assert, if/raise, if/return) that validates '{}'",
            self.function_name, self.missing_param,
            self.file, self.line,
            self.layer, self.missing_param
        )
    }
}

/// Build a mapping of package prefix → layer name.
fn build_layer_map(spec: &BasisSpec) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for (layer_name, layer) in &spec.layers {
        if layer.external {
            continue;
        }
        for pkg in &layer.packages {
            entries.push((pkg.clone(), layer_name.clone()));
        }
    }
    entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    entries
}

fn resolve_layer(path: &str, layer_map: &[(String, String)]) -> Option<String> {
    for (prefix, layer_name) in layer_map {
        if path.starts_with(prefix.as_str()) {
            return Some(layer_name.clone());
        }
    }
    None
}

/// Get the effective precondition scope for a layer.
/// Returns (scope, must_reference) — e.g. ("public", "parameters").
fn effective_config<'a>(spec: &'a BasisSpec, layer_name: &str) -> (&'a str, &'a str) {
    let contracts = match spec.contracts.as_ref().filter(|c| c.enabled) {
        Some(c) => c,
        None => return ("none", "none"),
    };

    // Check per-layer override first
    if let Some(override_config) = contracts.per_layer.get(layer_name) {
        if let Some(ref pre) = override_config.preconditions {
            return (&pre.scope, &pre.must_reference);
        }
    }

    // Fall back to global config
    (&contracts.preconditions.scope, &contracts.preconditions.must_reference)
}

/// Check all source files for missing precondition guards.
pub fn check_contracts(spec: &BasisSpec, root: &Path, registry: &LangRegistry) -> Vec<Violation> {
    let contracts = match spec.contracts.as_ref().filter(|c| c.enabled) {
        Some(c) => c,
        None => return Vec::new(),
    };

    // Quick exit if global scope is "none" and no per-layer overrides
    if contracts.preconditions.scope == "none" && contracts.per_layer.is_empty() {
        return Vec::new();
    }

    let layer_map = build_layer_map(spec);
    let mut violations = Vec::new();

    super::walk::walk_source_files(root, &mut |file_path| {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };

        let Some(layer_name) = resolve_layer(&rel, &layer_map) else {
            return;
        };

        let (scope, must_reference) = effective_config(spec, &layer_name);

        if scope == "none" || must_reference == "none" {
            return;
        }

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = registry.for_ext(ext) else {
            return;
        };

        // Skip test files unless check_tests is enabled
        if !spec.governance.check_tests && language::is_test_file(&rel, lang) {
            return;
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        // Apply language-specific preprocessing when not checking tests
        let content = if !spec.governance.check_tests {
            if let Some(preprocess) = lang.preprocess {
                preprocess(&content)
            } else {
                content
            }
        } else {
            content
        };

        let functions = (lang.scan_contracts)(&content);

        for func in &functions {
            // Check if this function is in scope
            let in_scope = match scope {
                "public" => func.is_public,
                "all" => true,
                "constructors" => func.is_constructor,
                _ => false,
            };

            if !in_scope {
                continue;
            }

            // Check which parameters are missing guards
            if must_reference == "parameters" {
                for param in &func.params {
                    if !func.guarded_params.contains(param) {
                        violations.push(Violation {
                            file: rel.clone(),
                            line: func.line,
                            function_name: func.name.clone(),
                            missing_param: param.clone(),
                            layer: layer_name.clone(),
                        });
                    }
                }
            }
        }
    });

    violations
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::spec::{
        BasisSpec, ContractConfig, Governance, Layer, LayerContractOverride,
        PreconditionConfig,
    };

    fn make_contract_spec() -> BasisSpec {
        let mut layers = HashMap::new();
        layers.insert(
            "lab".to_string(),
            Layer {
                role: "Pure logic".into(),
                packages: vec!["src/logic".into()],
                rules: HashMap::new(),
                depends_on: vec![],
                external: false,
            },
        );
        layers.insert(
            "io".to_string(),
            Layer {
                role: "IO layer".into(),
                packages: vec!["src/api".into()],
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
            contracts: Some(ContractConfig {
                enabled: true,
                preconditions: PreconditionConfig {
                    scope: "public".into(),
                    must_reference: "parameters".into(),
                },
                invariants: None,
                per_layer: HashMap::new(),
            }),
        }
    }

    #[test]
    fn contracts_disabled_returns_empty() {
        let mut spec = make_contract_spec();
        spec.contracts.as_mut().unwrap().enabled = false;
        let registry = LangRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn contracts_scope_none_returns_empty() {
        let mut spec = make_contract_spec();
        spec.contracts.as_mut().unwrap().preconditions.scope = "none".into();
        let registry = LangRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn python_function_with_guard_passes() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.py"),
            "def process(amount, account):\n    assert amount > 0\n    assert account is not None\n    return amount\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty(), "Expected no violations, got: {:?}", violations.iter().map(|v| format!("{}: {} missing {}", v.file, v.function_name, v.missing_param)).collect::<Vec<_>>());
    }

    #[test]
    fn python_function_missing_guard_violates() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.py"),
            "def process(amount, account):\n    return amount * 2\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert_eq!(violations.len(), 2);
        assert!(violations.iter().any(|v| v.missing_param == "amount"));
        assert!(violations.iter().any(|v| v.missing_param == "account"));
    }

    #[test]
    fn python_private_function_skipped() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.py"),
            "def _internal(x):\n    return x\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn python_if_raise_counts_as_guard() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.py"),
            "def process(amount):\n    if amount <= 0: raise ValueError('bad')\n    return amount\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn file_not_in_layer_ignored() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let other_dir = dir.path().join("src").join("other");
        std::fs::create_dir_all(&other_dir).unwrap();
        std::fs::write(
            other_dir.join("compute.py"),
            "def process(amount):\n    return amount\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn per_layer_override_scope() {
        let mut spec = make_contract_spec();
        // Global scope is "none", but io layer overrides to "public"
        spec.contracts.as_mut().unwrap().preconditions.scope = "none".into();
        spec.contracts.as_mut().unwrap().per_layer.insert(
            "io".to_string(),
            LayerContractOverride {
                preconditions: Some(PreconditionConfig {
                    scope: "public".into(),
                    must_reference: "parameters".into(),
                }),
            },
        );

        let registry = LangRegistry::new();
        let dir = tempfile::tempdir().unwrap();

        // File in lab layer — should NOT be checked (global scope is "none")
        let lab_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&lab_dir).unwrap();
        std::fs::write(
            lab_dir.join("compute.py"),
            "def process(x):\n    return x\n",
        ).unwrap();

        // File in io layer — SHOULD be checked (per-layer override)
        let io_dir = dir.path().join("src").join("api");
        std::fs::create_dir_all(&io_dir).unwrap();
        std::fs::write(
            io_dir.join("server.py"),
            "def handle(request):\n    return request\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].layer, "io");
        assert_eq!(violations[0].missing_param, "request");
    }

    #[test]
    fn rust_function_with_assert_passes() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.rs"),
            "pub fn process(count: usize) {\n    assert!(count > 0);\n    // work\n}\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn rust_function_missing_guard_violates() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.rs"),
            "pub fn process(count: usize, name: &str) {\n    println!(\"processing\");\n}\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert_eq!(violations.len(), 2);
    }

    #[test]
    fn rust_private_function_skipped() {
        let spec = make_contract_spec();
        let registry = LangRegistry::new();

        let dir = tempfile::tempdir().unwrap();
        let logic_dir = dir.path().join("src").join("logic");
        std::fs::create_dir_all(&logic_dir).unwrap();
        std::fs::write(
            logic_dir.join("compute.rs"),
            "fn internal(x: i32) -> i32 {\n    x\n}\n",
        ).unwrap();

        let violations = check_contracts(&spec, dir.path(), &registry);
        assert!(violations.is_empty());
    }

    #[test]
    fn violation_display_format() {
        let v = Violation {
            file: "src/logic/compute.py".into(),
            line: 5,
            function_name: "process".into(),
            missing_param: "amount".into(),
            layer: "laboratory".into(),
        };
        let s = format!("{v}");
        assert!(s.contains("error[B005]"));
        assert!(s.contains("process"));
        assert!(s.contains("amount"));
        assert!(s.contains("--> src/logic/compute.py:5"));
        assert!(s.contains("help:"));
    }
}
