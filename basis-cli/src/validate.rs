use std::collections::{HashMap, HashSet};

use crate::spec::{BasisSpec, ExhaustiveConfig, NewtypeConfig, KNOWN_LANGUAGES};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ValidationError(String);

pub fn validate(spec: &BasisSpec) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    validate_governance(spec, &mut errors);
    validate_layer_deps(spec, &mut errors);
    validate_cycle_free(spec, &mut errors);
    validate_external_layers(spec, &mut errors);
    validate_boundaries(spec, &mut errors);
    validate_purity(spec, &mut errors);
    validate_newtypes(spec, &mut errors);
    validate_unions(spec, &mut errors);

    errors
}

// ── Spec composition (pure merge) ──────────────────────────────────

/// Merge a parent spec into a child spec. Child values take precedence.
/// This is pure logic — no IO. The loader is responsible for reading files.
///
/// Merge semantics:
/// - governance: child wins (version, model)
/// - extends: cleared (already resolved by loader)
/// - layers: union by name; child overrides parent on conflict
/// - newtypes: types appended, deduplicated by name (child wins); exclude lists merged
/// - exhaustive_matching: unions appended, deduplicated by name (child wins)
/// - purity: child replaces parent if child defines it; otherwise inherited
/// - boundaries: child replaces parent if child defines it; otherwise inherited
pub fn merge_specs(parent: &BasisSpec, child: &BasisSpec) -> BasisSpec {
    // Layers: start with parent, overlay child
    let mut layers = parent.layers.clone();
    for (name, layer) in &child.layers {
        layers.insert(name.clone(), layer.clone());
    }

    // Newtypes: merge types list, child wins on name conflict
    let newtypes = merge_newtypes(&parent.newtypes, &child.newtypes);

    // Exhaustive matching: merge unions list, child wins on name conflict
    let exhaustive_matching = merge_exhaustive(&parent.exhaustive_matching, &child.exhaustive_matching);

    // Purity: child replaces parent if present
    let purity = if child.purity.is_some() {
        child.purity.clone()
    } else {
        parent.purity.clone()
    };

    // Boundaries: child replaces parent if present
    let boundaries = if child.boundaries.is_some() {
        child.boundaries.clone()
    } else {
        parent.boundaries.clone()
    };

    BasisSpec {
        governance: child.governance.clone(),
        extends: None, // resolved
        layers,
        newtypes,
        exhaustive_matching,
        purity,
        boundaries,
    }
}

fn merge_newtypes(
    parent: &Option<NewtypeConfig>,
    child: &Option<NewtypeConfig>,
) -> Option<NewtypeConfig> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => {
            // Child enabled flag wins
            let enabled = c.enabled;

            // Merge types: child wins on name conflict
            let mut types_map: HashMap<String, crate::spec::NewtypeDef> = HashMap::new();
            for nt in &p.types {
                types_map.insert(nt.name.clone(), nt.clone());
            }
            for nt in &c.types {
                types_map.insert(nt.name.clone(), nt.clone());
            }
            let mut types: Vec<_> = types_map.into_values().collect();
            types.sort_by(|a, b| a.name.cmp(&b.name));

            // Merge exclude lists (union, deduplicated)
            let mut exclude_params: Vec<String> = p.exclude_params.clone();
            for ep in &c.exclude_params {
                if !exclude_params.contains(ep) {
                    exclude_params.push(ep.clone());
                }
            }

            let mut exclude_functions: Vec<String> = p.exclude_functions.clone();
            for ef in &c.exclude_functions {
                if !exclude_functions.contains(ef) {
                    exclude_functions.push(ef.clone());
                }
            }

            Some(NewtypeConfig {
                enabled,
                types,
                exclude_params,
                exclude_functions,
            })
        }
    }
}

fn merge_exhaustive(
    parent: &Option<ExhaustiveConfig>,
    child: &Option<ExhaustiveConfig>,
) -> Option<ExhaustiveConfig> {
    match (parent, child) {
        (None, None) => None,
        (Some(p), None) => Some(p.clone()),
        (None, Some(c)) => Some(c.clone()),
        (Some(p), Some(c)) => {
            let enabled = c.enabled;

            let mut unions_map: HashMap<String, crate::spec::UnionDef> = HashMap::new();
            for u in &p.unions {
                unions_map.insert(u.name.clone(), u.clone());
            }
            for u in &c.unions {
                unions_map.insert(u.name.clone(), u.clone());
            }
            let mut unions: Vec<_> = unions_map.into_values().collect();
            unions.sort_by(|a, b| a.name.cmp(&b.name));

            Some(ExhaustiveConfig { enabled, unions })
        }
    }
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

fn validate_external_layers(spec: &BasisSpec, errors: &mut Vec<ValidationError>) {
    for (name, layer) in &spec.layers {
        if !layer.external {
            continue;
        }
        // External layers cannot have rules (purity, io, etc.)
        if !layer.rules.is_empty() {
            errors.push(ValidationError(format!(
                "external layer '{name}' cannot have rules (purity, io, etc.) — it describes code you don't control"
            )));
        }
        // Warn if packages look like file paths (likely misconfigured)
        for pkg in &layer.packages {
            if pkg != "*" && (pkg.contains('/') && pkg.contains("src")) {
                errors.push(ValidationError(format!(
                    "external layer '{name}' has package '{pkg}' that looks like a file path — external layer packages should be package names (e.g., 'serde', 'tokio'), not file paths"
                )));
            }
        }
    }

    // Check for conflict: both external layers and boundaries.external defined
    let has_external_layers = spec.layers.values().any(|l| l.external);
    let has_boundaries_external = spec
        .boundaries
        .as_ref()
        .map(|b| !b.external.is_empty())
        .unwrap_or(false);
    if has_external_layers && has_boundaries_external {
        errors.push(ValidationError(
            "cannot use both external layers and boundaries.external — migrate boundaries.external to external layers".into()
        ));
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
#[path = "validate_tests.rs"]
mod tests;
