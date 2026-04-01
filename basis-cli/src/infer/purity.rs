use super::{FileInfo, InferredLayer};
use crate::spec::PurityConfig;
use std::collections::{HashMap, HashSet};

/// Standard IO categories that are forbidden in strict layers.
const STANDARD_FORBIDDEN: &[&str] = &[
    "file_io",
    "network_io",
    "stdout",
    "stderr",
    "env_vars",
    "system_clock",
    "subprocess",
    "process",
    "dynamic_execution",
];

/// Infer purity configuration from IO usage patterns.
///
/// Layers with zero IO hits are candidates for `purity: strict`.
/// Returns (PurityConfig, list of strict layer names).
pub fn infer_purity(
    layers: &[InferredLayer],
    io_hits: &[(usize, String)],
    files: &[FileInfo],
) -> (Option<PurityConfig>, Vec<String>) {
    if layers.is_empty() {
        return (None, vec![]);
    }

    // Build file_index -> layer_name mapping
    let file_to_layer = build_file_layer_map(layers, files);

    // Count IO hits per layer
    let mut layer_io: HashMap<String, HashSet<String>> = HashMap::new();
    for (file_idx, category) in io_hits {
        if let Some(layer_name) = file_to_layer.get(file_idx) {
            layer_io
                .entry(layer_name.clone())
                .or_default()
                .insert(category.clone());
        }
    }

    // Layers with zero IO are strict candidates
    let mut strict_layers: Vec<String> = Vec::new();
    for layer in layers {
        if !layer_io.contains_key(&layer.name) || layer_io[&layer.name].is_empty() {
            strict_layers.push(layer.name.clone());
        }
    }

    if strict_layers.is_empty() {
        return (None, vec![]);
    }

    strict_layers.sort();

    let config = PurityConfig {
        enabled: true,
        forbidden_in_strict: STANDARD_FORBIDDEN.iter().map(|s| s.to_string()).collect(),
        per_layer: HashMap::new(),
    };

    (Some(config), strict_layers)
}

/// Map file indices to layer names based on package prefix matching.
fn build_file_layer_map(
    layers: &[InferredLayer],
    files: &[FileInfo],
) -> HashMap<usize, String> {
    let mut map = HashMap::new();

    // Build sorted package prefixes (longest first for best match)
    let mut prefixes: Vec<(&str, &str)> = Vec::new();
    for layer in layers {
        for pkg in &layer.packages {
            prefixes.push((pkg.as_str(), layer.name.as_str()));
        }
    }
    prefixes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

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

    fn make_layer(name: &str, packages: &[&str]) -> InferredLayer {
        InferredLayer {
            name: name.to_string(),
            role: "test".to_string(),
            packages: packages.iter().map(|s| s.to_string()).collect(),
            depends_on: vec![],
            is_strict: false,
        }
    }

    fn make_file(path: &str) -> FileInfo {
        FileInfo {
            rel_path: path.to_string(),
            lang_name: "python".to_string(),
        }
    }

    #[test]
    fn pure_layer_detected() {
        let layers = vec![
            make_layer("models", &["src/models"]),
            make_layer("api", &["src/api"]),
        ];
        let files = vec![
            make_file("src/models/user.py"),
            make_file("src/api/handlers.py"),
        ];
        // Only api has IO
        let io_hits = vec![(1, "network_io".to_string())];

        let (config, strict) = infer_purity(&layers, &io_hits, &files);
        assert!(config.is_some());
        assert_eq!(strict, vec!["models"]);
    }

    #[test]
    fn no_pure_layers() {
        let layers = vec![
            make_layer("api", &["src/api"]),
            make_layer("cli", &["src/cli"]),
        ];
        let files = vec![
            make_file("src/api/handlers.py"),
            make_file("src/cli/main.py"),
        ];
        let io_hits = vec![
            (0, "network_io".to_string()),
            (1, "stdout".to_string()),
        ];

        let (config, strict) = infer_purity(&layers, &io_hits, &files);
        assert!(config.is_none());
        assert!(strict.is_empty());
    }

    #[test]
    fn multiple_pure_layers() {
        let layers = vec![
            make_layer("models", &["src/models"]),
            make_layer("logic", &["src/logic"]),
            make_layer("api", &["src/api"]),
        ];
        let files = vec![
            make_file("src/models/user.py"),
            make_file("src/logic/validate.py"),
            make_file("src/api/handlers.py"),
        ];
        let io_hits = vec![(2, "network_io".to_string())];

        let (config, strict) = infer_purity(&layers, &io_hits, &files);
        assert!(config.is_some());
        assert_eq!(strict, vec!["logic", "models"]);
    }

    #[test]
    fn empty_layers() {
        let (config, strict) = infer_purity(&[], &[], &[]);
        assert!(config.is_none());
        assert!(strict.is_empty());
    }

    #[test]
    fn forbidden_categories_complete() {
        let layers = vec![make_layer("models", &["src/models"])];
        let files = vec![make_file("src/models/user.py")];
        let (config, _) = infer_purity(&layers, &[], &files);
        let config = config.unwrap();
        assert!(config.forbidden_in_strict.contains(&"file_io".to_string()));
        assert!(config.forbidden_in_strict.contains(&"network_io".to_string()));
        assert!(config.forbidden_in_strict.contains(&"subprocess".to_string()));
    }
}
