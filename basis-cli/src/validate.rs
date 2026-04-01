use std::collections::{HashMap, HashSet};

use crate::spec::{BasisSpec, KNOWN_LANGUAGES};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(String);

pub fn validate(spec: &BasisSpec) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    validate_governance(spec, &mut errors);
    validate_layer_deps(spec, &mut errors);
    validate_cycle_free(spec, &mut errors);
    validate_boundaries(spec, &mut errors);
    validate_purity(spec, &mut errors);
    validate_newtypes(spec, &mut errors);
    validate_unions(spec, &mut errors);

    errors
}

fn validate_governance(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    if spec.governance.version.is_empty() {
        errors.push(ValidationError("governance.version is empty".into()));
    }
}

fn validate_layer_deps(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    let layer_names: HashSet<&str> = spec.layers.keys().map(|s| s.as_str()).collect();

    for (name, layer) in &spec.layers {
        for dep in &layer.depends_on {
            if !layer_names.contains(dep.as_str()) {
                errors.push(ValidationError(format!(
                    "layer '{name}' depends on '{dep}', which does not exist"
                )));
            }
            if dep == name {
                errors.push(ValidationError(format!("layer '{name}' depends on itself")));
            }
        }
    }
}

fn validate_cycle_free(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    let layer_names: Vec<&String> = spec.layers.keys().collect();

    // Build adjacency: layer -> set of layers it depends on
    let deps: HashMap<&str, HashSet<&str>> = spec
        .layers
        .iter()
        .map(|(name, layer)| {
            let dep_set: HashSet<&str> = layer.depends_on.iter().map(|s| s.as_str()).collect();
            (name.as_str(), dep_set)
        })
        .collect();

    // Topological sort via Kahn's algorithm to detect cycles
    let mut in_degree: HashMap<&str, usize> = layer_names.iter().map(|n| (n.as_str(), 0)).collect();

    for layer_deps in deps.values() {
        for dep in layer_deps {
            if let Some(count) = in_degree.get_mut(dep) {
                *count += 1;
            }
        }
    }

    // Note: edges go from dependee to dependent (if A depends_on B, edge B->A)
    // Actually, for cycle detection we need: if A depends_on B, that's an edge A->B
    // in_degree[B] += 1 for each A that depends on B
    // Let me redo this correctly.

    // Reset
    let mut in_degree: HashMap<&str, usize> = layer_names.iter().map(|n| (n.as_str(), 0)).collect();

    // A depends_on B means A requires B, edge A->B in dependency graph
    // For topo sort, in_degree counts incoming edges
    // If A->B, then in_degree[B] += 1? No — for topo sort of dependency order,
    // B must come before A. So edge is B->A, in_degree[A] += 1.
    for (name, layer) in &spec.layers {
        in_degree.insert(name.as_str(), layer.depends_on.len());
    }

    let mut queue: Vec<&str> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&name, _)| name)
        .collect();

    let mut visited = 0usize;

    while let Some(node) = queue.pop() {
        visited += 1;
        // Find all layers that depend on this node and decrement their in-degree
        for (name, layer) in &spec.layers {
            if layer.depends_on.iter().any(|d| d == node) {
                if let Some(count) = in_degree.get_mut(name.as_str()) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push(name.as_str());
                    }
                }
            }
        }
    }

    if visited < layer_names.len() {
        let stuck: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &deg)| deg > 0)
            .map(|(&name, _)| name)
            .collect();
        errors.push(ValidationError(format!(
            "circular dependency among layers: {}",
            stuck.join(", ")
        )));
    }
}

fn validate_boundaries(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    let Some(boundaries) = &spec.boundaries else {
        return;
    };

    let layer_names: HashSet<&str> = spec.layers.keys().map(|s| s.as_str()).collect();

    for rule in &boundaries.rules {
        if !layer_names.contains(rule.from.as_str()) {
            errors.push(ValidationError(format!(
                "boundary rule references unknown layer '{}'",
                rule.from
            )));
        }
        if !layer_names.contains(rule.to.as_str()) {
            errors.push(ValidationError(format!(
                "boundary rule references unknown layer '{}'",
                rule.to
            )));
        }
        if rule.action != "allow" && rule.action != "deny" {
            errors.push(ValidationError(format!(
                "boundary rule from '{}' to '{}' has invalid action '{}' (expected 'allow' or 'deny')",
                rule.from, rule.to, rule.action
            )));
        }
    }

    for layer_name in boundaries.external.keys() {
        if !layer_names.contains(layer_name.as_str()) {
            errors.push(ValidationError(format!(
                "boundaries.external references unknown layer '{layer_name}'"
            )));
        }
    }
}

fn validate_purity(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    let Some(purity) = &spec.purity else {
        return;
    };

    let layer_names: HashSet<&str> = spec.layers.keys().map(|s| s.as_str()).collect();

    for layer_name in purity.per_layer.keys() {
        if !layer_names.contains(layer_name.as_str()) {
            errors.push(ValidationError(format!(
                "purity.per_layer references unknown layer '{layer_name}'"
            )));
        }
    }

    // Check that per-layer overrides reference the strict layers
    for layer_name in purity.per_layer.keys() {
        if let Some(layer) = spec.layers.get(layer_name) {
            let is_strict = layer
                .rules
                .get("purity")
                .map(|v| v == "strict")
                .unwrap_or(false);
            if !is_strict {
                errors.push(ValidationError(format!(
                    "purity.per_layer references layer '{layer_name}' which is not purity: strict"
                )));
            }
        }
    }
}

fn validate_languages(
    languages: &Option<Vec<String>>,
    kind: &str,
    name: &str,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(langs) = languages {
        if langs.is_empty() {
            errors.push(ValidationError(format!(
                "{kind} '{name}' has empty languages list (applies to no language)"
            )));
        }
        for lang in langs {
            if !KNOWN_LANGUAGES.contains(&lang.as_str()) {
                errors.push(ValidationError(format!(
                    "{kind} '{name}' references unknown language '{lang}'"
                )));
            }
        }
    }
}

fn validate_newtypes(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    let Some(newtypes) = &spec.newtypes else {
        return;
    };

    let mut seen = HashSet::new();
    for nt in &newtypes.types {
        if !seen.insert(&nt.name) {
            errors.push(ValidationError(format!(
                "duplicate newtype name '{}'",
                nt.name
            )));
        }
        validate_languages(&nt.languages, "newtype", &nt.name, errors);
    }
}

fn validate_unions(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    let Some(exhaustive) = &spec.exhaustive_matching else {
        return;
    };

    let mut seen = HashSet::new();

    for union_def in &exhaustive.unions {
        if !seen.insert(&union_def.name) {
            errors.push(ValidationError(format!(
                "duplicate union name '{}'",
                union_def.name
            )));
        }
        if union_def.variants.is_empty() {
            errors.push(ValidationError(format!(
                "union '{}' has no variants",
                union_def.name
            )));
        }
        validate_languages(&union_def.languages, "union", &union_def.name, errors);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::*;

    fn minimal_spec() -> BasisSpec {
        BasisSpec {
            governance: Governance {
                version: "1.0".into(),
                model: None,
            },
            layers: HashMap::new(),
            newtypes: None,
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        }
    }

    fn spec_with_layers(layers: Vec<(&str, Vec<&str>)>) -> BasisSpec {
        let mut map = HashMap::new();
        for (name, deps) in layers {
            map.insert(
                name.to_string(),
                Layer {
                    role: "test".into(),
                    packages: vec![],
                    rules: HashMap::new(),
                    depends_on: deps.into_iter().map(String::from).collect(),
                },
            );
        }
        BasisSpec {
            governance: Governance {
                version: "1.0".into(),
                model: None,
            },
            layers: map,
            newtypes: None,
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        }
    }

    #[test]
    fn valid_spec_passes() {
        let spec = minimal_spec();
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn empty_version_fails() {
        let mut spec = minimal_spec();
        spec.governance.version = "".into();
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("version is empty")));
    }

    #[test]
    fn valid_layer_deps() {
        let spec = spec_with_layers(vec![
            ("dictionary", vec![]),
            ("laboratory", vec!["dictionary"]),
            ("hands", vec!["dictionary", "laboratory"]),
        ]);
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn unknown_dep_fails() {
        let spec = spec_with_layers(vec![("a", vec!["nonexistent"])]);
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("nonexistent")));
    }

    #[test]
    fn self_dependency_fails() {
        let spec = spec_with_layers(vec![("a", vec!["a"])]);
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("depends on itself")));
    }

    #[test]
    fn acyclic_graph_passes() {
        let spec = spec_with_layers(vec![("a", vec![]), ("b", vec!["a"]), ("c", vec!["a", "b"])]);
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn two_node_cycle_fails() {
        let spec = spec_with_layers(vec![("a", vec!["b"]), ("b", vec!["a"])]);
        let errors = validate(&spec);
        assert!(errors.iter().any(|e| format!("{e}").contains("circular")));
    }

    #[test]
    fn three_node_cycle_fails() {
        let spec = spec_with_layers(vec![("a", vec!["c"]), ("b", vec!["a"]), ("c", vec!["b"])]);
        let errors = validate(&spec);
        assert!(errors.iter().any(|e| format!("{e}").contains("circular")));
    }

    #[test]
    fn valid_boundaries() {
        let mut spec = spec_with_layers(vec![("a", vec![]), ("b", vec!["a"])]);
        spec.boundaries = Some(BoundaryConfig {
            enabled: true,
            rules: vec![BoundaryRule {
                from: "a".into(),
                to: "b".into(),
                action: "deny".into(),
                reason: None,
            }],
            external: HashMap::new(),
        });
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn boundary_unknown_from_layer() {
        let mut spec = spec_with_layers(vec![("a", vec![])]);
        spec.boundaries = Some(BoundaryConfig {
            enabled: true,
            rules: vec![BoundaryRule {
                from: "ghost".into(),
                to: "a".into(),
                action: "deny".into(),
                reason: None,
            }],
            external: HashMap::new(),
        });
        let errors = validate(&spec);
        assert!(errors.iter().any(|e| format!("{e}").contains("ghost")));
    }

    #[test]
    fn boundary_invalid_action() {
        let mut spec = spec_with_layers(vec![("a", vec![]), ("b", vec![])]);
        spec.boundaries = Some(BoundaryConfig {
            enabled: true,
            rules: vec![BoundaryRule {
                from: "a".into(),
                to: "b".into(),
                action: "maybe".into(),
                reason: None,
            }],
            external: HashMap::new(),
        });
        let errors = validate(&spec);
        assert!(errors.iter().any(|e| format!("{e}").contains("maybe")));
    }

    #[test]
    fn boundary_external_unknown_layer() {
        let mut spec = spec_with_layers(vec![("a", vec![])]);
        let mut external = HashMap::new();
        external.insert(
            "ghost".to_string(),
            ExternalRules {
                allow: vec![],
                deny_patterns: vec![],
            },
        );
        spec.boundaries = Some(BoundaryConfig {
            enabled: true,
            rules: vec![],
            external,
        });
        let errors = validate(&spec);
        assert!(errors.iter().any(|e| format!("{e}").contains("ghost")));
    }

    #[test]
    fn unique_newtypes_pass() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
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
            ],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn duplicate_newtype_fails() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: true,
            types: vec![
                NewtypeDef {
                    name: "UserId".into(),
                    wraps: "string".into(),
                    validation: None,
                    languages: None,
                },
                NewtypeDef {
                    name: "UserId".into(),
                    wraps: "int".into(),
                    validation: None,
                    languages: None,
                },
            ],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("duplicate newtype")));
    }

    #[test]
    fn no_newtypes_section_passes() {
        let spec = minimal_spec();
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn unique_unions_pass() {
        let mut spec = minimal_spec();
        spec.exhaustive_matching = Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![
                UnionDef {
                    name: "A".into(),
                    variants: vec!["X".into()],
                    languages: None,
                },
                UnionDef {
                    name: "B".into(),
                    variants: vec!["Y".into()],
                    languages: None,
                },
            ],
        });
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn duplicate_union_fails() {
        let mut spec = minimal_spec();
        spec.exhaustive_matching = Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![
                UnionDef {
                    name: "A".into(),
                    variants: vec!["X".into()],
                    languages: None,
                },
                UnionDef {
                    name: "A".into(),
                    variants: vec!["Y".into()],
                    languages: None,
                },
            ],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("duplicate union")));
    }

    #[test]
    fn union_empty_variants_fails() {
        let mut spec = minimal_spec();
        spec.exhaustive_matching = Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![UnionDef {
                name: "Empty".into(),
                variants: vec![],
                languages: None,
            }],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("no variants")));
    }

    #[test]
    fn newtype_valid_languages_passes() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: true,
            types: vec![NewtypeDef {
                name: "WindowId".into(),
                wraps: "int".into(),
                validation: None,
                languages: Some(vec!["js".into(), "python".into()]),
            }],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn newtype_unknown_language_fails() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: true,
            types: vec![NewtypeDef {
                name: "WindowId".into(),
                wraps: "int".into(),
                validation: None,
                languages: Some(vec!["typescript".into()]),
            }],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("unknown language 'typescript'")));
    }

    #[test]
    fn newtype_empty_languages_fails() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: true,
            types: vec![NewtypeDef {
                name: "WindowId".into(),
                wraps: "int".into(),
                validation: None,
                languages: Some(vec![]),
            }],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("empty languages list")));
    }

    #[test]
    fn union_unknown_language_fails() {
        let mut spec = minimal_spec();
        spec.exhaustive_matching = Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![UnionDef {
                name: "Status".into(),
                variants: vec!["A".into()],
                languages: Some(vec!["cplusplus".into()]),
            }],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("unknown language 'cplusplus'")));
    }

    #[test]
    fn union_empty_languages_fails() {
        let mut spec = minimal_spec();
        spec.exhaustive_matching = Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![UnionDef {
                name: "Status".into(),
                variants: vec!["A".into()],
                languages: Some(vec![]),
            }],
        });
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("empty languages list")));
    }

    // ── Per-layer purity validation ─────────────────────

    fn spec_with_strict_layer(name: &str) -> BasisSpec {
        let mut layers = HashMap::new();
        layers.insert(
            name.to_string(),
            Layer {
                role: "test".into(),
                packages: vec![],
                rules: {
                    let mut r = HashMap::new();
                    r.insert("purity".into(), "strict".into());
                    r
                },
                depends_on: vec![],
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
                forbidden_in_strict: vec!["file_io".into()],
                per_layer: HashMap::new(),
            }),
            boundaries: None,
        }
    }

    #[test]
    fn purity_per_layer_valid_strict_layer_passes() {
        let mut spec = spec_with_strict_layer("lab");
        spec.purity.as_mut().unwrap().per_layer.insert(
            "lab".into(),
            LayerPurityOverride {
                also_forbid: vec!["stdout".into()],
                allow: vec![],
            },
        );
        assert!(validate(&spec).is_empty());
    }

    #[test]
    fn purity_per_layer_unknown_layer_fails() {
        let mut spec = spec_with_strict_layer("lab");
        spec.purity.as_mut().unwrap().per_layer.insert(
            "ghost".into(),
            LayerPurityOverride {
                also_forbid: vec![],
                allow: vec![],
            },
        );
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("unknown layer 'ghost'")));
    }

    #[test]
    fn purity_per_layer_non_strict_layer_fails() {
        let mut spec = spec_with_strict_layer("lab");
        // Add a non-strict layer
        spec.layers.insert(
            "io".into(),
            Layer {
                role: "test".into(),
                packages: vec![],
                rules: HashMap::new(),
                depends_on: vec![],
            },
        );
        spec.purity.as_mut().unwrap().per_layer.insert(
            "io".into(),
            LayerPurityOverride {
                also_forbid: vec!["stdout".into()],
                allow: vec![],
            },
        );
        let errors = validate(&spec);
        assert!(errors
            .iter()
            .any(|e| format!("{e}").contains("not purity: strict")));
    }
}
