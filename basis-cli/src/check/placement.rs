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

impl Violation {
    /// The actionable fix line. Single source of truth — both the
    /// human-readable Display impl and `UnifiedViolation::help` read
    /// from this method, so the agent reading the JSON-derived help
    /// sees exactly the same prescription as a human reading
    /// `basis check` text output. The text intentionally omits the
    /// "help: " prefix; callers add it (Display does) or render it
    /// inline (the JSON `help` field is the bare prescription).
    pub fn help_text(&self) -> String {
        format!(
            "move this code into the '{}' layer, or add '{}' to depends_on in basis.yaml",
            self.to_layer, self.to_layer
        )
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B001]: import '{}' violates boundary ({} -> {})\n  --> {}:{}\n  = note: {}\n  = help: {}",
            self.import, self.from_layer, self.to_layer,
            self.file, self.line,
            self.reason,
            self.help_text()
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
    let external_map = build_external_map(spec);
    let has_external_layers = !external_map.is_empty();
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

        // Skip test files unless check_tests is enabled in the spec
        if !spec.governance.check_tests && language::is_test_file(&rel, lang) {
            return;
        }

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

        // Compute the directory of the importing file for relative path resolution
        let file_dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");

        for import in (lang.extract_imports)(&content) {
            // Normalize language-native module path to slash-separated for layer matching
            let normalized = normalize_module_path(&import.module, lang);

            // Resolve relative imports (./ and ../) against the importing file's directory.
            // This is generic filesystem path resolution, not language-specific knowledge.
            let normalized = resolve_relative_import(&normalized, file_dir);

            // Try to resolve as internal layer import
            if let Some(to_layer) = resolve_layer(&normalized, &layer_map) {
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
                continue;
            }

            // Not an internal layer import — check against external layers.
            // Only flag imports that positively match a declared external layer.
            // Imports that don't match any layer (internal or external) pass silently —
            // governance is opt-in, not opt-out.
            if has_external_layers {
                if let Some(ext_layer) = resolve_external_layer(&normalized, &external_map) {
                    if !is_allowed(&from_layer, &ext_layer, spec) {
                        violations.push(Violation {
                            file: rel.clone(),
                            line: import.line,
                            import: import.module.clone(),
                            from_layer: from_layer.clone(),
                            to_layer: ext_layer.clone(),
                            reason: format!(
                                "layer '{}' does not depend on external layer '{}'",
                                from_layer, ext_layer
                            ),
                        });
                    }
                }
            }
        }
    });

    violations
}

/// Resolve a relative import path (./ or ../) against the importing file's directory.
/// Returns the resolved path, or the original if it's not relative.
///
/// This is generic filesystem path logic — no language-specific knowledge.
/// `./utils` from `analysis/trends.ts` → `analysis/utils`
/// `../types/types` from `analysis/trends.ts` → `types/types`
fn resolve_relative_import(normalized: &str, file_dir: &str) -> String {
    if normalized.starts_with("./") {
        // Same-directory reference: prepend the file's directory
        let rest = &normalized[2..];
        if file_dir.is_empty() {
            rest.to_string()
        } else {
            format!("{file_dir}/{rest}")
        }
    } else if normalized.starts_with("../") {
        // Parent-directory reference: walk up from the file's directory
        let mut dir_parts: Vec<&str> = if file_dir.is_empty() {
            Vec::new()
        } else {
            file_dir.split('/').collect()
        };
        let mut rest = &normalized[..];
        while rest.starts_with("../") {
            dir_parts.pop(); // go up one level
            rest = &rest[3..];
        }
        if dir_parts.is_empty() {
            rest.to_string()
        } else {
            format!("{}/{rest}", dir_parts.join("/"))
        }
    } else {
        normalized.to_string()
    }
}

pub fn normalize_module_path(module: &str, lang: &LangDef) -> String {
    match lang.name {
        "python" | "java" | "kotlin" | "csharp" => module.replace('.', "/"),
        "rust" => module.replace("::", "/"),
        _ => module.to_string(), // Go, JS, Swift — already path-like or single-word
    }
}

// ── Layer resolution ────────────────────────────────────────────────────

/// Build a map of package path prefix → layer name (internal layers only).
fn build_layer_map(spec: &BasisSpec) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for (layer_name, layer) in &spec.layers {
        if layer.external {
            continue; // External layers don't own file paths
        }
        for pkg in &layer.packages {
            entries.push((pkg.clone(), layer_name.clone()));
        }
    }
    // Sort by length descending so longer (more specific) prefixes match first
    entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    entries
}

/// Build a map of external package name → external layer name.
/// Sorted by length descending so longer (more specific) entries match first.
fn build_external_map(spec: &BasisSpec) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for (layer_name, layer) in &spec.layers {
        if !layer.external {
            continue;
        }
        for pkg in &layer.packages {
            entries.push((pkg.clone(), layer_name.clone()));
        }
    }
    entries.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    entries
}

/// Check if a normalized import matches an external layer package entry
/// using segment-boundary matching.
///
/// Entry `E` matches import `I` if:
/// - `I == E` (exact match), or
/// - `I` starts with `E/` (sub-path match), or
/// - `E` is `"*"` (wildcard — matches everything)
fn external_entry_matches(import: &str, entry: &str) -> bool {
    if entry == "*" {
        return true;
    }
    import == entry || import.starts_with(&format!("{entry}/"))
}

/// Resolve an import to an external layer by matching against external package entries.
fn resolve_external_layer(import: &str, external_map: &[(String, String)]) -> Option<String> {
    for (entry, layer_name) in external_map {
        if external_entry_matches(import, entry) {
            return Some(layer_name.clone());
        }
    }
    None
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
#[path = "placement_tests.rs"]
mod tests;
