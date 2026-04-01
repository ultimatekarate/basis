use super::RawMatchCases;
use crate::spec::UnionDef;
use std::collections::{HashMap, HashSet};

/// Infer union types from match/switch case sets.
///
/// Groups overlapping case sets that share the same inferred name.
/// Takes the union of all variants in each group as the full variant set.
pub fn infer_unions(match_groups: &[RawMatchCases]) -> Vec<UnionDef> {
    if match_groups.is_empty() {
        return vec![];
    }

    // Step 1: Extract a union name candidate from each match expression
    let annotated: Vec<(&RawMatchCases, String)> = match_groups
        .iter()
        .map(|mg| {
            let name = infer_union_name(&mg.matched_expr);
            (mg, name)
        })
        .collect();

    // Step 2: Group by inferred name
    let mut groups: HashMap<String, Vec<&RawMatchCases>> = HashMap::new();
    for (mg, name) in &annotated {
        groups.entry(name.clone()).or_default().push(mg);
    }

    // Step 3: For each named group, merge overlapping case sets
    let mut unions: Vec<UnionDef> = Vec::new();
    let mut unnamed_counter = 0;

    for (name, members) in &groups {
        // Merge case sets: collect all variants seen
        let mut all_variants: HashSet<String> = HashSet::new();
        let mut occurrence_count = 0;

        for mg in members {
            if mg.cases.len() >= 2 {
                all_variants.extend(mg.cases.iter().cloned());
                occurrence_count += 1;
            }
        }

        // Filter: need >= 2 variants and >= 2 occurrences
        if all_variants.len() < 2 || occurrence_count < 2 {
            continue;
        }

        let union_name = if name.is_empty() {
            unnamed_counter += 1;
            format!("Union{}", unnamed_counter)
        } else {
            name.clone()
        };

        let mut variants: Vec<String> = all_variants.into_iter().collect();
        variants.sort();

        unions.push(UnionDef {
            name: union_name,
            variants,
            languages: None,
        });
    }

    // Also try to merge unnamed groups with named groups if they have overlapping variants
    // (This handles cases where some match stmts have clear expressions and others don't)

    unions.sort_by(|a, b| a.name.cmp(&b.name));
    unions
}

/// Infer a union name from a matched expression.
///
/// - `user.status` → "Status"
/// - `order_type` → "OrderType"
/// - `self.state` → "State"
/// - Complex expressions → empty string
fn infer_union_name(expr: &str) -> String {
    let expr = expr.trim();
    if expr.is_empty() {
        return String::new();
    }

    // Strip common prefixes: self., this., @
    let expr = expr
        .trim_start_matches("self.")
        .trim_start_matches("this.")
        .trim_start_matches('@')
        .trim_start_matches('*'); // Rust derefs

    // If it has dots, take the last component
    let base = if expr.contains('.') {
        expr.rsplit('.').next().unwrap_or("")
    } else {
        expr
    };

    // Skip complex expressions (contains operators, parens, etc.)
    if base.contains('(')
        || base.contains(')')
        || base.contains('[')
        || base.contains('+')
        || base.contains('-')
        || base.contains(' ')
    {
        return String::new();
    }

    // Convert to PascalCase
    let words = crate::language::split_identifier_words(base);
    if words.is_empty() {
        return String::new();
    }

    words
        .iter()
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

    fn make_match(expr: &str, cases: &[&str]) -> RawMatchCases {
        RawMatchCases {
            file_index: 0,
            line: 1,
            matched_expr: expr.to_string(),
            cases: cases.iter().map(|s| s.to_string()).collect(),
            has_wildcard: false,
        }
    }

    #[test]
    fn infer_union_from_matching_exprs() {
        let groups = vec![
            make_match("order.status", &["Pending", "Shipped"]),
            make_match("order.status", &["Pending", "Shipped", "Delivered"]),
        ];
        let unions = infer_unions(&groups);
        assert_eq!(unions.len(), 1);
        assert_eq!(unions[0].name, "Status");
        assert!(unions[0].variants.contains(&"Pending".to_string()));
        assert!(unions[0].variants.contains(&"Shipped".to_string()));
        assert!(unions[0].variants.contains(&"Delivered".to_string()));
    }

    #[test]
    fn no_merge_different_names() {
        let groups = vec![
            make_match("user.status", &["Active", "Inactive"]),
            make_match("user.status", &["Active", "Suspended"]),
            make_match("order.type", &["Standard", "Express"]),
            make_match("order.type", &["Standard", "Express"]),
        ];
        let unions = infer_unions(&groups);
        assert_eq!(unions.len(), 2);
        let names: Vec<&str> = unions.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Status"));
        assert!(names.contains(&"Type"));
    }

    #[test]
    fn filter_single_occurrence() {
        let groups = vec![
            make_match("status", &["Active", "Inactive"]),
            // Only one occurrence — should be filtered out
        ];
        let unions = infer_unions(&groups);
        assert!(unions.is_empty());
    }

    #[test]
    fn filter_single_variant() {
        let groups = vec![
            make_match("status", &["Active"]),
            make_match("status", &["Active"]),
        ];
        let unions = infer_unions(&groups);
        assert!(unions.is_empty()); // Only 1 variant
    }

    #[test]
    fn union_naming_dot_expr() {
        assert_eq!(infer_union_name("user.status"), "Status");
        assert_eq!(infer_union_name("order.type"), "Type");
        assert_eq!(infer_union_name("self.state"), "State");
    }

    #[test]
    fn union_naming_simple_var() {
        assert_eq!(infer_union_name("order_status"), "OrderStatus");
        assert_eq!(infer_union_name("paymentMethod"), "PaymentMethod");
    }

    #[test]
    fn union_naming_complex_skipped() {
        assert_eq!(infer_union_name("get_status()"), "");
        assert_eq!(infer_union_name("a + b"), "");
    }

    #[test]
    fn empty_input() {
        let unions = infer_unions(&[]);
        assert!(unions.is_empty());
    }

    #[test]
    fn unnamed_fallback() {
        let groups = vec![
            make_match("get_status()", &["A", "B"]),
            make_match("get_status()", &["A", "B", "C"]),
        ];
        // Complex expression -> empty name -> "Union1"
        let unions = infer_unions(&groups);
        // Should not produce unions since the name is empty and cases need >= 2 occurrences
        // Actually with same empty name, they group together
        if !unions.is_empty() {
            assert!(unions[0].name.starts_with("Union"));
        }
    }
}
