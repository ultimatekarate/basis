use super::*;
use crate::spec::*;

fn minimal_spec() -> BasisSpec {
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
                external: false,
            },
        );
    }
    BasisSpec {
        governance: Governance::new("1.0"),
        extends: None,
        layers: map,
        newtypes: None,
        exhaustive_matching: None,
        purity: None,
        boundaries: None,
        granularity: None,
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
    });
    let errors = validate(&spec);
    assert!(errors.iter().any(|e| format!("{e}").contains("maybe")));
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
            forbidden_in_strict: vec!["file_io".into()],
            per_layer: HashMap::new(),
        }),
        boundaries: None,
        granularity: None,
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
            external: false,
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

// ── merge_specs tests ─────────────────────────────────

#[test]
fn merge_layers_union() {
    let parent = spec_with_layers(vec![("a", vec![]), ("b", vec!["a"])]);
    let child = spec_with_layers(vec![("c", vec!["a"])]);
    let merged = merge_specs(&parent, &child);
    assert!(merged.layers.contains_key("a"));
    assert!(merged.layers.contains_key("b"));
    assert!(merged.layers.contains_key("c"));
}

#[test]
fn merge_layers_child_overrides() {
    let mut parent = spec_with_layers(vec![("a", vec![])]);
    parent.layers.get_mut("a").unwrap().role = "parent role".into();

    let mut child = spec_with_layers(vec![("a", vec![])]);
    child.layers.get_mut("a").unwrap().role = "child role".into();

    let merged = merge_specs(&parent, &child);
    assert_eq!(merged.layers["a"].role, "child role");
}

#[test]
fn merge_newtypes_appends_and_deduplicates() {
    let mut parent = minimal_spec();
    parent.newtypes = Some(NewtypeConfig {
        enabled: true,
        types: vec![
            NewtypeDef { name: "UserId".into(), wraps: "string".into(), validation: None, languages: None },
            NewtypeDef { name: "OrderId".into(), wraps: "string".into(), validation: None, languages: None },
        ],
        exclude_params: vec!["index".into()],
        exclude_functions: vec![],
    });

    let mut child = minimal_spec();
    child.newtypes = Some(NewtypeConfig {
        enabled: true,
        types: vec![
            NewtypeDef { name: "OrderId".into(), wraps: "int".into(), validation: Some("positive".into()), languages: None },
            NewtypeDef { name: "SessionId".into(), wraps: "string".into(), validation: None, languages: None },
        ],
        exclude_params: vec!["count".into()],
        exclude_functions: vec!["len".into()],
    });

    let merged = merge_specs(&parent, &child);
    let nt = merged.newtypes.unwrap();
    assert_eq!(nt.types.len(), 3); // UserId, OrderId (child wins), SessionId
    let order = nt.types.iter().find(|t| t.name == "OrderId").unwrap();
    assert_eq!(order.wraps, "int"); // child overrides
    assert!(nt.exclude_params.contains(&"index".into()));
    assert!(nt.exclude_params.contains(&"count".into()));
    assert!(nt.exclude_functions.contains(&"len".into()));
}

#[test]
fn merge_newtypes_parent_only() {
    let mut parent = minimal_spec();
    parent.newtypes = Some(NewtypeConfig {
        enabled: true,
        types: vec![NewtypeDef { name: "UserId".into(), wraps: "string".into(), validation: None, languages: None }],
        exclude_params: vec![],
        exclude_functions: vec![],
    });
    let child = minimal_spec();
    let merged = merge_specs(&parent, &child);
    assert!(merged.newtypes.is_some());
    assert_eq!(merged.newtypes.unwrap().types.len(), 1);
}

#[test]
fn merge_exhaustive_appends_and_deduplicates() {
    let mut parent = minimal_spec();
    parent.exhaustive_matching = Some(ExhaustiveConfig {
        enabled: true,
        unions: vec![
            UnionDef { name: "Status".into(), variants: vec!["A".into(), "B".into()], languages: None },
        ],
    });

    let mut child = minimal_spec();
    child.exhaustive_matching = Some(ExhaustiveConfig {
        enabled: true,
        unions: vec![
            UnionDef { name: "Status".into(), variants: vec!["X".into(), "Y".into()], languages: None },
            UnionDef { name: "Mode".into(), variants: vec!["Fast".into(), "Slow".into()], languages: None },
        ],
    });

    let merged = merge_specs(&parent, &child);
    let em = merged.exhaustive_matching.unwrap();
    assert_eq!(em.unions.len(), 2); // Status (child wins), Mode
    let status = em.unions.iter().find(|u| u.name == "Status").unwrap();
    assert_eq!(status.variants, vec!["X", "Y"]); // child overrides
}

#[test]
fn merge_purity_child_replaces() {
    let mut parent = minimal_spec();
    parent.purity = Some(PurityConfig {
        enabled: true,
        forbidden_in_strict: vec!["file_io".into(), "network_io".into()],
        per_layer: HashMap::new(),
    });

    let mut child = minimal_spec();
    child.purity = Some(PurityConfig {
        enabled: true,
        forbidden_in_strict: vec!["stdout".into()],
        per_layer: HashMap::new(),
    });

    let merged = merge_specs(&parent, &child);
    let p = merged.purity.unwrap();
    assert_eq!(p.forbidden_in_strict, vec!["stdout"]);
}

#[test]
fn merge_purity_inherited_when_child_absent() {
    let mut parent = minimal_spec();
    parent.purity = Some(PurityConfig {
        enabled: true,
        forbidden_in_strict: vec!["file_io".into()],
        per_layer: HashMap::new(),
    });
    let child = minimal_spec();
    let merged = merge_specs(&parent, &child);
    assert!(merged.purity.is_some());
    assert_eq!(merged.purity.unwrap().forbidden_in_strict, vec!["file_io"]);
}

#[test]
fn merge_extends_cleared() {
    let mut parent = minimal_spec();
    parent.extends = Some("grandparent.yaml".into());
    let mut child = minimal_spec();
    child.extends = Some("parent.yaml".into());
    let merged = merge_specs(&parent, &child);
    assert!(merged.extends.is_none());
}

#[test]
fn merge_governance_child_wins() {
    let mut parent = minimal_spec();
    parent.governance.model = Some("linguistic".into());
    let mut child = minimal_spec();
    child.governance.model = Some("hexagonal".into());
    let merged = merge_specs(&parent, &child);
    assert_eq!(merged.governance.model, Some("hexagonal".into()));
}

// ── external layer validation ─────────────────────────────

#[test]
fn external_layer_with_rules_fails() {
    let mut spec = minimal_spec();
    spec.layers.insert(
        "ext".into(),
        Layer {
            role: "external".into(),
            packages: vec!["serde".into()],
            rules: {
                let mut r = HashMap::new();
                r.insert("purity".into(), "strict".into());
                r
            },
            depends_on: vec![],
            external: true,
        },
    );
    let errors = validate(&spec);
    assert!(errors
        .iter()
        .any(|e| format!("{e}").contains("cannot have rules")));
}

#[test]
fn external_layer_without_rules_passes() {
    let mut spec = minimal_spec();
    spec.layers.insert(
        "ext".into(),
        Layer {
            role: "external".into(),
            packages: vec!["serde".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: true,
        },
    );
    assert!(validate(&spec).is_empty());
}

#[test]
fn external_layer_with_filepath_warns() {
    let mut spec = minimal_spec();
    spec.layers.insert(
        "ext".into(),
        Layer {
            role: "external".into(),
            packages: vec!["src/models".into()],
            rules: HashMap::new(),
            depends_on: vec![],
            external: true,
        },
    );
    let errors = validate(&spec);
    assert!(errors
        .iter()
        .any(|e| format!("{e}").contains("looks like a file path")));
}

