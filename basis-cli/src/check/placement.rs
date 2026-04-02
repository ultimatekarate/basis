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

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B001]: import '{}' violates boundary ({} -> {})\n  --> {}:{}\n  = note: {}\n  = help: move this code into the '{}' layer, or add '{}' to depends_on in basis.yaml",
            self.import, self.from_layer, self.to_layer,
            self.file, self.line,
            self.reason,
            self.to_layer, self.to_layer
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

        // Skip test files — governance applies to production code only
        if language::is_test_file(&rel, lang) {
            return;
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        for import in (lang.extract_imports)(&content) {
            // Normalize language-native module path to slash-separated for layer matching
            let normalized = normalize_module_path(&import.module, lang);

            // Legacy: check external deny_patterns (boundaries.external) if no external layers defined
            if !has_external_layers {
                if let Some(boundaries) = &spec.boundaries {
                    if let Some(ext_rules) = boundaries.external.get(&from_layer) {
                        for pattern in &ext_rules.deny_patterns {
                            if deny_pattern_matches(&import.module, pattern) {
                                violations.push(Violation {
                                    file: rel.clone(),
                                    line: import.line,
                                    import: import.module.clone(),
                                    from_layer: from_layer.clone(),
                                    to_layer: "(external)".to_string(),
                                    reason: format!(
                                        "import '{}' matches deny pattern '{}' for layer '{}'",
                                        import.module, pattern, from_layer
                                    ),
                                });
                                break;
                            }
                        }
                    }
                }
            }

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

            // Skip language-internal imports (crate-relative, std library, etc.)
            // These can never be external packages.
            if is_lang_internal_import(&normalized, lang) {
                continue;
            }

            // Not an internal layer import — check against external layers
            if has_external_layers {
                if let Some(ext_layer) = resolve_external_layer(&normalized, &external_map) {
                    // Import matches an external layer — check if allowed
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
                } else {
                    // Import doesn't match any external layer — check wildcard access
                    if !has_wildcard_external_access(&from_layer, spec) {
                        violations.push(Violation {
                            file: rel.clone(),
                            line: import.line,
                            import: import.module.clone(),
                            from_layer: from_layer.clone(),
                            to_layer: "(ungoverned external)".to_string(),
                            reason: format!(
                                "import '{}' is not in any external layer depended on by '{}'",
                                import.module, from_layer
                            ),
                        });
                    }
                }
            }
        }
    });

    violations
}

/// Normalize a language-native module path to slash-separated for layer matching.
/// Only converts separators for languages that use them (Python uses `.`, Rust uses `::`).
/// Go and JS imports are already path-like and should not be transformed.
/// Check whether a normalized import is language-internal (not an external package).
/// These imports are intra-crate references or standard library and should not be
/// checked against external layers.
fn is_lang_internal_import(normalized: &str, lang: &LangDef) -> bool {
    match lang.name {
        "rust" => {
            // crate/, super/, self/ are intra-crate references
            // std/, core/, alloc/ are Rust built-in libraries
            let prefix = normalized.split('/').next().unwrap_or("");
            matches!(prefix, "crate" | "super" | "self" | "std" | "core" | "alloc")
        }
        _ => false,
    }
}

pub fn normalize_module_path(module: &str, lang: &LangDef) -> String {
    match lang.name {
        "python" | "java" | "kotlin" | "csharp" => module.replace('.', "/"),
        "rust" => module.replace("::", "/"),
        _ => module.to_string(), // Go, JS, Swift — already path-like or single-word
    }
}

// ── Deny pattern matching ───────────────────────────────────────────────

/// Match an import module against a deny pattern.
/// Supports glob-style `*` as a wildcard (matches any substring).
/// Without `*`, falls back to substring matching for backward compatibility.
fn deny_pattern_matches(module: &str, pattern: &str) -> bool {
    if pattern.contains('*') {
        // Split on '*' and check that parts appear in order
        let parts: Vec<&str> = pattern.split('*').collect();
        let mut pos = 0;
        // First part must be a prefix
        if !parts[0].is_empty() {
            if !module.starts_with(parts[0]) {
                return false;
            }
            pos = parts[0].len();
        }
        // Last part must be a suffix
        let last = parts.len() - 1;
        if !parts[last].is_empty() {
            if !module.ends_with(parts[last]) {
                return false;
            }
            let suffix_start = module.len() - parts[last].len();
            if suffix_start < pos {
                return false;
            }
        }
        // Middle parts must appear in order
        for &part in &parts[1..last] {
            if part.is_empty() {
                continue;
            }
            if let Some(found) = module[pos..].find(part) {
                pos += found + part.len();
            } else {
                return false;
            }
        }
        true
    } else {
        module.contains(pattern)
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

/// Check if a layer has a wildcard external layer in its transitive dependencies.
fn has_wildcard_external_access(layer: &str, spec: &BasisSpec) -> bool {
    let deps = transitive_deps(layer, spec);
    for dep in &deps {
        if let Some(dep_layer) = spec.layers.get(dep) {
            if dep_layer.external && dep_layer.packages.iter().any(|p| p == "*") {
                return true;
            }
        }
    }
    false
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
