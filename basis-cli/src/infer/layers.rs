use super::{FileInfo, InferredLayer};
use std::collections::HashMap;

/// Role name heuristics based on directory name.
fn role_for_dir(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "models" | "types" | "schema" | "entities" | "dto" | "dtos" => "Data definitions",
        "domain" => "Domain logic",
        "api" | "handlers" | "routes" | "controllers" | "views" | "endpoints" => "API/IO boundary",
        "logic" | "core" | "service" | "services" | "engine" | "use_cases" | "usecases" => {
            "Business logic"
        }
        "utils" | "common" | "shared" | "lib" | "helpers" | "util" => "Shared utilities",
        "db" | "storage" | "repo" | "repos" | "persistence" | "data" | "database" | "dal" => {
            "Data access"
        }
        "cmd" | "bin" | "main" | "cli" | "app" => "Entry point",
        "internal" => "Internal modules",
        "pkg" => "Public packages",
        "config" | "settings" | "conf" => "Configuration",
        "middleware" | "interceptors" | "filters" => "Middleware",
        "test" | "tests" | "spec" | "specs" => "Tests",
        _ => "Application code",
    }
}

/// Infer layers from file directory structure.
///
/// Strategy:
/// 1. Find the directory depth that produces 2-8 candidate layers
/// 2. If a single `src/` wraps everything, use its children
/// 3. Flat directory → single layer with warning
pub fn infer_layers(files: &[FileInfo]) -> Vec<InferredLayer> {
    if files.is_empty() {
        return vec![];
    }

    // Collect unique top-level directory components
    let mut dir_counts: HashMap<String, usize> = HashMap::new();
    for f in files {
        let first = first_dir_component(&f.rel_path);
        *dir_counts.entry(first).or_insert(0) += 1;
    }

    // If there's a single top-level dir (e.g., "src"), go one level deeper
    let candidates = if dir_counts.len() == 1 {
        let top = dir_counts.keys().next().unwrap().clone();
        // Check if it's actually a directory (has children) by looking for second component
        let mut sub_counts: HashMap<String, usize> = HashMap::new();
        for f in files {
            let sub = second_dir_component(&f.rel_path, &top);
            *sub_counts.entry(sub).or_insert(0) += 1;
        }
        if sub_counts.len() >= 2 {
            // Use second-level dirs as layers, with top/ prefix in packages
            sub_counts
                .into_iter()
                .map(|(name, count)| {
                    let pkg = if name == top {
                        // Files directly in top-level dir
                        top.clone()
                    } else {
                        format!("{}/{}", top, name)
                    };
                    (name, pkg, count)
                })
                .collect::<Vec<_>>()
        } else {
            // Truly flat — single layer
            vec![(top.clone(), top, files.len())]
        }
    } else {
        // Multiple top-level dirs — use them directly
        dir_counts
            .into_iter()
            .map(|(name, count)| (name.clone(), name, count))
            .collect()
    };

    if candidates.len() < 2 {
        eprintln!(
            "Warning: Flat structure detected — single layer inferred. Consider organizing into subdirectories."
        );
    }

    let mut layers: Vec<InferredLayer> = candidates
        .into_iter()
        .filter(|(name, _, _)| {
            // Skip hidden dirs and obvious non-layer dirs
            !name.starts_with('.')
                && name != "node_modules"
                && name != "target"
                && name != "__pycache__"
                && name != "dist"
                && name != "build"
        })
        .map(|(name, pkg, _count)| InferredLayer {
            role: role_for_dir(&name).to_string(),
            packages: vec![pkg],
            depends_on: vec![],
            is_strict: false,
            name,
        })
        .collect();

    layers.sort_by(|a, b| a.name.cmp(&b.name));
    layers
}

/// Get the first directory component of a path, or the filename if flat.
fn first_dir_component(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    match normalized.split('/').next() {
        Some(first) if normalized.contains('/') => first.to_string(),
        _ => {
            // Flat file — use filename stem as grouping hint
            normalized
                .rsplit('.')
                .next_back()
                .unwrap_or(&normalized)
                .to_string()
        }
    }
}

/// Get the second directory component under a known top-level dir.
fn second_dir_component(path: &str, top: &str) -> String {
    let normalized = path.replace('\\', "/");
    let rest = normalized
        .strip_prefix(top)
        .and_then(|r| r.strip_prefix('/'));
    match rest {
        Some(r) => match r.split('/').next() {
            Some(second) if r.contains('/') => second.to_string(),
            _ => top.to_string(), // File directly in top dir, group with top
        },
        None => top.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(path: &str) -> FileInfo {
        FileInfo {
            rel_path: path.to_string(),
            lang_name: "python".to_string(),
        }
    }

    #[test]
    fn infer_from_src_subdirs() {
        let files = vec![
            fi("src/models/user.py"),
            fi("src/models/order.py"),
            fi("src/api/handlers.py"),
            fi("src/api/routes.py"),
            fi("src/logic/validate.py"),
        ];
        let layers = infer_layers(&files);
        assert!(layers.len() >= 3);
        let names: Vec<&str> = layers.iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"models"));
        assert!(names.contains(&"api"));
        assert!(names.contains(&"logic"));
    }

    #[test]
    fn infer_roles() {
        let files = vec![
            fi("src/models/user.py"),
            fi("src/api/handlers.py"),
            fi("src/utils/helpers.py"),
        ];
        let layers = infer_layers(&files);
        let models = layers.iter().find(|l| l.name == "models").unwrap();
        assert_eq!(models.role, "Data definitions");
        let api = layers.iter().find(|l| l.name == "api").unwrap();
        assert_eq!(api.role, "API/IO boundary");
    }

    #[test]
    fn infer_multiple_top_level() {
        let files = vec![
            fi("app/main.py"),
            fi("lib/utils.py"),
            fi("config/settings.py"),
        ];
        let layers = infer_layers(&files);
        assert_eq!(layers.len(), 3);
    }

    #[test]
    fn infer_flat_dir() {
        let files = vec![fi("main.py"), fi("utils.py")];
        let layers = infer_layers(&files);
        // Flat structure — each file becomes its own "layer"
        assert!(!layers.is_empty());
    }

    #[test]
    fn infer_packages_have_prefix() {
        let files = vec![
            fi("src/models/user.py"),
            fi("src/api/handlers.py"),
        ];
        let layers = infer_layers(&files);
        let models = layers.iter().find(|l| l.name == "models").unwrap();
        assert_eq!(models.packages, vec!["src/models"]);
    }

    #[test]
    fn empty_files() {
        let layers = infer_layers(&[]);
        assert!(layers.is_empty());
    }
}
