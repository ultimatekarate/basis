use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::language::{self, LangRegistry};
use crate::spec::{applies_to_lang, BasisSpec};

/// A value axis violation — raw primitive used where a branded newtype should be.
pub struct Violation {
    pub file: String,
    pub line: usize,
    pub function_name: String,
    pub param_name: String,
    pub raw_type: String,
    pub suggested: Vec<String>,
}

impl Violation {
    /// The actionable fix line. Single source of truth shared by the
    /// Display impl and the JSON `UnifiedViolation::help` field. Two
    /// shapes: returns are phrased as "return X instead of Y", params
    /// as "use X instead of Y", because that is how the fix actually
    /// reads in code. No "help: " prefix — callers add it.
    pub fn help_text(&self) -> String {
        let suggested = self.suggested.join(" or ");
        if self.param_name == "(return)" {
            format!("return {} instead of {}", suggested, self.raw_type)
        } else {
            format!("use {} instead of {}", suggested, self.raw_type)
        }
    }
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.param_name == "(return)" {
            write!(
                f,
                "error[B002]: return type '{}' is a raw primitive where branded newtype expected\n  --> {}:{}\n  = help: {}",
                self.raw_type,
                self.file, self.line,
                self.help_text()
            )
        } else {
            write!(
                f,
                "error[B002]: parameter '{}' uses raw '{}' instead of branded newtype\n  --> {}:{}\n  = help: {}",
                self.param_name, self.raw_type,
                self.file, self.line,
                self.help_text()
            )
        }
    }
}

/// Build a map from primitive type name → list of newtype names that wrap it,
/// filtered to only include newtypes that apply to the given language.
fn build_type_map_for_lang(
    spec: &BasisSpec,
    lang_name: &str,
    lang_primitives: &[(&str, &[&str])],
) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(newtypes) = &spec.newtypes {
        if !newtypes.enabled {
            return map;
        }
        for nt in &newtypes.types {
            if !applies_to_lang(&nt.languages, lang_name) {
                continue;
            }
            for &(wraps_key, primitives) in lang_primitives {
                if wraps_key == nt.wraps {
                    for &prim in primitives {
                        map.entry(prim.to_string())
                            .or_default()
                            .push(nt.name.clone());
                    }
                }
            }
        }
    }
    // Deduplicate
    for names in map.values_mut() {
        names.sort();
        names.dedup();
    }
    map
}

/// Build a map from primitive type name → list of newtype names that wrap it.
/// Includes all newtypes across all languages (used by tests that don't need filtering).
#[cfg(test)]
fn build_type_map(spec: &BasisSpec, registry: &LangRegistry) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(newtypes) = &spec.newtypes {
        if !newtypes.enabled {
            return map;
        }
        for nt in &newtypes.types {
            for lang in registry.all() {
                for &(wraps_key, primitives) in lang.primitives {
                    if wraps_key == nt.wraps {
                        for &prim in primitives {
                            map.entry(prim.to_string())
                                .or_default()
                                .push(nt.name.clone());
                        }
                    }
                }
            }
        }
    }
    for names in map.values_mut() {
        names.sort();
        names.dedup();
    }
    map
}

/// Build snake_case → newtypes lookup, filtered to a specific language.
fn build_name_hints_for_lang(
    spec: &BasisSpec,
    lang_name: &str,
) -> HashMap<String, Vec<String>> {
    let mut name_hints: HashMap<String, Vec<String>> = HashMap::new();
    if let Some(newtypes) = &spec.newtypes {
        for nt in &newtypes.types {
            if !applies_to_lang(&nt.languages, lang_name) {
                continue;
            }
            let snake = to_snake_case(&nt.name);
            name_hints.entry(snake).or_default().push(nt.name.clone());
        }
    }
    name_hints
}

/// Convert PascalCase newtype name to snake_case for matching.
fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }
    result
}

/// Check all source files for raw primitive usage at function boundaries.
pub fn check_values(spec: &BasisSpec, root: &Path, registry: &LangRegistry) -> Vec<Violation> {
    // Quick check: are newtypes enabled at all?
    let newtypes_enabled = spec
        .newtypes
        .as_ref()
        .is_some_and(|n| n.enabled && !n.types.is_empty());
    if !newtypes_enabled {
        return Vec::new();
    }

    // Build per-language type maps and name hints
    let mut type_maps: HashMap<&str, HashMap<String, Vec<String>>> = HashMap::new();
    let mut name_hints_map: HashMap<&str, HashMap<String, Vec<String>>> = HashMap::new();
    for lang in registry.all() {
        let tm = build_type_map_for_lang(spec, lang.name, lang.primitives);
        let nh = build_name_hints_for_lang(spec, lang.name);
        type_maps.insert(lang.name, tm);
        name_hints_map.insert(lang.name, nh);
    }

    // Build exclusion sets from spec
    let exclude_params: HashSet<&str> = spec
        .newtypes
        .as_ref()
        .map(|n| n.exclude_params.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    let exclude_functions: HashSet<&str> = spec
        .newtypes
        .as_ref()
        .map(|n| n.exclude_functions.iter().map(|s| s.as_str()).collect())
        .unwrap_or_default();

    let mut violations = Vec::new();

    super::walk::walk_source_files(root, &mut |file_path| {
        let rel = match file_path.strip_prefix(root) {
            Ok(r) => r.to_string_lossy().replace('\\', "/"),
            Err(_) => return,
        };

        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Some(lang) = registry.for_ext(ext) else {
            return;
        };

        // Skip test files unless check_tests is enabled in the spec
        if !spec.governance.check_tests && language::is_test_file(&rel, lang) {
            return;
        }

        // Get the per-language type map and name hints
        let Some(type_map) = type_maps.get(lang.name) else {
            return;
        };
        if type_map.is_empty() {
            return;
        }
        let empty_hints = HashMap::new();
        let name_hints = name_hints_map.get(lang.name).unwrap_or(&empty_hints);

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => return,
        };

        let mut hits = Vec::new();
        (lang.scan_signatures)(&content, &rel, type_map, name_hints, &mut hits);

        // Filter: exclude_params, exclude_functions, inline basis:allow(B002)
        let lines: Vec<&str> = content.lines().collect();
        hits.retain(|hit| {
            // Spec-level: exclude by param name (not applicable to return types)
            if hit.param_name != "(return)" && exclude_params.contains(hit.param_name.as_str()) {
                return false;
            }
            // Spec-level: exclude by function name
            if !hit.function_name.is_empty()
                && exclude_functions.contains(hit.function_name.as_str())
            {
                return false;
            }
            // Inline: basis:allow(B002) on the declaration line or the line above
            let line_idx = hit.line.saturating_sub(1);
            if let Some(current) = lines.get(line_idx) {
                if current.contains("basis:allow(B002)") {
                    return false;
                }
            }
            if line_idx > 0 {
                if let Some(above) = lines.get(line_idx - 1) {
                    if above.contains("basis:allow(B002)") {
                        return false;
                    }
                }
            }
            true
        });

        for hit in hits {
            violations.push(Violation {
                file: rel.clone(),
                line: hit.line,
                function_name: hit.function_name,
                param_name: hit.param_name,
                raw_type: hit.raw_type,
                suggested: hit.suggested,
            });
        }
    });

    violations
}

#[cfg(test)]
#[path = "values_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "values_return_tests.rs"]
mod return_tests;
