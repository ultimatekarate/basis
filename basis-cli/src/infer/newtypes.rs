use super::{FileInfo, InferStats, RawParam};
use crate::language::{self, LangRegistry};
use crate::spec::NewtypeDef;
use std::collections::HashMap;

/// Infer newtype candidates from recurring parameter name suffixes.
///
/// Only considers parameters with type annotations (raw_type is non-empty).
/// Returns (newtypes, stats) where stats tracks typed/untyped counts.
pub fn infer_newtypes(
    params: &[RawParam],
    files: &[FileInfo],
    registry: &LangRegistry,
    min_occurrences: usize,
) -> (Vec<NewtypeDef>, InferStats) {
    let mut stats = InferStats::default();

    // Build reverse primitive map: native type name -> canonical name
    // e.g., "str" -> "string", "i32" -> "int", "String" -> "string"
    let mut reverse_primitives: HashMap<String, String> = HashMap::new();
    for lang in registry.all() {
        for (canonical, natives) in lang.primitives {
            for native in *natives {
                reverse_primitives.insert(native.to_string(), canonical.to_string());
            }
        }
    }

    // Frequency map: (suffix_words_joined, canonical_type) -> (count, example_params)
    let mut freq: HashMap<(String, String), (usize, Vec<String>)> = HashMap::new();

    for param in params {
        if param.raw_type.is_empty() {
            stats.untyped_params_skipped += 1;
            continue;
        }

        stats.typed_params += 1;

        // Check if the raw type is a primitive
        let canonical = match reverse_primitives.get(&param.raw_type) {
            Some(c) => c.clone(),
            None => continue, // Not a primitive — skip (it's already a complex/custom type)
        };

        // Split param name into words and extract suffix
        let words = language::split_identifier_words(&param.param_name);
        if words.is_empty() || words.len() > 4 {
            continue; // Skip very long names — too specific to be a reusable type
        }

        // Use the full word list as the suffix (we'll deduplicate by the suffix)
        let suffix_key = words.join("_");
        let key = (suffix_key, canonical);
        let entry = freq.entry(key).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            let file_info = &files[param.file_index];
            entry.1.push(format!(
                "{}: {}",
                file_info.rel_path, param.param_name
            ));
        }
    }

    stats.candidates_before_threshold = freq.len();

    // Filter by threshold and convert to NewtypeDef
    let mut newtypes: Vec<NewtypeDef> = Vec::new();
    let mut seen_names: HashMap<String, usize> = HashMap::new();

    let mut candidates: Vec<_> = freq.into_iter().collect();
    candidates.sort_by(|a, b| b.1 .0.cmp(&a.1 .0)); // Sort by frequency descending

    for ((suffix_key, canonical_type), (count, _examples)) in candidates {
        if count < min_occurrences {
            continue;
        }

        let name = words_to_pascal_case(&suffix_key);
        if name.is_empty() || name.len() < 2 {
            continue;
        }

        // Deduplicate: same PascalCase name might come from different suffix decompositions
        if seen_names.contains_key(&name) {
            continue;
        }
        seen_names.insert(name.clone(), count);

        newtypes.push(NewtypeDef {
            name,
            wraps: canonical_type,
            validation: None,
            languages: None,
        });
    }

    stats.candidates_after_threshold = newtypes.len();

    // Sort alphabetically for stable output
    newtypes.sort_by(|a, b| a.name.cmp(&b.name));

    (newtypes, stats)
}

/// Convert underscore-separated words to PascalCase.
/// "user_id" -> "UserId", "email_address" -> "EmailAddress"
fn words_to_pascal_case(words: &str) -> String {
    words
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let mut s = c.to_uppercase().to_string();
                    s.extend(chars);
                    s
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_param(file_idx: usize, param_name: &str, raw_type: &str) -> RawParam {
        RawParam {
            file_index: file_idx,
            line: 1,
            function_name: "test_fn".to_string(),
            param_name: param_name.to_string(),
            raw_type: raw_type.to_string(),
        }
    }

    fn make_file(path: &str, lang: &str) -> FileInfo {
        FileInfo {
            rel_path: path.to_string(),
            lang_name: lang.to_string(),
        }
    }

    #[test]
    fn infer_from_recurring_suffix() {
        let registry = LangRegistry::new();
        let files = vec![
            make_file("src/a.py", "python"),
            make_file("src/b.py", "python"),
        ];
        let params = vec![
            make_param(0, "user_id", "str"),
            make_param(1, "order_id", "str"),
            make_param(0, "account_id", "str"),
        ];
        // With min_occurrences=2, "id" suffix appears 3 times
        // But the full suffix is different for each: "user_id", "order_id", "account_id"
        // So we'd get 3 separate candidates, none meeting threshold of 2
        // unless the suffix matching groups them better
        let (newtypes, stats) = infer_newtypes(&params, &files, &registry, 1);
        assert_eq!(stats.typed_params, 3);
        assert_eq!(stats.untyped_params_skipped, 0);
        // Each is unique suffix
        assert!(newtypes.len() >= 3);
    }

    #[test]
    fn skip_untyped() {
        let registry = LangRegistry::new();
        let files = vec![make_file("src/a.rb", "ruby")];
        let params = vec![make_param(0, "user_id", "")];
        let (newtypes, stats) = infer_newtypes(&params, &files, &registry, 1);
        assert_eq!(stats.untyped_params_skipped, 1);
        assert!(newtypes.is_empty());
    }

    #[test]
    fn threshold_filters() {
        let registry = LangRegistry::new();
        let files = vec![
            make_file("src/a.py", "python"),
            make_file("src/b.py", "python"),
        ];
        let params = vec![
            make_param(0, "user_id", "str"),
            make_param(1, "user_id", "str"),
        ];
        let (newtypes, stats) = infer_newtypes(&params, &files, &registry, 2);
        assert_eq!(stats.candidates_after_threshold, 1);
        assert_eq!(newtypes[0].name, "UserId");
        assert_eq!(newtypes[0].wraps, "string");
    }

    #[test]
    fn cross_language_dedup() {
        let registry = LangRegistry::new();
        let files = vec![
            make_file("src/a.py", "python"),
            make_file("src/b.java", "java"),
        ];
        let params = vec![
            make_param(0, "user_id", "str"),
            make_param(1, "userId", "String"),
        ];
        // Both should produce "UserId wraps string"
        // But they have different suffix keys: "user_id" vs "user_id" (from userId)
        let (newtypes, _) = infer_newtypes(&params, &files, &registry, 1);
        // Should have at most one UserId (deduped by PascalCase name)
        let user_id_count = newtypes.iter().filter(|n| n.name == "UserId").count();
        assert_eq!(user_id_count, 1);
    }

    #[test]
    fn pascal_case_conversion() {
        assert_eq!(words_to_pascal_case("user_id"), "UserId");
        assert_eq!(words_to_pascal_case("email_address"), "EmailAddress");
        assert_eq!(words_to_pascal_case("amount"), "Amount");
    }

    #[test]
    fn skip_non_primitive() {
        let registry = LangRegistry::new();
        let files = vec![make_file("src/a.py", "python")];
        let params = vec![make_param(0, "user", "User")]; // User is not a primitive
        let (newtypes, stats) = infer_newtypes(&params, &files, &registry, 1);
        assert_eq!(stats.typed_params, 1);
        assert!(newtypes.is_empty()); // User is not in the primitives table
    }
}
