pub mod boundaries;
pub mod completeness;
pub mod extraction;
pub mod layers;
pub mod matching;
pub mod newtypes;
pub mod purity;

use crate::check::walk::walk_source_files;
use crate::language::{self, LangRegistry};
use crate::spec::{
    BasisSpec, BoundaryConfig, ExhaustiveConfig, Governance, Layer, NewtypeConfig, NewtypeDef,
    PurityConfig, UnionDef,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

// ── Data types ──────────────────────────────────────────────────────────

/// Raw parameter extracted from a function signature (no type_map filtering).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used in verbose mode and tests
pub struct RawParam {
    pub file_index: usize,
    pub line: usize,
    pub function_name: String,
    pub param_name: String,
    pub raw_type: String, // empty string if no type annotation
}

/// Raw case set from a match/switch statement (no union_map filtering).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used in verbose mode and tests
pub struct RawMatchCases {
    pub file_index: usize,
    pub line: usize,
    pub matched_expr: String, // e.g. "user.status", "order_type"
    pub cases: HashSet<String>,
    pub has_wildcard: bool,
}

/// Metadata about a source file discovered during the walk.
#[derive(Debug, Clone)]
pub struct FileInfo {
    pub rel_path: String,
    pub lang_name: String,
}

/// All raw data collected from a single codebase walk.
pub struct CollectedData {
    pub files: Vec<FileInfo>,
    pub imports: Vec<(usize, language::Import)>,
    pub params: Vec<RawParam>,
    pub match_groups: Vec<RawMatchCases>,
    pub io_hits: Vec<(usize, String)>, // (file_index, IO category)
}

/// Inferred layer before final assembly.
#[derive(Debug, Clone)]
pub struct InferredLayer {
    pub name: String,
    pub role: String,
    pub packages: Vec<String>,
    pub depends_on: Vec<String>,
    pub is_strict: bool, // set by purity inference
}

/// Statistics for the stderr summary.
#[derive(Debug, Default)]
pub struct InferStats {
    pub typed_params: usize,
    pub untyped_params_skipped: usize,
    pub candidates_before_threshold: usize,
    pub candidates_after_threshold: usize,
}

/// Ordered version of BasisSpec for deterministic YAML output.
#[derive(Debug, serde::Serialize)]
pub struct InferredSpecYaml {
    pub governance: Governance,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub layers: BTreeMap<String, Layer>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newtypes: Option<NewtypeConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exhaustive_matching: Option<ExhaustiveConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purity: Option<PurityConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boundaries: Option<BoundaryConfig>,
}

impl From<BasisSpec> for InferredSpecYaml {
    fn from(spec: BasisSpec) -> Self {
        InferredSpecYaml {
            governance: spec.governance,
            layers: spec.layers.into_iter().collect(),
            newtypes: spec.newtypes,
            exhaustive_matching: spec.exhaustive_matching,
            purity: spec.purity,
            boundaries: spec.boundaries,
        }
    }
}

// ── Collection walk ─────────────────────────────────────────────────────

/// Walk a codebase and collect all raw data for inference.
pub fn walk_and_collect(root: &Path, registry: &LangRegistry) -> CollectedData {
    let mut data = CollectedData {
        files: Vec::new(),
        imports: Vec::new(),
        params: Vec::new(),
        match_groups: Vec::new(),
        io_hits: Vec::new(),
    };

    walk_source_files(root, &mut |file_path| {
        let ext = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let Some(lang) = registry.for_ext(ext) else {
            return;
        };

        // Get relative path
        let rel_path = file_path
            .strip_prefix(root)
            .unwrap_or(file_path)
            .to_string_lossy()
            .replace('\\', "/");

        // Skip test files
        if language::is_test_file(&rel_path, lang) {
            return;
        }

        // Read file content
        let Ok(content) = std::fs::read_to_string(file_path) else {
            return;
        };

        let file_index = data.files.len();
        data.files.push(FileInfo {
            rel_path: rel_path.to_string(),
            lang_name: lang.name.to_string(),
        });

        // Imports
        let imports = (lang.extract_imports)(&content);
        for imp in imports {
            data.imports.push((file_index, imp));
        }

        // Raw params
        let mut params = extraction::extract_raw_params(&content, lang.name);
        for p in &mut params {
            p.file_index = file_index;
        }
        data.params.extend(params);

        // Raw match cases
        let mut matches = matching::extract_raw_match_cases(&content, lang.name);
        for m in &mut matches {
            m.file_index = file_index;
        }
        data.match_groups.extend(matches);

        // IO hits
        for (category, patterns) in lang.purity_imports {
            for pattern in *patterns {
                if content.contains(pattern) {
                    data.io_hits.push((file_index, category.to_string()));
                    break;
                }
            }
        }
        for (category, patterns) in lang.purity_calls {
            for pattern in *patterns {
                if content.contains(pattern) {
                    data.io_hits.push((file_index, category.to_string()));
                    break;
                }
            }
        }
    });

    data
}

// ── Assembly ────────────────────────────────────────────────────────────

/// Assemble a BasisSpec from all inferred components.
pub fn assemble_spec(
    inferred_layers: Vec<InferredLayer>,
    newtypes: Vec<NewtypeDef>,
    unions: Vec<UnionDef>,
    purity_config: Option<PurityConfig>,
    boundaries: Option<BoundaryConfig>,
) -> BasisSpec {
    let mut layers = HashMap::new();
    for il in inferred_layers {
        let mut rules = HashMap::new();
        if il.is_strict {
            rules.insert("purity".to_string(), "strict".to_string());
        }
        layers.insert(
            il.name,
            Layer {
                role: il.role,
                packages: il.packages,
                rules,
                depends_on: il.depends_on,
                external: false,
            },
        );
    }

    let newtypes_config = if newtypes.is_empty() {
        None
    } else {
        Some(NewtypeConfig {
            enabled: true,
            types: newtypes,
            exclude_params: vec![],
            exclude_functions: vec![],
        })
    };

    let exhaustive = if unions.is_empty() {
        None
    } else {
        Some(ExhaustiveConfig {
            enabled: true,
            unions,
        })
    };

    BasisSpec {
        governance: Governance::new("1.0"),
        extends: None,
        layers,
        newtypes: newtypes_config,
        exhaustive_matching: exhaustive,
        purity: purity_config,
        boundaries,
        contracts: None,
    }
}

// ── Public entry point ──────────────────────────────────────────────────

/// Inference result containing the spec and summary info.
pub struct InferResult {
    pub spec: BasisSpec,
    pub file_count: usize,
    pub layer_names: Vec<String>,
    pub newtype_names: Vec<String>,
    pub union_names: Vec<String>,
    pub strict_layers: Vec<String>,
    pub boundary_rule_count: usize,
    pub stats: InferStats,
}

/// Infer a BasisSpec from a codebase at the given root path.
pub fn infer(root: &Path, registry: &LangRegistry, min_occurrences: usize) -> InferResult {
    // Phase 1: Collect
    let data = walk_and_collect(root, registry);
    let file_count = data.files.len();

    // Phase 2: Layers
    let mut inferred_layers = layers::infer_layers(&data.files);

    // Phase 3: Newtypes and unions
    let (newtypes, stats) =
        newtypes::infer_newtypes(&data.params, &data.files, registry, min_occurrences);
    let unions = completeness::infer_unions(&data.match_groups);

    // Phase 4: Purity and boundaries
    let (purity_config, strict_layers) =
        purity::infer_purity(&inferred_layers, &data.io_hits, &data.files);
    // Mark strict layers
    for layer in &mut inferred_layers {
        if strict_layers.contains(&layer.name) {
            layer.is_strict = true;
        }
    }
    let boundaries =
        boundaries::infer_boundaries(&mut inferred_layers, &data.files, &data.imports, registry);

    let layer_names: Vec<String> = inferred_layers.iter().map(|l| l.name.clone()).collect();
    let newtype_names: Vec<String> = newtypes.iter().map(|n| n.name.clone()).collect();
    let union_names: Vec<String> = unions.iter().map(|u| u.name.clone()).collect();
    let boundary_rule_count = boundaries
        .as_ref()
        .map(|b| b.rules.len())
        .unwrap_or(0);

    // Phase 5: Assemble
    let spec = assemble_spec(inferred_layers, newtypes, unions, purity_config, boundaries);

    InferResult {
        spec,
        file_count,
        layer_names,
        newtype_names,
        union_names,
        strict_layers,
        boundary_rule_count,
        stats,
    }
}

#[cfg(test)]
#[path = "infer_tests.rs"]
mod tests;
