use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::language::{self, LangRegistry};
use crate::spec::BasisSpec;

/// A purity violation — forbidden IO module imported in a strict-purity layer.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub layer: String,
    pub forbidden: String,
    pub category: String,
}

impl Violation {
    /// The actionable fix line. Single source of truth shared by the
    /// Display impl and the JSON `UnifiedViolation::help` field. The
    /// text is fully static — purity violations always resolve the
    /// same two ways: relocate the code or drop the import. No
    /// "help: " prefix — callers add it.
    pub fn help_text(&self) -> String {
        "move this code to a non-strict layer, or remove the import".to_string()
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B004]: forbidden import '{}' in strict-purity layer '{}'\n  --> {}:{}\n  = note: '{}' is in the '{}' category, which is forbidden in strict layers\n  = help: {}",
            self.forbidden, self.layer,
            self.file, self.line,
            self.forbidden, self.category,
            self.help_text()
        )
    }
}

// ── Forbidden pattern sets ──────────────────────────────────────────

/// Pre-built forbidden patterns for a single language.
struct LangForbidden {
    imports: HashSet<String>,
    calls: HashSet<String>,
    import_categories: HashMap<String, String>,
    call_categories: HashMap<String, String>,
}

/// Compute the effective forbidden categories for a given layer.
/// Starts with the global `forbidden_in_strict` list, then applies per-layer overrides.
fn effective_categories(spec: &BasisSpec, layer_name: &str) -> Vec<String> {
    let purity = match spec.purity.as_ref().filter(|p| p.enabled) {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut categories: HashSet<String> = purity.forbidden_in_strict.iter().cloned().collect();

    if let Some(override_config) = purity.per_layer.get(layer_name) {
        // Add layer-specific extra categories
        for cat in &override_config.also_forbid {
            categories.insert(cat.clone());
        }
        // Remove layer-specific exemptions
        for cat in &override_config.allow {
            categories.remove(cat);
        }
    }

    categories.into_iter().collect()
}

/// Build forbidden pattern sets for a specific language and category list.
fn build_lang_forbidden(
    lang: &crate::language::LangDef,
    categories: &[String],
) -> LangForbidden {
    let mut imports = HashSet::new();
    let mut calls = HashSet::new();
    let mut import_cats = HashMap::new();
    let mut call_cats = HashMap::new();

    for category in categories {
        for &(cat, patterns) in lang.purity_imports {
            if cat == category {
                for &p in patterns {
                    imports.insert(p.to_string());
                    import_cats.insert(p.to_string(), category.clone());
                }
            }
        }
        for &(cat, patterns) in lang.purity_calls {
            if cat == category {
                for &p in patterns {
                    calls.insert(p.to_string());
                    call_cats.insert(p.to_string(), category.clone());
                }
            }
        }
    }

    LangForbidden {
        imports,
        calls,
        import_categories: import_cats,
        call_categories: call_cats,
    }
}

/// Build forbidden pattern sets for all languages using the global categories.
/// Used when no per-layer overrides exist for a layer.
fn build_all_forbidden<'a>(
    spec: &BasisSpec,
    registry: &'a LangRegistry,
) -> HashMap<&'a str, LangForbidden> {
    let categories: Vec<String> = spec
        .purity
        .as_ref()
        .filter(|p| p.enabled)
        .map(|p| p.forbidden_in_strict.clone())
        .unwrap_or_default();

    let mut result = HashMap::new();
    for lang in registry.all() {
        result.insert(lang.name, build_lang_forbidden(lang, &categories));
    }
    result
}

// ── Layer helpers ───────────────────────────────────────────────────

/// Identify which layers have purity=strict.
fn strict_layers(spec: &BasisSpec) -> HashSet<String> {
    let mut layers = HashSet::new();
    for (name, layer) in &spec.layers {
        if let Some(purity) = layer.rules.get("purity") {
            if purity == "strict" {
                layers.insert(name.clone());
            }
        }
    }
    layers
}

/// Build a mapping of package prefix → layer name.
fn build_layer_map(spec: &BasisSpec) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for (layer_name, layer) in &spec.layers {
        for pkg in &layer.packages {
            entries.push((pkg.clone(), layer_name.clone()));
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
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

// ── Main checker ────────────────────────────────────────────────────

/// Check all source files in strict-purity layers for forbidden imports and calls.
pub fn check_purity(spec: &BasisSpec, root: &Path, registry: &LangRegistry) -> Vec<Violation> {
    let global_forbidden = build_all_forbidden(spec, registry);

    // Check if any per-layer overrides exist
    let has_per_layer = spec
        .purity
        .as_ref()
        .is_some_and(|p| !p.per_layer.is_empty());

    // Quick exit: if no patterns for any language (and no per-layer overrides), nothing to check
    if !has_per_layer {
        let has_any = global_forbidden
            .values()
            .any(|f| !f.imports.is_empty() || !f.calls.is_empty());
        if !has_any {
            return Vec::new();
        }
    }

    let strict = strict_layers(spec);
    if strict.is_empty() {
        return Vec::new();
    }

    let layer_map = build_layer_map(spec);
    let mut violations = Vec::new();

    super::walk::walk_source_files(root, &mut |file_path| {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };

        let Some(layer) = resolve_layer(&rel, &layer_map) else {
            return;
        };

        if !strict.contains(&layer) {
            return;
        }

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = registry.for_ext(ext) else {
            return;
        };

        // Skip test files unless check_tests is enabled in the spec
        if !spec.governance.check_tests && language::is_test_file(&rel, lang) {
            return;
        }

        // Use per-layer forbidden set if overrides exist, otherwise global
        let layer_specific;
        let forbidden = if has_per_layer
            && spec
                .purity
                .as_ref()
                .is_some_and(|p| p.per_layer.contains_key(&layer))
        {
            let categories = effective_categories(spec, &layer);
            layer_specific = build_lang_forbidden(lang, &categories);
            &layer_specific
        } else {
            match global_forbidden.get(lang.name) {
                Some(f) => f,
                None => return,
            }
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        // Apply language-specific preprocessing (e.g., strip #[cfg(test)] for Rust)
        // Only when check_tests is false — if the user wants tests checked,
        // the preprocessor must not strip test sections.
        let content = if !spec.governance.check_tests {
            if let Some(preprocess) = lang.preprocess {
                preprocess(&content)
            } else {
                content
            }
        } else {
            content
        };

        // Check imports
        let imports = (lang.extract_imports)(&content);
        for import in imports {
            for pattern in &forbidden.imports {
                if import.module.contains(pattern.as_str())
                    || pattern.contains(import.module.as_str())
                {
                    let category = forbidden
                        .import_categories
                        .get(pattern)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    violations.push(Violation {
                        file: rel.clone(),
                        line: import.line,
                        layer: layer.clone(),
                        forbidden: import.module.clone(),
                        category,
                    });
                    break;
                }
            }
        }

        // Check calls in non-import/non-comment lines
        let comment_prefix = lang.comment_prefix;

        for (line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with(comment_prefix) {
                continue;
            }
            if trimmed.starts_with("import ")
                || trimmed.starts_with("from ")
                || trimmed.starts_with("use ")
                || trimmed.contains("require(")
            {
                continue;
            }
            if is_string_literal_line(trimmed) {
                continue;
            }

            for pattern in &forbidden.calls {
                if let Some(pos) = trimmed.find(pattern.as_str()) {
                    if is_inside_string(trimmed, pos) {
                        continue;
                    }
                    let category = forbidden
                        .call_categories
                        .get(pattern)
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    violations.push(Violation {
                        file: rel.clone(),
                        line: line_num + 1,
                        layer: layer.clone(),
                        forbidden: pattern.trim_end_matches('(').to_string(),
                        category,
                    });
                    break;
                }
            }
        }
    });

    violations
}

// ── String helpers ─────────────────────────────────────────────────

/// Check if a line is primarily a string literal (data definition, not a call).
fn is_string_literal_line(trimmed: &str) -> bool {
    trimmed.starts_with('"')
}

/// Check if a position in a line falls inside a string literal.
fn is_inside_string(line: &str, pos: usize) -> bool {
    let bytes = line.as_bytes();
    let mut in_string = false;
    let mut quote_char: u8 = 0;
    let mut i = 0;

    while i < pos && i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if b == b'\\' {
                i += 1; // Skip escaped character
            } else if b == quote_char {
                in_string = false;
            }
        } else if b == b'"' || b == b'\'' {
            in_string = true;
            quote_char = b;
        }
        i += 1;
    }

    in_string
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::LangDef;
    use crate::spec::{BasisSpec, Governance, Layer, PurityConfig};

    fn make_registry() -> LangRegistry {
        LangRegistry::new()
    }

    // ── Pattern data from LangDef ─────────────────────────

    fn get_import_patterns<'a>(lang: &'a LangDef, category: &str) -> Vec<&'a str> {
        let cat = category.to_string();
        lang.purity_imports
            .iter()
            .filter(move |(c, _)| *c == cat)
            .flat_map(|(_, patterns)| patterns.iter().copied())
            .collect()
    }

    fn get_call_patterns<'a>(lang: &'a LangDef, category: &str) -> Vec<&'a str> {
        let cat = category.to_string();
        lang.purity_calls
            .iter()
            .filter(move |(c, _)| *c == cat)
            .flat_map(|(_, patterns)| patterns.iter().copied())
            .collect()
    }

    #[test]
    fn import_patterns_file_io_python() {
        let patterns = get_import_patterns(&language::python::PYTHON, "file_io");
        assert!(patterns.contains(&"shutil"));
        assert!(patterns.contains(&"tempfile"));
    }

    #[test]
    fn import_patterns_file_io_rust() {
        let patterns = get_import_patterns(&language::rust_lang::RUST, "file_io");
        assert!(patterns.contains(&"std::fs"));
        assert!(!patterns.contains(&"shutil")); // No cross-language contamination
    }

    #[test]
    fn import_patterns_network_io() {
        let py = get_import_patterns(&language::python::PYTHON, "network_io");
        assert!(py.contains(&"requests"));
        assert!(py.contains(&"socket"));

        let rs = get_import_patterns(&language::rust_lang::RUST, "network_io");
        assert!(rs.contains(&"reqwest"));
        assert!(!rs.contains(&"requests")); // Python-only
    }

    #[test]
    fn import_patterns_unknown_category() {
        assert!(get_import_patterns(&language::python::PYTHON, "unknown_category").is_empty());
    }

    #[test]
    fn call_patterns_file_io_per_lang() {
        let py = get_call_patterns(&language::python::PYTHON, "file_io");
        assert!(py.contains(&"open("));
        assert!(!py.contains(&"File::open(")); // Rust-only

        let rs = get_call_patterns(&language::rust_lang::RUST, "file_io");
        assert!(rs.contains(&"File::open("));
        assert!(!rs.contains(&"open(")); // Python-only
    }

    #[test]
    fn call_patterns_stdout_per_lang() {
        let py = get_call_patterns(&language::python::PYTHON, "stdout");
        assert!(py.contains(&"print("));
        assert!(!py.contains(&"console.log(")); // JS-only

        let js = get_call_patterns(&language::javascript::JAVASCRIPT, "stdout");
        assert!(js.contains(&"console.log("));
        assert!(!js.contains(&"print(")); // Python-only
    }

    #[test]
    fn call_patterns_dynamic_execution() {
        let py = get_call_patterns(&language::python::PYTHON, "dynamic_execution");
        assert!(py.contains(&"eval("));
        assert!(py.contains(&"exec("));

        let java = get_call_patterns(&language::java::JAVA, "dynamic_execution");
        assert!(java.contains(&"Class.forName("));
    }

    #[test]
    fn call_patterns_subprocess() {
        let patterns = get_call_patterns(&language::python::PYTHON, "subprocess");
        assert!(patterns.contains(&"subprocess.run("));
    }

    #[test]
    fn call_patterns_process() {
        let py = get_import_patterns(&language::python::PYTHON, "process");
        assert!(py.contains(&"sys.exit"));

        let rs = get_import_patterns(&language::rust_lang::RUST, "process");
        assert!(rs.contains(&"std::process::exit"));
    }

    fn make_purity_spec() -> BasisSpec {
        let mut layers = HashMap::new();
        layers.insert(
            "lab".to_string(),
            Layer {
                role: "Pure".into(),
                packages: vec!["src/logic".into()],
                rules: {
                    let mut r = HashMap::new();
                    r.insert("purity".into(), "strict".into());
                    r
                },
                depends_on: vec![],
                external: false,
            },
        );
        layers.insert(
            "io".to_string(),
            Layer {
                role: "IO".into(),
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
            purity: Some(PurityConfig {
                enabled: true,
                forbidden_in_strict: vec!["file_io".into(), "network_io".into()],
                per_layer: HashMap::new(),
            }),
            boundaries: None,
            granularity: None,
        }
    }

    #[test]
    fn build_all_forbidden_includes_expected() {
        let spec = make_purity_spec();
        let registry = make_registry();
        let all = build_all_forbidden(&spec, &registry);

        let py = &all["python"];
        assert!(py.imports.contains("shutil"));
        assert!(py.imports.contains("requests"));
        assert!(!py.imports.contains("std::fs")); // Rust-only

        let rs = &all["rust"];
        assert!(rs.imports.contains("std::fs"));
        assert!(!rs.imports.contains("shutil")); // Python-only
    }

    #[test]
    fn build_all_forbidden_disabled() {
        let mut spec = make_purity_spec();
        spec.purity.as_mut().unwrap().enabled = false;
        let registry = make_registry();
        let all = build_all_forbidden(&spec, &registry);
        for lang in registry.all() {
            let f = &all[lang.name];
            assert!(f.imports.is_empty());
            assert!(f.calls.is_empty());
        }
    }

    #[test]
    fn is_inside_string_positive() {
        assert!(is_inside_string(r#"        "open(","#, 9));
    }

    #[test]
    fn is_inside_string_negative() {
        assert!(!is_inside_string("    open(file)", 4));
    }

    #[test]
    fn is_inside_string_after_quote() {
        assert!(!is_inside_string(r#""label", open(file)"#, 9));
    }

    #[test]
    fn strict_layers_finds_correct() {
        let spec = make_purity_spec();
        let strict = strict_layers(&spec);
        assert!(strict.contains("lab"));
        assert!(!strict.contains("io"));
    }

    #[test]
    fn strict_layers_none_strict() {
        let spec = BasisSpec {
            governance: Governance::new("1.0"),
            extends: None,
            layers: {
                let mut m = HashMap::new();
                m.insert(
                    "x".into(),
                    Layer {
                        role: "test".into(),
                        packages: vec![],
                        rules: HashMap::new(),
                        depends_on: vec![],
                        external: false,
                    },
                );
                m
            },
            newtypes: None,
            exhaustive_matching: None,
            purity: Some(PurityConfig {
                enabled: true,
                forbidden_in_strict: vec![],
                per_layer: HashMap::new(),
            }),
            boundaries: None,
            granularity: None,
        };
        assert!(strict_layers(&spec).is_empty());
    }

    #[test]
    fn extract_python_imports_from_and_import() {
        let content = "from os import path\nimport requests\nimport json, sys\n";
        let imports = (language::python::extract_imports)(content);
        assert_eq!(imports.len(), 4);
        assert_eq!(imports[0].module, "os");
        assert_eq!(imports[0].line, 1);
        assert_eq!(imports[1].module, "requests");
        assert_eq!(imports[2].module, "json");
        assert_eq!(imports[3].module, "sys");
    }

    #[test]
    fn extract_rust_imports_use() {
        let content = "use std::fs;\nuse crate::spec::BasisSpec;\n";
        let imports = (language::rust_lang::extract_imports)(content);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "std::fs");
        assert_eq!(imports[1].module, "crate::spec::BasisSpec");
    }

    #[test]
    fn extract_js_imports_quoted() {
        let content = "import axios from \"axios\";\nconst fs = require('fs');\n";
        let imports = (language::javascript::extract_imports)(content);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module, "axios");
        assert_eq!(imports[1].module, "fs");
    }

    #[test]
    fn resolve_layer_matches_prefix() {
        let layer_map = vec![
            ("src/logic".to_string(), "lab".to_string()),
            ("src/api".to_string(), "io".to_string()),
        ];
        assert_eq!(
            resolve_layer("src/logic/compute.py", &layer_map),
            Some("lab".into())
        );
        assert_eq!(
            resolve_layer("src/api/server.py", &layer_map),
            Some("io".into())
        );
        assert_eq!(resolve_layer("src/other/file.py", &layer_map), None);
    }

    #[test]
    fn ext_to_lang_maps_correctly() {
        let registry = make_registry();
        assert_eq!(registry.for_ext("py").unwrap().name, "python");
        assert_eq!(registry.for_ext("rs").unwrap().name, "rust");
        assert_eq!(registry.for_ext("ts").unwrap().name, "js");
        assert_eq!(registry.for_ext("tsx").unwrap().name, "js");
        assert_eq!(registry.for_ext("go").unwrap().name, "go");
        assert_eq!(registry.for_ext("java").unwrap().name, "java");
        assert_eq!(registry.for_ext("rb").unwrap().name, "ruby");
        assert_eq!(registry.for_ext("kt").unwrap().name, "kotlin");
        assert_eq!(registry.for_ext("swift").unwrap().name, "swift");
        assert_eq!(registry.for_ext("cs").unwrap().name, "csharp");
        assert!(registry.for_ext("zig").is_none());
    }

    #[test]
    fn violation_display() {
        let v = Violation {
            file: "src/logic/compute.py".into(),
            line: 3,
            layer: "laboratory".into(),
            forbidden: "requests".into(),
            category: "network_io".into(),
        };
        let s = format!("{v}");
        assert!(s.contains("error[B004]"));
        assert!(s.contains("--> src/logic/compute.py:3"));
        assert!(s.contains("laboratory"));
        assert!(s.contains("requests"));
        assert!(s.contains("help: move this code"));
    }

    // ── Per-layer override tests ─────────────────────────

    #[test]
    fn effective_categories_no_override() {
        let spec = make_purity_spec();
        let cats = effective_categories(&spec, "lab");
        assert!(cats.contains(&"file_io".to_string()));
        assert!(cats.contains(&"network_io".to_string()));
    }

    #[test]
    fn effective_categories_also_forbid() {
        let mut spec = make_purity_spec();
        let p = spec.purity.as_mut().unwrap();
        p.per_layer.insert(
            "lab".to_string(),
            crate::spec::LayerPurityOverride {
                also_forbid: vec!["stdout".into()],
                allow: vec![],
            },
        );
        let cats = effective_categories(&spec, "lab");
        assert!(cats.contains(&"file_io".to_string()));
        assert!(cats.contains(&"network_io".to_string()));
        assert!(cats.contains(&"stdout".to_string()));
    }

    #[test]
    fn effective_categories_allow_exemption() {
        let mut spec = make_purity_spec();
        let p = spec.purity.as_mut().unwrap();
        p.per_layer.insert(
            "lab".to_string(),
            crate::spec::LayerPurityOverride {
                also_forbid: vec![],
                allow: vec!["file_io".into()],
            },
        );
        let cats = effective_categories(&spec, "lab");
        assert!(!cats.contains(&"file_io".to_string()));
        assert!(cats.contains(&"network_io".to_string()));
    }

    #[test]
    fn effective_categories_also_forbid_and_allow() {
        let mut spec = make_purity_spec();
        let p = spec.purity.as_mut().unwrap();
        p.per_layer.insert(
            "lab".to_string(),
            crate::spec::LayerPurityOverride {
                also_forbid: vec!["stdout".into(), "env_vars".into()],
                allow: vec!["network_io".into()],
            },
        );
        let cats = effective_categories(&spec, "lab");
        // Global: file_io, network_io
        // + stdout, env_vars
        // - network_io
        assert!(cats.contains(&"file_io".to_string()));
        assert!(!cats.contains(&"network_io".to_string()));
        assert!(cats.contains(&"stdout".to_string()));
        assert!(cats.contains(&"env_vars".to_string()));
    }

    #[test]
    fn effective_categories_unknown_layer_returns_global() {
        let spec = make_purity_spec();
        let cats = effective_categories(&spec, "nonexistent");
        assert!(cats.contains(&"file_io".to_string()));
        assert!(cats.contains(&"network_io".to_string()));
    }

    #[test]
    fn build_lang_forbidden_respects_categories() {
        let registry = make_registry();
        let py = registry.for_ext("py").unwrap();

        let full = build_lang_forbidden(py, &["file_io".into(), "network_io".into()]);
        assert!(full.imports.contains("shutil"));
        assert!(full.imports.contains("requests"));

        let partial = build_lang_forbidden(py, &["file_io".into()]);
        assert!(partial.imports.contains("shutil"));
        assert!(!partial.imports.contains("requests")); // network_io not included
    }

    #[test]
    fn per_layer_override_deserializes() {
        let yaml = r#"
governance:
  version: "1.0"
purity:
  enabled: true
  forbidden_in_strict:
    - file_io
    - network_io
  per_layer:
    lab:
      also_forbid: [stdout]
      allow: [network_io]
"#;
        let spec: BasisSpec = serde_yaml::from_str(yaml).unwrap();
        let p = spec.purity.unwrap();
        assert!(p.per_layer.contains_key("lab"));
        let lab = &p.per_layer["lab"];
        assert_eq!(lab.also_forbid, vec!["stdout".to_string()]);
        assert_eq!(lab.allow, vec!["network_io".to_string()]);
    }
}
