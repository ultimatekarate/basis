use crate::spec::BasisSpec;

pub fn generate_report(spec: &BasisSpec, format: &str) -> String {
    match format {
        "json" => generate_json(spec),
        _ => generate_text(spec),
    }
}

fn generate_json(spec: &BasisSpec) -> String {
    serde_json::to_string_pretty(spec).unwrap_or_else(|e| format!("Error serializing spec: {e}"))
}

fn generate_text(spec: &BasisSpec) -> String {
    let mut out = String::new();

    // Header
    out.push_str(&format!(
        "Basis Governance Spec v{}\n",
        spec.governance.version
    ));
    if let Some(model) = &spec.governance.model {
        out.push_str(&format!("Model: {model}\n"));
    }
    out.push('\n');

    // Layers
    if !spec.layers.is_empty() {
        out.push_str("== Layers ==\n\n");
        let mut layers: Vec<_> = spec.layers.iter().collect();
        layers.sort_by_key(|(_, l)| l.depends_on.len());

        for (name, layer) in &layers {
            out.push_str(&format!("  {name}\n"));
            out.push_str(&format!("    Role: {}\n", layer.role));
            if !layer.packages.is_empty() {
                out.push_str(&format!("    Packages: {}\n", layer.packages.join(", ")));
            }
            if !layer.depends_on.is_empty() {
                out.push_str(&format!(
                    "    Depends on: {}\n",
                    layer.depends_on.join(", ")
                ));
            }
            if !layer.rules.is_empty() {
                let rules: Vec<String> = layer
                    .rules
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                out.push_str(&format!("    Rules: {}\n", rules.join(", ")));
            }
            out.push('\n');
        }
    }

    // Newtypes
    if let Some(newtypes) = &spec.newtypes {
        if newtypes.enabled && !newtypes.types.is_empty() {
            out.push_str("== Newtypes (Values Axis) ==\n\n");
            for nt in &newtypes.types {
                let validation = nt
                    .validation
                    .as_deref()
                    .map(|v| format!(" [{v}]"))
                    .unwrap_or_default();
                let langs = nt
                    .languages
                    .as_ref()
                    .map(|l| format!(" ({})", l.join(", ")))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "  {} -> {}{}{}\n",
                    nt.name, nt.wraps, validation, langs
                ));
            }
            out.push('\n');
        }
    }

    // Exhaustive matching
    if let Some(exhaustive) = &spec.exhaustive_matching {
        if exhaustive.enabled && !exhaustive.unions.is_empty() {
            out.push_str("== Unions (Completeness Axis) ==\n\n");
            for union_def in &exhaustive.unions {
                let langs = union_def
                    .languages
                    .as_ref()
                    .map(|l| format!(" ({})", l.join(", ")))
                    .unwrap_or_default();
                out.push_str(&format!("  {}{}\n", union_def.name, langs));
                for variant in &union_def.variants {
                    out.push_str(&format!("    - {variant}\n"));
                }
            }
            out.push('\n');
        }
    }

    // Purity
    if let Some(purity) = &spec.purity {
        if purity.enabled && !purity.forbidden_in_strict.is_empty() {
            out.push_str("== Forbidden in Strict Layers (Purity Axis) ==\n\n");
            for item in &purity.forbidden_in_strict {
                out.push_str(&format!("  - {item}\n"));
            }
            out.push('\n');
        }
    }

    // Boundaries
    if let Some(boundaries) = &spec.boundaries {
        if boundaries.enabled && !boundaries.rules.is_empty() {
            out.push_str("== Boundary Rules (Placement Axis) ==\n\n");
            for rule in &boundaries.rules {
                let reason = rule
                    .reason
                    .as_deref()
                    .map(|r| format!("  # {r}"))
                    .unwrap_or_default();
                out.push_str(&format!(
                    "  {} -> {}: {}{}\n",
                    rule.from, rule.to, rule.action, reason
                ));
            }
            out.push('\n');
        }

        if !boundaries.external.is_empty() {
            out.push_str("== External Dependency Rules ==\n\n");
            let mut externals: Vec<_> = boundaries.external.iter().collect();
            externals.sort_by_key(|(k, _)| (*k).clone());

            for (layer, rules) in &externals {
                out.push_str(&format!("  {layer}\n"));
                if !rules.allow.is_empty() {
                    out.push_str(&format!("    Allow: {}\n", rules.allow.join(", ")));
                }
                if !rules.deny_patterns.is_empty() {
                    out.push_str(&format!("    Deny: {}\n", rules.deny_patterns.join(", ")));
                }
            }
            out.push('\n');
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::*;
    use std::collections::HashMap;

    fn minimal_spec() -> BasisSpec {
        BasisSpec {
            governance: Governance {
                version: "1.0".into(),
                model: Some("linguistic".into()),
            },
            layers: HashMap::new(),
            newtypes: None,
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        }
    }

    #[test]
    fn text_report_includes_version() {
        let spec = minimal_spec();
        let report = generate_report(&spec, "text");
        assert!(report.contains("v1.0"));
        assert!(report.contains("linguistic"));
    }

    #[test]
    fn text_report_includes_layers() {
        let mut spec = minimal_spec();
        spec.layers.insert(
            "dictionary".into(),
            Layer {
                role: "Data types".into(),
                packages: vec!["src/models".into()],
                rules: HashMap::new(),
                depends_on: vec![],
            },
        );
        let report = generate_report(&spec, "text");
        assert!(report.contains("dictionary"));
        assert!(report.contains("Data types"));
        assert!(report.contains("src/models"));
    }

    #[test]
    fn text_report_includes_newtypes() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: true,
            types: vec![NewtypeDef {
                name: "UserId".into(),
                wraps: "string".into(),
                validation: None,
                languages: None,
            }],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        let report = generate_report(&spec, "text");
        assert!(report.contains("UserId"));
        assert!(report.contains("string"));
    }

    #[test]
    fn text_report_skips_disabled_newtypes() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: false,
            types: vec![NewtypeDef {
                name: "UserId".into(),
                wraps: "string".into(),
                validation: None,
                languages: None,
            }],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        let report = generate_report(&spec, "text");
        assert!(!report.contains("UserId"));
    }

    #[test]
    fn text_report_includes_unions() {
        let mut spec = minimal_spec();
        spec.exhaustive_matching = Some(ExhaustiveConfig {
            enabled: true,
            unions: vec![UnionDef {
                name: "OrderStatus".into(),
                variants: vec!["Pending".into(), "Shipped".into()],
                languages: None,
            }],
        });
        let report = generate_report(&spec, "text");
        assert!(report.contains("OrderStatus"));
        assert!(report.contains("Pending"));
        assert!(report.contains("Shipped"));
    }

    #[test]
    fn text_report_includes_purity() {
        let mut spec = minimal_spec();
        spec.purity = Some(PurityConfig {
            enabled: true,
            forbidden_in_strict: vec!["file_io".into(), "network_io".into()],
            per_layer: std::collections::HashMap::new(),
        });
        let report = generate_report(&spec, "text");
        assert!(report.contains("file_io"));
        assert!(report.contains("network_io"));
    }

    #[test]
    fn text_report_includes_boundaries() {
        let mut spec = minimal_spec();
        spec.boundaries = Some(BoundaryConfig {
            enabled: true,
            rules: vec![BoundaryRule {
                from: "dict".into(),
                to: "lab".into(),
                action: "deny".into(),
                reason: Some("not allowed".into()),
            }],
            external: HashMap::new(),
        });
        let report = generate_report(&spec, "text");
        assert!(report.contains("dict"));
        assert!(report.contains("lab"));
        assert!(report.contains("deny"));
        assert!(report.contains("not allowed"));
    }

    #[test]
    fn json_report_is_valid_json() {
        let spec = minimal_spec();
        let report = generate_report(&spec, "json");
        let parsed: serde_json::Value = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed["governance"]["version"], "1.0");
    }

    #[test]
    fn json_report_roundtrips() {
        let mut spec = minimal_spec();
        spec.newtypes = Some(NewtypeConfig {
            enabled: true,
            types: vec![NewtypeDef {
                name: "UserId".into(),
                wraps: "string".into(),
                validation: None,
                languages: None,
            }],
            exclude_params: vec![],
            exclude_functions: vec![],
        });
        let report = generate_report(&spec, "json");
        let parsed: BasisSpec = serde_json::from_str(&report).unwrap();
        assert_eq!(parsed.newtypes.unwrap().types[0].name, "UserId");
    }

    #[test]
    fn empty_spec_text_report() {
        let spec = BasisSpec {
            governance: Governance {
                version: "1.0".into(),
                model: None,
            },
            layers: HashMap::new(),
            newtypes: None,
            exhaustive_matching: None,
            purity: None,
            boundaries: None,
        };
        let report = generate_report(&spec, "text");
        assert!(report.contains("v1.0"));
        // Should not crash or contain empty sections
        assert!(!report.contains("== Layers =="));
    }

    #[test]
    fn unknown_format_defaults_to_text() {
        let spec = minimal_spec();
        let report = generate_report(&spec, "xml");
        assert!(report.contains("v1.0")); // text output
    }
}
