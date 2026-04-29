use super::{FileInfo, InferredLayer};
use crate::check::placement::normalize_module_path;
use crate::language::{Import, LangRegistry};
use crate::spec::{BoundaryConfig, BoundaryRule};
use std::collections::{HashMap, HashSet};

/// Infer boundary rules from observed import patterns between layers.
///
/// Also sets `depends_on` on each layer based on import edges.
/// Returns None if fewer than 2 layers exist.
pub fn infer_boundaries(
    layers: &mut [InferredLayer],
    files: &[FileInfo],
    imports: &[(usize, Import)],
    registry: &LangRegistry,
) -> Option<BoundaryConfig> {
    if layers.len() < 2 {
        return None;
    }

    // Build file_index -> layer_name mapping
    let file_to_layer = build_file_layer_map(layers, files);

    // Build package prefix -> layer_name mapping for resolving import targets
    let mut prefix_map: Vec<(String, String)> = Vec::new();
    for layer in layers.iter() {
        for pkg in &layer.packages {
            prefix_map.push((pkg.clone(), layer.name.clone()));
        }
    }
    // Sort by length descending for longest-prefix matching
    prefix_map.sort_by_key(|e| std::cmp::Reverse(e.0.len()));

    // Collect observed edges: (from_layer, to_layer) -> count
    let mut edges: HashMap<(String, String), usize> = HashMap::new();

    for (file_idx, import) in imports {
        let Some(from_layer) = file_to_layer.get(file_idx) else {
            continue;
        };

        // Resolve import to a layer
        let lang_name = &files[*file_idx].lang_name;
        let Some(lang) = registry.for_name(lang_name) else {
            continue;
        };
        let normalized = normalize_module_path(&import.module, lang);

        let to_layer = resolve_to_layer(&normalized, &prefix_map);
        let Some(to_layer) = to_layer else {
            continue; // External import — not resolved to any layer
        };

        if from_layer == &to_layer {
            continue; // Same-layer import — not a boundary crossing
        }

        *edges.entry((from_layer.clone(), to_layer)).or_insert(0) += 1;
    }

    if edges.is_empty() {
        return None;
    }

    // Generate boundary rules (allow for each observed edge)
    let mut rules: Vec<BoundaryRule> = Vec::new();
    let mut depends_on_map: HashMap<String, HashSet<String>> = HashMap::new();

    for (from, to) in edges.keys() {
        rules.push(BoundaryRule {
            from: from.clone(),
            to: to.clone(),
            action: "allow".to_string(),
            reason: None,
        });
        depends_on_map
            .entry(from.clone())
            .or_default()
            .insert(to.clone());
    }

    // Set depends_on on layers
    for layer in layers.iter_mut() {
        if let Some(deps) = depends_on_map.get(&layer.name) {
            let mut sorted: Vec<String> = deps.iter().cloned().collect();
            sorted.sort();
            layer.depends_on = sorted;
        }
    }

    rules.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));

    Some(BoundaryConfig {
        enabled: true,
        rules,
    })
}

/// Resolve a normalized module path to a layer name.
fn resolve_to_layer(path: &str, prefix_map: &[(String, String)]) -> Option<String> {
    for (prefix, layer_name) in prefix_map {
        if path.starts_with(prefix.as_str()) {
            return Some(layer_name.clone());
        }
    }
    None
}

/// Map file indices to layer names based on package prefix matching.
fn build_file_layer_map(layers: &[InferredLayer], files: &[FileInfo]) -> HashMap<usize, String> {
    let mut map = HashMap::new();
    let mut prefixes: Vec<(&str, &str)> = Vec::new();
    for layer in layers {
        for pkg in &layer.packages {
            prefixes.push((pkg.as_str(), layer.name.as_str()));
        }
    }
    prefixes.sort_by_key(|e| std::cmp::Reverse(e.0.len()));

    for (idx, file) in files.iter().enumerate() {
        for (prefix, layer_name) in &prefixes {
            if file.rel_path.starts_with(prefix) {
                map.insert(idx, layer_name.to_string());
                break;
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::Import;

    fn make_layer(name: &str, packages: &[&str]) -> InferredLayer {
        InferredLayer {
            name: name.to_string(),
            role: "test".to_string(),
            packages: packages.iter().map(|s| s.to_string()).collect(),
            depends_on: vec![],
            is_strict: false,
        }
    }

    fn make_file(path: &str, lang: &str) -> FileInfo {
        FileInfo {
            rel_path: path.to_string(),
            lang_name: lang.to_string(),
        }
    }

    fn make_import(module: &str) -> Import {
        Import {
            module: module.to_string(),
            line: 1,
        }
    }

    #[test]
    fn basic_boundary_inference() {
        let registry = LangRegistry::new();
        let mut layers = vec![
            make_layer("api", &["src/api"]),
            make_layer("models", &["src/models"]),
        ];
        let files = vec![
            make_file("src/api/handlers.py", "python"),
            make_file("src/models/user.py", "python"),
        ];
        // api imports from models
        let imports = vec![(0, make_import("src.models.user"))];

        let config = infer_boundaries(&mut layers, &files, &imports, &registry);
        assert!(config.is_some());
        let config = config.unwrap();
        assert_eq!(config.rules.len(), 1);
        assert_eq!(config.rules[0].from, "api");
        assert_eq!(config.rules[0].to, "models");

        // Check depends_on was set
        let api = layers.iter().find(|l| l.name == "api").unwrap();
        assert!(api.depends_on.contains(&"models".to_string()));
    }

    #[test]
    fn same_layer_imports_ignored() {
        let registry = LangRegistry::new();
        let mut layers = vec![make_layer("api", &["src/api"])];
        let files = vec![
            make_file("src/api/a.py", "python"),
            make_file("src/api/b.py", "python"),
        ];
        let imports = vec![(0, make_import("src.api.b"))];

        let config = infer_boundaries(&mut layers, &files, &imports, &registry);
        assert!(config.is_none()); // Single layer, no cross-boundary imports
    }

    #[test]
    fn external_imports_ignored() {
        let registry = LangRegistry::new();
        let mut layers = vec![make_layer("api", &["src/api"])];
        let files = vec![make_file("src/api/handlers.py", "python")];
        let imports = vec![(0, make_import("requests"))]; // External lib

        let config = infer_boundaries(&mut layers, &files, &imports, &registry);
        assert!(config.is_none()); // No internal cross-layer imports
    }

    #[test]
    fn too_few_layers() {
        let registry = LangRegistry::new();
        let mut layers = vec![make_layer("all", &["src"])];
        let files = vec![];
        let imports = vec![];

        let config = infer_boundaries(&mut layers, &files, &imports, &registry);
        assert!(config.is_none());
    }

    #[test]
    fn rules_sorted() {
        let registry = LangRegistry::new();
        let mut layers = vec![
            make_layer("api", &["src/api"]),
            make_layer("models", &["src/models"]),
            make_layer("logic", &["src/logic"]),
        ];
        let files = vec![
            make_file("src/api/handlers.py", "python"),
            make_file("src/logic/validate.py", "python"),
        ];
        let imports = vec![
            (0, make_import("src.models.user")),
            (1, make_import("src.models.order")),
        ];

        let config = infer_boundaries(&mut layers, &files, &imports, &registry);
        let config = config.unwrap();
        // Rules should be sorted
        assert!(config.rules.len() >= 2);
        assert!(config.rules[0].from <= config.rules[1].from);
    }
}
