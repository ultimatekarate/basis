pub mod csharp;
pub mod go;
pub mod java;
pub mod javascript;
pub mod kotlin;
pub mod python;
pub mod ruby;
pub mod rust_lang;
pub mod swift;

use std::collections::{HashMap, HashSet};

/// A single import found in source code.
pub struct Import {
    pub module: String, // Language-native form: "os.path", "std::fs", "net/http"
    pub line: usize,
}

/// Intermediate output from signature scanning — checker converts to Violation.
pub struct SignatureHit {
    pub line: usize,
    pub function_name: String,
    pub param_name: String,
    pub raw_type: String,
    pub suggested: Vec<String>,
}

/// Intermediate output from match scanning — checker converts to Violation.
pub struct MatchHit {
    pub line: usize,
    pub union_name: String,
    pub missing_variants: Vec<String>,
}

/// How to detect test files for a language.
pub struct TestFilePatterns {
    pub path_contains: &'static [&'static str],
    pub filename_prefixes: &'static [&'static str],
    pub filename_suffixes: &'static [&'static str],
}

/// Scanner function: finds raw-primitive parameters in function signatures.
pub type SignatureScanner = fn(
    content: &str,
    file: &str,
    type_map: &HashMap<String, Vec<String>>,
    name_hints: &HashMap<String, Vec<String>>,
    out: &mut Vec<SignatureHit>,
);

/// Scanner function: finds non-exhaustive match/switch statements.
pub type MatchScanner = fn(
    content: &str,
    file: &str,
    variant_index: &HashMap<String, (String, HashSet<String>)>,
    union_map: &HashMap<String, HashSet<String>>,
    out: &mut Vec<MatchHit>,
);

/// Generator function: emits a newtype wrapper given (name, primitive, optional validation hint).
pub type NewtypeGenerator = fn(&str, &str, Option<&str>) -> String;

/// A complete language definition.
pub struct LangDef {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub comment_prefix: &'static str,

    // Functions — each language implements these
    pub extract_imports: fn(&str) -> Vec<Import>,
    pub scan_signatures: SignatureScanner,
    pub scan_matches: MatchScanner,

    // Data tables
    pub test_file: TestFilePatterns,
    pub primitives: &'static [(&'static str, &'static [&'static str])],
    pub purity_imports: &'static [(&'static str, &'static [&'static str])],
    pub purity_calls: &'static [(&'static str, &'static [&'static str])],

    // Optional preprocessing (e.g., strip #[cfg(test)] for Rust)
    pub preprocess: Option<fn(&str) -> String>,

    // Generation — projecting spec into idiomatic code
    pub preferred_type: &'static [(&'static str, &'static str)],
    pub generate_preamble: Option<fn(bool, bool) -> String>,
    pub generate_newtype: Option<NewtypeGenerator>,
    pub generate_union: Option<fn(&str, &[String]) -> String>,
    pub generate_match_scaffold: Option<fn(&str, &[String]) -> String>,
    pub type_file_name: &'static str,
}

/// Registry mapping file extensions to language definitions.
pub struct LangRegistry {
    langs: Vec<&'static LangDef>,
    ext_map: HashMap<&'static str, usize>,
}

impl Default for LangRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl LangRegistry {
    pub fn new() -> Self {
        let langs: Vec<&'static LangDef> = vec![
            &python::PYTHON,
            &rust_lang::RUST,
            &javascript::JAVASCRIPT,
            &go::GO,
            &java::JAVA,
            &kotlin::KOTLIN,
            &ruby::RUBY,
            &swift::SWIFT,
            &csharp::CSHARP,
        ];
        let mut ext_map = HashMap::new();
        for (i, lang) in langs.iter().enumerate() {
            for ext in lang.extensions {
                ext_map.insert(*ext, i);
            }
        }
        LangRegistry { langs, ext_map }
    }

    pub fn for_ext(&self, ext: &str) -> Option<&'static LangDef> {
        self.ext_map.get(ext).map(|&i| self.langs[i])
    }

    pub fn for_name(&self, name: &str) -> Option<&'static LangDef> {
        self.langs.iter().find(|l| l.name == name).copied()
    }

    pub fn all(&self) -> &[&'static LangDef] {
        &self.langs
    }
}

/// Try to find which union a set of handled cases belongs to.
/// Returns (union_name, missing_variants) if cases partially match a union.
pub fn find_union_for_cases(
    cases: &HashSet<String>,
    variant_index: &HashMap<String, (String, HashSet<String>)>,
    union_map: &HashMap<String, HashSet<String>>,
) -> Option<(String, Vec<String>)> {
    if cases.is_empty() {
        return None;
    }

    let mut best_union: Option<String> = None;
    let mut best_overlap = 0;

    for case in cases {
        if let Some((union_name, _)) = variant_index.get(case) {
            if let Some(all_variants) = union_map.get(union_name) {
                let overlap = cases.intersection(all_variants).count();
                if overlap > best_overlap {
                    best_union = Some(union_name.clone());
                    best_overlap = overlap;
                }
            }
        }
    }

    if let Some(union_name) = best_union {
        if let Some(all_variants) = union_map.get(&union_name) {
            let missing: Vec<String> = all_variants
                .iter()
                .filter(|v| !cases.contains(*v))
                .cloned()
                .collect();
            if !missing.is_empty() {
                return Some((union_name, missing));
            }
        }
    }

    None
}

/// Split an identifier into lowercase words at camelCase, underscore, or acronym boundaries.
///   "userId"        -> ["user", "id"]
///   "user_id"       -> ["user", "id"]
///   "primaryUserId" -> ["primary", "user", "id"]
///   "userID"        -> ["user", "id"]
///   "HTTPClient"    -> ["http", "client"]
///   "valid"         -> ["valid"]
pub fn split_identifier_words(name: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();

    let chars: Vec<char> = name.chars().collect();
    for i in 0..chars.len() {
        let ch = chars[i];

        if ch == '_' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }

        if ch.is_uppercase() {
            let next_is_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();
            let prev_is_upper = i > 0 && chars[i - 1].is_uppercase();

            if !current.is_empty() {
                // Start new word at camelCase boundary (lowercase->uppercase)
                // or at end of acronym run (uppercase before uppercase+lowercase)
                if !prev_is_upper || next_is_lower {
                    words.push(std::mem::take(&mut current));
                }
            }
            current.push(ch.to_lowercase().next().unwrap_or(ch));
        } else {
            current.push(ch.to_lowercase().next().unwrap_or(ch));
        }
    }

    if !current.is_empty() {
        words.push(current);
    }

    words
}

/// Check whether a parameter name suggests it should use the given newtype.
/// Both names are split into words; returns true if the newtype's words
/// appear as a suffix of the parameter's words.
pub fn name_matches_newtype(param_name: &str, newtype_name: &str) -> bool {
    let param_words = split_identifier_words(param_name);
    let nt_words = split_identifier_words(newtype_name);

    if nt_words.is_empty() || param_words.len() < nt_words.len() {
        return false;
    }

    let offset = param_words.len() - nt_words.len();
    param_words[offset..] == nt_words[..]
}

/// Check a return type against the type map and emit a hit if the function name
/// suggests a newtype but the return type is a raw primitive.
pub fn check_return_type(
    return_type: &str,
    fn_name: &str,
    decl_line: usize,
    type_map: &HashMap<String, Vec<String>>,
    out: &mut Vec<SignatureHit>,
) {
    let cleaned = return_type
        .trim()
        .trim_start_matches('&')    // Rust references
        .trim_start_matches("mut ") // Rust mut
        .trim_end_matches('?')      // Kotlin/Swift nullable
        .trim();

    if let Some(newtypes) = type_map.get(cleaned) {
        for nt in newtypes {
            if name_matches_newtype(fn_name, nt) {
                out.push(SignatureHit {
                    line: decl_line + 1,
                    function_name: fn_name.to_string(),
                    param_name: "(return)".to_string(),
                    raw_type: cleaned.to_string(),
                    suggested: vec![nt.clone()],
                });
            }
        }
    }
}

/// Collect complete parameter text from a multi-line function declaration.
/// Finds the opening `(` on the start line, then collects through the closing `)`.
/// Returns (params_text, last_line_index) or None if no opening paren found.
pub fn collect_params_text(lines: &[&str], start_idx: usize) -> Option<(String, usize)> {
    let first = lines[start_idx];
    let paren_start = first.find('(')?;
    let after_paren = &first[paren_start + 1..];

    if let Some(paren_end) = after_paren.find(')') {
        return Some((after_paren[..paren_end].to_string(), start_idx));
    }

    let mut text = after_paren.trim().to_string();

    for (i, line) in lines
        .iter()
        .enumerate()
        .take(lines.len().min(start_idx + 21))
        .skip(start_idx + 1)
    {
        let continuation = line.trim();
        if let Some(paren_end) = continuation.find(')') {
            if !text.is_empty() {
                text.push_str(", ");
            }
            text.push_str(continuation[..paren_end].trim());
            return Some((text, i));
        }
        if !text.is_empty() && !continuation.is_empty() {
            text.push_str(", ");
        }
        text.push_str(continuation);
    }

    None
}

/// Check if a relative path is a test file for the given language.
pub fn is_test_file(rel_path: &str, lang: &LangDef) -> bool {
    for pattern in lang.test_file.path_contains {
        if rel_path.contains(pattern) {
            return true;
        }
    }
    let filename = rel_path.rsplit('/').next().unwrap_or(rel_path);
    for prefix in lang.test_file.filename_prefixes {
        if filename.starts_with(prefix) {
            return true;
        }
    }
    for suffix in lang.test_file.filename_suffixes {
        if filename.ends_with(suffix) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- split_identifier_words ---

    #[test]
    fn split_camel_case() {
        assert_eq!(split_identifier_words("userId"), vec!["user", "id"]);
    }

    #[test]
    fn split_snake_case() {
        assert_eq!(split_identifier_words("user_id"), vec!["user", "id"]);
    }

    #[test]
    fn split_pascal_case() {
        assert_eq!(split_identifier_words("UserId"), vec!["user", "id"]);
    }

    #[test]
    fn split_multi_word_camel() {
        assert_eq!(
            split_identifier_words("primaryUserId"),
            vec!["primary", "user", "id"]
        );
    }

    #[test]
    fn split_acronym() {
        assert_eq!(split_identifier_words("userID"), vec!["user", "id"]);
    }

    #[test]
    fn split_leading_acronym() {
        assert_eq!(split_identifier_words("HTTPClient"), vec!["http", "client"]);
    }

    #[test]
    fn split_single_word() {
        assert_eq!(split_identifier_words("valid"), vec!["valid"]);
    }

    #[test]
    fn split_single_short() {
        assert_eq!(split_identifier_words("id"), vec!["id"]);
    }

    #[test]
    fn split_mixed_underscore_camel() {
        assert_eq!(
            split_identifier_words("my_userId"),
            vec!["my", "user", "id"]
        );
    }

    // --- name_matches_newtype ---

    #[test]
    fn match_exact() {
        assert!(name_matches_newtype("user_id", "UserId"));
    }

    #[test]
    fn match_camel_case_param() {
        assert!(name_matches_newtype("userId", "UserId"));
    }

    #[test]
    fn match_prefixed() {
        assert!(name_matches_newtype("primary_user_id", "UserId"));
    }

    #[test]
    fn no_match_unrelated() {
        assert!(!name_matches_newtype("provider", "Id"));
    }

    #[test]
    fn no_match_valid_vs_id() {
        assert!(!name_matches_newtype("valid", "Id"));
    }

    #[test]
    fn no_match_grid_vs_id() {
        assert!(!name_matches_newtype("grid", "Id"));
    }

    #[test]
    fn match_suffix_short_newtype() {
        assert!(name_matches_newtype("order_id", "Id"));
    }

    #[test]
    fn no_match_documentation_vs_amount() {
        assert!(!name_matches_newtype("documentation", "Amount"));
    }

    #[test]
    fn match_total_amount() {
        assert!(name_matches_newtype("total_amount", "Amount"));
    }

    #[test]
    fn match_email_address() {
        assert!(name_matches_newtype("user_email_address", "EmailAddress"));
    }

    // --- collect_params_text ---

    #[test]
    fn collect_single_line() {
        let lines = vec!["def get_user(user_id: str) -> None:"];
        let (text, end) = collect_params_text(&lines, 0).unwrap();
        assert_eq!(text, "user_id: str");
        assert_eq!(end, 0);
    }

    #[test]
    fn collect_multi_line() {
        let lines = vec![
            "def get_user(",
            "    user_id: str,",
            "    name: str",
            ") -> None:",
        ];
        let (text, end) = collect_params_text(&lines, 0).unwrap();
        assert!(text.contains("user_id: str"));
        assert!(text.contains("name: str"));
        assert_eq!(end, 3);
    }

    #[test]
    fn collect_no_paren() {
        let lines = vec!["x = 1"];
        assert!(collect_params_text(&lines, 0).is_none());
    }
}
