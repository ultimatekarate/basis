use std::path::Path;

use crate::language::LangRegistry;
use crate::spec::{BasisSpec, GranularityConfig};

/// A single granularity violation: one file exceeds the line budget for its layer.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub actual_lines: usize,
    pub max_lines: usize,
    pub layer: String,
}

impl Violation {
    /// The actionable fix line. Single source of truth — both Display and the
    /// JSON `help` field render through this method, so the agent reading the
    /// JSON-derived help sees exactly what a human reading `basis check` sees.
    pub fn help_text(&self) -> String {
        format!(
            "split this file by extracting helpers, types, or tests into a sibling module to fit the {}-line budget for layer '{}'",
            self.max_lines, self.layer
        )
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[B005]: file is {} lines, exceeds {}-line budget for layer '{}'\n  --> {}:{}\n  = note: large files inflate the context window every time an agent reads them\n  = help: {}",
            self.actual_lines, self.max_lines, self.layer,
            self.file, self.line,
            self.help_text()
        )
    }
}

/// Check all governed source files under `root` for granularity violations.
///
/// Granularity intentionally ignores `governance.check_tests`: a large test
/// file costs the agent's context window just like a large production file,
/// and the budget exists to bound that cost.
pub fn check_granularity(
    spec: &BasisSpec,
    root: &Path,
    registry: &LangRegistry,
) -> Vec<Violation> {
    let Some(config) = &spec.granularity else {
        return Vec::new();
    };
    if !config.enabled {
        return Vec::new();
    }

    let layer_map = build_layer_map(spec);
    let mut violations = Vec::new();

    super::walk::walk_source_files(root, &mut |file_path| {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };

        // Only governed files: must belong to a non-external layer
        let Some(layer_name) = resolve_layer(&rel, &layer_map) else {
            return;
        };

        // Only files with a registered source language extension
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if registry.for_ext(ext).is_none() {
            return;
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };
        let actual = content.lines().count();
        let max = effective_max_lines(config, &layer_name);

        if actual > max {
            violations.push(Violation {
                file: rel,
                line: 1,
                actual_lines: actual,
                max_lines: max,
                layer: layer_name,
            });
        }
    });

    violations
}

/// Resolve the line budget for a layer: per-layer override if present,
/// otherwise the global default.
fn effective_max_lines(config: &GranularityConfig, layer: &str) -> usize {
    if let Some(ovr) = config.per_layer.get(layer) {
        return ovr.max_lines;
    }
    config.max_lines
}

/// Build a map of package path prefix → layer name (internal layers only).
fn build_layer_map(spec: &BasisSpec) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for (layer_name, layer) in &spec.layers {
        if layer.external {
            continue;
        }
        for pkg in &layer.packages {
            entries.push((pkg.clone(), layer_name.clone()));
        }
    }
    // Longer prefixes win — match `basis-cli/src/check` before `basis-cli/src`.
    entries.sort_by_key(|e| std::cmp::Reverse(e.0.len()));
    entries
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
