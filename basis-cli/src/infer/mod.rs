pub mod boundaries;
pub mod completeness;
pub mod layers;
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

// ── Raw extraction functions ────────────────────────────────────────────

/// Extract all function parameters from source code, regardless of type map.
/// Returns RawParam with file_index = 0 (caller stamps correct index).
pub fn extract_raw_params(content: &str, lang_name: &str) -> Vec<RawParam> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        match lang_name {
            "python" => {
                // Skip private/dunder
                if trimmed.starts_with("def _") || trimmed.starts_with("async def _") {
                    i += 1;
                    continue;
                }
                if !trimmed.starts_with("def ") && !trimmed.starts_with("async def ") {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let fn_name = {
                    let after_kw = if let Some(rest) = trimmed.strip_prefix("async def ") {
                        rest
                    } else if let Some(rest) = trimmed.strip_prefix("def ") {
                        rest
                    } else {
                        trimmed
                    };
                    after_kw.split('(').next().unwrap_or("").trim().to_string()
                };
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() || param == "self" || param == "cls" {
                        continue;
                    }
                    if let Some((name, type_ann)) = param.split_once(':') {
                        let name = name.trim().trim_start_matches('*');
                        let type_ann = type_ann.trim().trim_end_matches('=').trim();
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    } else {
                        // Untyped param
                        let name = param.trim().trim_start_matches('*');
                        if !name.is_empty() {
                            out.push(RawParam {
                                file_index: 0,
                                line: decl_line + 1,
                                function_name: fn_name.clone(),
                                param_name: name.to_string(),
                                raw_type: String::new(),
                            });
                        }
                    }
                }
            }
            "rust" => {
                if !trimmed.starts_with("pub fn ")
                    && !trimmed.starts_with("fn ")
                    && !trimmed.starts_with("pub async fn ")
                    && !trimmed.starts_with("async fn ")
                {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let fn_name = {
                    let fn_pos = trimmed.find("fn ").unwrap_or(0) + 3;
                    let after_fn = &trimmed[fn_pos..];
                    after_fn
                        .split(&['(', '<'][..])
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty()
                        || param == "&self"
                        || param == "&mut self"
                        || param == "self"
                    {
                        continue;
                    }
                    if let Some((name, type_ann)) = param.split_once(':') {
                        let name = name.trim();
                        let type_ann = type_ann.trim().trim_start_matches('&').trim_start_matches("mut ");
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    }
                }
            }
            "js" => {
                // JS/TS: function declarations, arrow functions, methods
                let is_declaration = trimmed.contains("function ")
                    || trimmed.contains("function(")
                    || trimmed.contains("=>")
                    || trimmed.starts_with("export ")
                    || trimmed.starts_with("async ")
                    || trimmed.starts_with("const ")
                    || trimmed.starts_with("let ")
                    || trimmed.starts_with("var ")
                    || (trimmed.contains('(')
                        && (trimmed.contains('{') || trimmed.ends_with(';'))
                        && !trimmed.starts_with("if ")
                        && !trimmed.starts_with("if(")
                        && !trimmed.starts_with("for ")
                        && !trimmed.starts_with("for(")
                        && !trimmed.starts_with("while ")
                        && !trimmed.starts_with("while(")
                        && !trimmed.starts_with("switch ")
                        && !trimmed.starts_with("switch(")
                        && !trimmed.starts_with("new ")
                        && !trimmed.starts_with("return ")
                        && !trimmed.starts_with("throw "));
                if !is_declaration {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let fn_name = extract_js_fn_name(trimmed);
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    // TS-style: name: Type
                    if let Some((name, type_ann)) = param.split_once(':') {
                        let name = name.trim();
                        let type_ann = type_ann.trim();
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    } else {
                        // Untyped JS param
                        let name = param.split('=').next().unwrap_or("").trim();
                        if !name.is_empty() {
                            out.push(RawParam {
                                file_index: 0,
                                line: decl_line + 1,
                                function_name: fn_name.clone(),
                                param_name: name.to_string(),
                                raw_type: String::new(),
                            });
                        }
                    }
                }
            }
            "go" => {
                if !trimmed.starts_with("func ") {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let func_part = &trimmed[5..];
                // For methods with receivers, skip past receiver paren
                let effective_start = if func_part.starts_with('(') {
                    let receiver_end = func_part.find(')').unwrap_or(0) + 1;
                    let rest = &func_part[receiver_end..];
                    if rest.find('(').is_none() {
                        i += 1;
                        continue;
                    }
                    let receiver_abs_end = 5 + receiver_end;
                    trimmed[receiver_abs_end..].find('(').unwrap() + receiver_abs_end
                } else {
                    match trimmed.find('(') {
                        Some(p) => p,
                        None => {
                            i += 1;
                            continue;
                        }
                    }
                };
                let fn_name = {
                    let name_part = &trimmed[5..effective_start];
                    name_part
                        .split_whitespace()
                        .last()
                        .unwrap_or("")
                        .to_string()
                };
                let synthetic = &trimmed[effective_start..];
                let synthetic_lines = [synthetic];
                let (params_text, _) =
                    if let Some(result) = language::collect_params_text(&synthetic_lines, 0) {
                        (result.0, decl_line)
                    } else {
                        let mut ml_lines: Vec<&str> = vec![synthetic];
                        for line in &lines[(i + 1)..lines.len().min(i + 21)] {
                            ml_lines.push(line);
                        }
                        if let Some((text, offset)) = language::collect_params_text(&ml_lines, 0) {
                            i = decl_line + offset + 1;
                            (text, decl_line)
                        } else {
                            i += 1;
                            continue;
                        }
                    };
                if i == decl_line {
                    i += 1;
                }
                // Go params: "name type" or "name, name2 type"
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = param.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let name = parts[0].trim_start_matches('*');
                        let type_ann = parts[parts.len() - 1].trim_start_matches('*');
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    }
                }
            }
            "java" => {
                // Java: public/protected/static/abstract/final/synchronized methods
                if trimmed.contains("private ") {
                    i += 1;
                    continue;
                }
                let is_method = (trimmed.starts_with("public ")
                    || trimmed.starts_with("protected ")
                    || trimmed.starts_with("static ")
                    || trimmed.starts_with("abstract ")
                    || trimmed.starts_with("final ")
                    || trimmed.starts_with("synchronized "))
                    && trimmed.contains('(')
                    && !trimmed.starts_with("public class ")
                    && !trimmed.starts_with("public interface ")
                    && !trimmed.starts_with("public enum ");
                if !is_method {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let before_paren = trimmed.split('(').next().unwrap_or("");
                let words: Vec<&str> = before_paren.split_whitespace().collect();
                let fn_name = words.last().unwrap_or(&"").to_string();
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                // Java params: "Type name" or "@Annotation Type name"
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = param.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let name = parts[parts.len() - 1];
                        let type_ann = parts[parts.len() - 2];
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    }
                }
            }
            "kotlin" => {
                // Kotlin: fun declarations
                if trimmed.contains("private ") || trimmed.contains("internal ") {
                    i += 1;
                    continue;
                }
                if !trimmed.contains("fun ") {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let fn_name = {
                    let after_fun = trimmed.split("fun ").nth(1).unwrap_or("");
                    after_fun
                        .split(&['(', '<'][..])
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                // Kotlin params: "name: Type" or "name: Type?"
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    if let Some((name, type_ann)) = param.split_once(':') {
                        let name = name.trim().trim_start_matches("vararg ");
                        let type_ann = type_ann.trim().trim_end_matches('?').trim_end_matches('=').trim();
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    }
                }
            }
            "swift" => {
                // Swift: func declarations
                if trimmed.starts_with("private ") || trimmed.starts_with("fileprivate ") {
                    i += 1;
                    continue;
                }
                if !trimmed.contains("func ") {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let fn_name = {
                    let after_func = trimmed.split("func ").nth(1).unwrap_or("");
                    after_func
                        .split(&['(', '<'][..])
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string()
                };
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                // Swift params: "label name: Type" or "name: Type"
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    if let Some((before_colon, type_ann)) = param.split_once(':') {
                        let parts: Vec<&str> = before_colon.split_whitespace().collect();
                        let name = parts.last().unwrap_or(&"").trim_start_matches('_');
                        let type_ann = type_ann.trim().trim_end_matches('?').trim_end_matches('=').trim();
                        if !name.is_empty() {
                            out.push(RawParam {
                                file_index: 0,
                                line: decl_line + 1,
                                function_name: fn_name.clone(),
                                param_name: name.to_string(),
                                raw_type: type_ann.to_string(),
                            });
                        }
                    }
                }
            }
            "csharp" => {
                // C#: public/protected/static/abstract/virtual methods
                if trimmed.contains("private ") {
                    i += 1;
                    continue;
                }
                let is_method = (trimmed.starts_with("public ")
                    || trimmed.starts_with("protected ")
                    || trimmed.starts_with("static ")
                    || trimmed.starts_with("abstract ")
                    || trimmed.starts_with("virtual ")
                    || trimmed.starts_with("override ")
                    || trimmed.starts_with("internal ")
                    || trimmed.starts_with("async "))
                    && trimmed.contains('(')
                    && !trimmed.starts_with("public class ")
                    && !trimmed.starts_with("public interface ")
                    && !trimmed.starts_with("public enum ");
                if !is_method {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let before_paren = trimmed.split('(').next().unwrap_or("");
                let words: Vec<&str> = before_paren.split_whitespace().collect();
                let fn_name = words.last().unwrap_or(&"").to_string();
                let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) else {
                    i += 1;
                    continue;
                };
                i = end_idx + 1;
                // C# params: "Type name" or "Type? name"
                for param in params_text.split(',') {
                    let param = param.trim();
                    if param.is_empty() {
                        continue;
                    }
                    let parts: Vec<&str> = param.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let name = parts[parts.len() - 1];
                        let type_ann = parts[parts.len() - 2].trim_end_matches('?');
                        out.push(RawParam {
                            file_index: 0,
                            line: decl_line + 1,
                            function_name: fn_name.clone(),
                            param_name: name.to_string(),
                            raw_type: type_ann.to_string(),
                        });
                    }
                }
            }
            "ruby" => {
                // Ruby: def declarations (mostly untyped)
                if !trimmed.starts_with("def ") {
                    i += 1;
                    continue;
                }
                let decl_line = i;
                let after_def = &trimmed[4..];
                let fn_name = after_def
                    .split(&['(', ' ', '\n'][..])
                    .next()
                    .unwrap_or("")
                    .to_string();
                if let Some((params_text, end_idx)) = language::collect_params_text(&lines, i) {
                    i = end_idx + 1;
                    for param in params_text.split(',') {
                        let param = param.trim().trim_start_matches('*').trim_start_matches('&');
                        if param.is_empty() {
                            continue;
                        }
                        // Ruby params are untyped
                        let name = param.split('=').next().unwrap_or("").trim().trim_start_matches(':');
                        if !name.is_empty() {
                            out.push(RawParam {
                                file_index: 0,
                                line: decl_line + 1,
                                function_name: fn_name.clone(),
                                param_name: name.to_string(),
                                raw_type: String::new(),
                            });
                        }
                    }
                } else {
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    out
}

/// Helper to extract JS/TS function name from a declaration line.
fn extract_js_fn_name(line: &str) -> String {
    // "function foo(" -> "foo"
    if let Some(pos) = line.find("function ") {
        let after = &line[pos + 9..];
        let name = after.split(&['(', '<', ' '][..]).next().unwrap_or("");
        if !name.is_empty() {
            return name.to_string();
        }
    }
    // "const foo = (" or "const foo = async ("
    for kw in &["const ", "let ", "var ", "export const ", "export let "] {
        if let Some(pos) = line.find(kw) {
            let after = &line[pos + kw.len()..];
            let name = after.split(&['=', ':', ' ', '('][..]).next().unwrap_or("").trim();
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    // Method shorthand: "methodName(" at start
    let name = line
        .trim()
        .split(&['(', '<', ':'][..])
        .next()
        .unwrap_or("")
        .split_whitespace()
        .last()
        .unwrap_or("")
        .to_string();
    name
}

/// Extract all match/switch case sets from source code.
/// Returns RawMatchCases with file_index = 0 (caller stamps correct index).
pub fn extract_raw_match_cases(content: &str, lang_name: &str) -> Vec<RawMatchCases> {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();

    for (line_num, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        match lang_name {
            "python" => {
                // Python match statement
                if trimmed.starts_with("match ") && trimmed.ends_with(':') {
                    let matched_expr = trimmed[6..trimmed.len() - 1].trim().to_string();
                    let mut cases = HashSet::new();
                    let mut has_wildcard = false;
                    for subsequent in &lines[line_num + 1..] {
                        let sub = subsequent.trim();
                        if sub.starts_with("case ") {
                            if sub.contains("case _:") {
                                has_wildcard = true;
                                break;
                            }
                            if let Some(val) = extract_case_value_python(sub) {
                                cases.insert(val);
                            }
                        } else if !sub.is_empty()
                            && !sub.starts_with('#')
                            && indent_level(subsequent) <= indent_level(line)
                        {
                            break;
                        }
                    }
                    if !cases.is_empty() {
                        out.push(RawMatchCases {
                            file_index: 0,
                            line: line_num + 1,
                            matched_expr,
                            cases,
                            has_wildcard,
                        });
                    }
                }
                // Python if/elif chains
                if trimmed.starts_with("if ") && trimmed.contains("==") {
                    let matched_expr = extract_comparison_lhs(trimmed);
                    let mut cases = HashSet::new();
                    let mut has_wildcard = false;
                    if let Some(val) = extract_comparison_rhs(trimmed) {
                        cases.insert(val);
                    }
                    for subsequent in &lines[line_num + 1..] {
                        let sub = subsequent.trim();
                        if sub.starts_with("elif ") && sub.contains("==") {
                            if let Some(val) = extract_comparison_rhs(sub) {
                                cases.insert(val);
                            }
                        } else if sub.starts_with("else:") {
                            has_wildcard = true;
                            break;
                        } else if !sub.is_empty()
                            && !sub.starts_with('#')
                            && indent_level(subsequent) <= indent_level(line)
                        {
                            break;
                        }
                    }
                    if cases.len() >= 2 {
                        out.push(RawMatchCases {
                            file_index: 0,
                            line: line_num + 1,
                            matched_expr,
                            cases,
                            has_wildcard,
                        });
                    }
                }
            }
            "rust" => {
                if trimmed.starts_with("match ") && trimmed.contains('{') {
                    let matched_expr = trimmed[6..]
                        .split('{')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let mut cases = HashSet::new();
                    let mut has_wildcard = false;
                    for subsequent in &lines[line_num + 1..] {
                        let sub = subsequent.trim();
                        if sub == "}" {
                            break;
                        }
                        if sub.starts_with('_') && sub.contains("=>") {
                            has_wildcard = true;
                            break;
                        }
                        if sub.contains("=>") {
                            let pattern = sub.split("=>").next().unwrap_or("").trim();
                            for part in pattern.split('|') {
                                let part = part.trim();
                                let clean = if part.contains("::") {
                                    part.rsplit("::").next().unwrap_or("")
                                } else {
                                    part.split('(')
                                        .next()
                                        .unwrap_or("")
                                        .split('{')
                                        .next()
                                        .unwrap_or("")
                                };
                                let clean = clean.trim();
                                if !clean.is_empty() && clean != "_" {
                                    cases.insert(clean.to_string());
                                }
                            }
                        }
                    }
                    if !cases.is_empty() {
                        out.push(RawMatchCases {
                            file_index: 0,
                            line: line_num + 1,
                            matched_expr,
                            cases,
                            has_wildcard,
                        });
                    }
                }
            }
            "js" | "java" | "csharp" | "go" | "kotlin" | "swift" => {
                // switch/when statements
                let is_switch = trimmed.starts_with("switch ")
                    || trimmed.starts_with("switch(")
                    || (lang_name == "kotlin" && trimmed.starts_with("when "))
                    || (lang_name == "kotlin" && trimmed.starts_with("when("));
                if !is_switch {
                    continue;
                }
                let matched_expr = extract_switch_expr(trimmed);
                let mut cases = HashSet::new();
                let mut has_wildcard = false;
                let mut brace_depth = 0;
                let mut started = false;
                for subsequent in &lines[line_num..] {
                    let sub = subsequent.trim();
                    if sub.contains('{') {
                        brace_depth += sub.matches('{').count();
                        started = true;
                    }
                    if sub.contains('}') {
                        brace_depth -= sub.matches('}').count().min(brace_depth);
                        if started && brace_depth == 0 {
                            break;
                        }
                    }
                    if sub.starts_with("case ") {
                        let val = sub
                            .trim_start_matches("case ")
                            .split(':')
                            .next()
                            .unwrap_or("")
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'');
                        // Strip enum prefix: EnumName.Value -> Value
                        let val = val.rsplit('.').next().unwrap_or(val);
                        if !val.is_empty() {
                            cases.insert(val.to_string());
                        }
                    }
                    if sub.starts_with("default:")
                        || sub.starts_with("default =>")
                        || sub.starts_with("else ->")
                        || sub.starts_with("else =>")
                    {
                        has_wildcard = true;
                    }
                }
                if !cases.is_empty() {
                    out.push(RawMatchCases {
                        file_index: 0,
                        line: line_num + 1,
                        matched_expr,
                        cases,
                        has_wildcard,
                    });
                }
            }
            "ruby" => {
                // Ruby case/when
                if trimmed.starts_with("case ") || trimmed == "case" {
                    let matched_expr = if trimmed.len() > 5 {
                        trimmed[5..].trim().to_string()
                    } else {
                        String::new()
                    };
                    let mut cases = HashSet::new();
                    let mut has_wildcard = false;
                    for subsequent in &lines[line_num + 1..] {
                        let sub = subsequent.trim();
                        if sub == "end" {
                            break;
                        }
                        if let Some(after_when) = sub.strip_prefix("when ") {
                            let val = after_when
                                .split(&[',', '\n'][..])
                                .next()
                                .unwrap_or("")
                                .trim()
                                .trim_matches('"')
                                .trim_matches('\'')
                                .trim_matches(':');
                            if !val.is_empty() {
                                cases.insert(val.to_string());
                            }
                        }
                        if sub.starts_with("else") {
                            has_wildcard = true;
                        }
                    }
                    if !cases.is_empty() {
                        out.push(RawMatchCases {
                            file_index: 0,
                            line: line_num + 1,
                            matched_expr,
                            cases,
                            has_wildcard,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    out
}

/// Extract Python case value from "case Value:" line.
fn extract_case_value_python(case_line: &str) -> Option<String> {
    let rest = case_line.trim().strip_prefix("case ")?.split(':').next()?.trim();
    if rest.starts_with('"') && rest.ends_with('"') {
        Some(rest.trim_matches('"').to_string())
    } else if rest.contains('(') {
        Some(rest.split('(').next()?.trim().to_string())
    } else if rest.contains('.') {
        Some(rest.rsplit('.').next()?.trim().to_string())
    } else {
        Some(rest.to_string())
    }
}

/// Extract the left-hand side of an == comparison for union naming.
fn extract_comparison_lhs(line: &str) -> String {
    let stripped = line.trim().trim_start_matches("if ").trim_start_matches("elif ");
    stripped
        .split("==")
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Extract the right-hand side value of an == comparison.
fn extract_comparison_rhs(line: &str) -> Option<String> {
    let rhs = line.split("==").nth(1)?.trim();
    let rhs = rhs.split(':').next()?.trim();
    let val = rhs
        .trim_matches('"')
        .trim_matches('\'')
        .rsplit('.')
        .next()?
        .trim();
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// Extract the expression from a switch/when statement.
fn extract_switch_expr(line: &str) -> String {
    let after_kw = if line.starts_with("switch(") || line.starts_with("when(") {
        let start = line.find('(').unwrap_or(0) + 1;
        let end = line.rfind(')').unwrap_or(line.len());
        &line[start..end]
    } else if let Some(rest) = line.strip_prefix("switch ") {
        rest
    } else {
        line.strip_prefix("when ").unwrap_or("")
    };
    after_kw
        .split('{')
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .to_string()
}

/// Indentation level helper.
fn indent_level(line: &str) -> usize {
    line.len() - line.trim_start().len()
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
        let mut params = extract_raw_params(&content, lang.name);
        for p in &mut params {
            p.file_index = file_index;
        }
        data.params.extend(params);

        // Raw match cases
        let mut matches = extract_raw_match_cases(&content, lang.name);
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
        governance: Governance {
            version: "1.0".to_string(),
            model: None,
        },
        extends: None,
        layers,
        newtypes: newtypes_config,
        exhaustive_matching: exhaustive,
        purity: purity_config,
        boundaries,
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
mod tests {
    use super::*;

    #[test]
    fn extract_python_params() {
        let content = "def get_user(user_id: str, name: str) -> None:\n    pass\n";
        let params = extract_raw_params(content, "python");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "user_id");
        assert_eq!(params[0].raw_type, "str");
        assert_eq!(params[0].function_name, "get_user");
        assert_eq!(params[1].param_name, "name");
    }

    #[test]
    fn extract_python_untyped() {
        let content = "def foo(x, y):\n    pass\n";
        let params = extract_raw_params(content, "python");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].raw_type, "");
    }

    #[test]
    fn extract_rust_params() {
        let content = "pub fn get_user(user_id: String, count: usize) -> User {\n}\n";
        let params = extract_raw_params(content, "rust");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "user_id");
        assert_eq!(params[0].raw_type, "String");
        assert_eq!(params[1].param_name, "count");
        assert_eq!(params[1].raw_type, "usize");
    }

    #[test]
    fn extract_java_params() {
        let content = "public User getUser(String userId, int count) {\n}\n";
        let params = extract_raw_params(content, "java");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "userId");
        assert_eq!(params[0].raw_type, "String");
        assert_eq!(params[1].param_name, "count");
        assert_eq!(params[1].raw_type, "int");
    }

    #[test]
    fn extract_go_params() {
        let content = "func GetUser(userId string, count int) *User {\n}\n";
        let params = extract_raw_params(content, "go");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "userId");
        assert_eq!(params[0].raw_type, "string");
    }

    #[test]
    fn extract_kotlin_params() {
        let content = "fun getUser(userId: String, count: Int): User {\n}\n";
        let params = extract_raw_params(content, "kotlin");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "userId");
        assert_eq!(params[0].raw_type, "String");
    }

    #[test]
    fn extract_swift_params() {
        let content = "func getUser(for userId: String) -> User {\n}\n";
        let params = extract_raw_params(content, "swift");
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].param_name, "userId");
        assert_eq!(params[0].raw_type, "String");
    }

    #[test]
    fn extract_csharp_params() {
        let content = "public User GetUser(string userId, int count) {\n}\n";
        let params = extract_raw_params(content, "csharp");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "userId");
        assert_eq!(params[0].raw_type, "string");
    }

    #[test]
    fn extract_ruby_params_untyped() {
        let content = "def get_user(user_id, name)\n  # ...\nend\n";
        let params = extract_raw_params(content, "ruby");
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].param_name, "user_id");
        assert_eq!(params[0].raw_type, "");
    }

    #[test]
    fn extract_rust_match_cases() {
        let content = "match status {\n    Pending => {},\n    Active => {},\n}\n";
        let groups = extract_raw_match_cases(content, "rust");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].matched_expr, "status");
        assert!(groups[0].cases.contains("Pending"));
        assert!(groups[0].cases.contains("Active"));
        assert!(!groups[0].has_wildcard);
    }

    #[test]
    fn extract_rust_match_wildcard() {
        let content = "match status {\n    Pending => {},\n    _ => {},\n}\n";
        let groups = extract_raw_match_cases(content, "rust");
        assert_eq!(groups.len(), 1);
        assert!(groups[0].has_wildcard);
    }

    #[test]
    fn extract_python_match_cases() {
        let content = "match order.status:\n    case \"Pending\":\n        pass\n    case \"Shipped\":\n        pass\n";
        let groups = extract_raw_match_cases(content, "python");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].matched_expr, "order.status");
        assert!(groups[0].cases.contains("Pending"));
        assert!(groups[0].cases.contains("Shipped"));
    }

    #[test]
    fn extract_js_switch_cases() {
        let content = "switch (status) {\n    case \"Active\":\n        break;\n    case \"Inactive\":\n        break;\n}\n";
        let groups = extract_raw_match_cases(content, "js");
        assert_eq!(groups.len(), 1);
        assert!(groups[0].cases.contains("Active"));
        assert!(groups[0].cases.contains("Inactive"));
    }
}
