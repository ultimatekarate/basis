use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

// ── Core types ─────────────────────────────────────────────────────

/// Structured output from `basis check`. Serializable to JSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub version: String,
    pub timestamp: String,
    pub spec_path: String,
    pub target_path: String,
    pub axes_checked: Vec<String>,
    pub summary: Summary,
    pub violations: Vec<UnifiedViolation>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub total: usize,
    pub by_axis: HashMap<String, usize>,
}

/// A single violation in unified format, usable across all axes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedViolation {
    pub code: String,
    pub axis: String,
    pub file: String,
    pub line: usize,
    pub message: String,
    pub identity: String,
    pub details: ViolationDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ViolationDetails {
    Placement {
        from_layer: String,
        to_layer: String,
        module: String,
    },
    Values {
        param_name: String,
        raw_type: String,
        suggested: Vec<String>,
        function_name: String,
    },
    Completeness {
        union_name: String,
        missing_variants: Vec<String>,
    },
    Purity {
        category: String,
        trigger: String,
        layer: String,
    },
    Contracts {
        function_name: String,
        missing_param: String,
        layer: String,
    },
}

// ── Constructors ───────────────────────────────────────────────────

impl CheckResult {
    pub fn new(
        spec_path: &str,
        target_path: &str,
        axes_checked: Vec<String>,
        violations: Vec<UnifiedViolation>,
    ) -> Self {
        let mut by_axis: HashMap<String, usize> = HashMap::new();
        for v in &violations {
            *by_axis.entry(v.axis.clone()).or_insert(0) += 1;
        }
        let total = violations.len();

        CheckResult {
            version: "1.0".to_string(),
            timestamp: iso8601_now(),
            spec_path: spec_path.to_string(),
            target_path: target_path.to_string(),
            axes_checked,
            summary: Summary { total, by_axis },
            violations,
        }
    }
}

// ── Identity keys ──────────────────────────────────────────────────

/// Stable identity key for a violation. Excludes line numbers so that
/// refactoring (which shifts lines) doesn't cause false regressions.
pub fn identity_key(v: &UnifiedViolation) -> String {
    match &v.details {
        ViolationDetails::Placement {
            from_layer,
            to_layer,
            module,
        } => format!("B001:{}:{}:{}:{}", v.file, from_layer, to_layer, module),
        ViolationDetails::Values {
            function_name,
            param_name,
            raw_type,
            ..
        } => format!(
            "B002:{}:{}:{}:{}",
            v.file, function_name, param_name, raw_type
        ),
        ViolationDetails::Completeness {
            union_name,
            missing_variants,
        } => {
            let mut sorted = missing_variants.clone();
            sorted.sort();
            format!("B003:{}:{}:{}", v.file, union_name, sorted.join(","))
        }
        ViolationDetails::Purity {
            category, trigger, ..
        } => format!("B004:{}:{}:{}", v.file, category, trigger),
        ViolationDetails::Contracts {
            function_name,
            missing_param,
            ..
        } => format!("B005:{}:{}:{}", v.file, function_name, missing_param),
    }
}

// ── Conversions from axis-specific violations ──────────────────────

impl From<&super::placement::Violation> for UnifiedViolation {
    fn from(v: &super::placement::Violation) -> Self {
        let details = ViolationDetails::Placement {
            from_layer: v.from_layer.clone(),
            to_layer: v.to_layer.clone(),
            module: v.import.clone(),
        };
        let mut uv = UnifiedViolation {
            code: "B001".to_string(),
            axis: "placement".to_string(),
            file: v.file.clone(),
            line: v.line,
            message: format!(
                "import '{}' violates boundary ({} -> {})",
                v.import, v.from_layer, v.to_layer
            ),
            identity: String::new(),
            details,
        };
        uv.identity = identity_key(&uv);
        uv
    }
}

impl From<&super::values::Violation> for UnifiedViolation {
    fn from(v: &super::values::Violation) -> Self {
        let message = if v.param_name == "(return)" {
            format!(
                "return type '{}' is a raw primitive where branded newtype expected",
                v.raw_type
            )
        } else {
            format!(
                "parameter '{}' uses raw '{}' instead of branded newtype",
                v.param_name, v.raw_type
            )
        };
        let details = ViolationDetails::Values {
            param_name: v.param_name.clone(),
            raw_type: v.raw_type.clone(),
            suggested: v.suggested.clone(),
            function_name: v.function_name.clone(),
        };
        let mut uv = UnifiedViolation {
            code: "B002".to_string(),
            axis: "values".to_string(),
            file: v.file.clone(),
            line: v.line,
            message,
            identity: String::new(),
            details,
        };
        uv.identity = identity_key(&uv);
        uv
    }
}

impl From<&super::completeness::Violation> for UnifiedViolation {
    fn from(v: &super::completeness::Violation) -> Self {
        let details = ViolationDetails::Completeness {
            union_name: v.union_name.clone(),
            missing_variants: v.missing_variants.clone(),
        };
        let mut uv = UnifiedViolation {
            code: "B003".to_string(),
            axis: "completeness".to_string(),
            file: v.file.clone(),
            line: v.line,
            message: format!(
                "match on '{}' is not exhaustive, missing: {}",
                v.union_name,
                v.missing_variants.join(", ")
            ),
            identity: String::new(),
            details,
        };
        uv.identity = identity_key(&uv);
        uv
    }
}

impl From<&super::purity::Violation> for UnifiedViolation {
    fn from(v: &super::purity::Violation) -> Self {
        let details = ViolationDetails::Purity {
            category: v.category.clone(),
            trigger: v.forbidden.clone(),
            layer: v.layer.clone(),
        };
        let mut uv = UnifiedViolation {
            code: "B004".to_string(),
            axis: "purity".to_string(),
            file: v.file.clone(),
            line: v.line,
            message: format!(
                "forbidden import '{}' in strict-purity layer '{}'",
                v.forbidden, v.layer
            ),
            identity: String::new(),
            details,
        };
        uv.identity = identity_key(&uv);
        uv
    }
}

impl From<&super::contracts::Violation> for UnifiedViolation {
    fn from(v: &super::contracts::Violation) -> Self {
        let details = ViolationDetails::Contracts {
            function_name: v.function_name.clone(),
            missing_param: v.missing_param.clone(),
            layer: v.layer.clone(),
        };
        let mut uv = UnifiedViolation {
            code: "B005".to_string(),
            axis: "contracts".to_string(),
            file: v.file.clone(),
            line: v.line,
            message: format!(
                "function '{}' has no precondition referencing parameter '{}'",
                v.function_name, v.missing_param
            ),
            identity: String::new(),
            details,
        };
        uv.identity = identity_key(&uv);
        uv
    }
}

// ── Trend diffing ──────────────────────────────────────────────────

/// Result of comparing current violations against a baseline.
#[derive(Debug, Serialize, Deserialize)]
pub struct TrendResult {
    pub baseline_path: String,
    pub improved: Vec<UnifiedViolation>,
    pub regressed: Vec<UnifiedViolation>,
    pub unchanged: Vec<UnifiedViolation>,
    pub axes_mismatch: Option<AxesMismatch>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AxesMismatch {
    pub baseline_axes: Vec<String>,
    pub current_axes: Vec<String>,
}

impl TrendResult {
    pub fn net_change(&self) -> i64 {
        self.regressed.len() as i64 - self.improved.len() as i64
    }
}

/// Diff two CheckResults by identity key.
pub fn diff(baseline: &CheckResult, current: &CheckResult) -> TrendResult {
    let baseline_ids: HashSet<String> = baseline
        .violations
        .iter()
        .map(|v| v.identity.clone())
        .collect();
    let current_ids: HashSet<String> = current
        .violations
        .iter()
        .map(|v| v.identity.clone())
        .collect();

    let improved: Vec<UnifiedViolation> = baseline
        .violations
        .iter()
        .filter(|v| !current_ids.contains(&v.identity))
        .cloned()
        .collect();

    let regressed: Vec<UnifiedViolation> = current
        .violations
        .iter()
        .filter(|v| !baseline_ids.contains(&v.identity))
        .cloned()
        .collect();

    let unchanged: Vec<UnifiedViolation> = current
        .violations
        .iter()
        .filter(|v| baseline_ids.contains(&v.identity))
        .cloned()
        .collect();

    // Check axes mismatch
    let mut baseline_axes = baseline.axes_checked.clone();
    let mut current_axes = current.axes_checked.clone();
    baseline_axes.sort();
    current_axes.sort();
    let axes_mismatch = if baseline_axes != current_axes {
        Some(AxesMismatch {
            baseline_axes,
            current_axes,
        })
    } else {
        None
    };

    TrendResult {
        baseline_path: String::new(),
        improved,
        regressed,
        unchanged,
        axes_mismatch,
    }
}

/// Render a TrendResult as human-readable text.
pub fn render_trend_text(result: &TrendResult) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Basis Trend: comparing against {}\n\n",
        result.baseline_path
    ));

    if let Some(mismatch) = &result.axes_mismatch {
        out.push_str(&format!(
            "  WARNING: axes mismatch — baseline checked [{}], current checked [{}]\n\n",
            mismatch.baseline_axes.join(", "),
            mismatch.current_axes.join(", ")
        ));
    }

    // Improved
    if result.improved.is_empty() {
        out.push_str("  Improved:  0 violations\n");
    } else {
        let breakdown = axis_breakdown(&result.improved);
        out.push_str(&format!(
            "  Improved:  -{} violations ({})\n",
            result.improved.len(),
            breakdown
        ));
    }

    // Regressed
    if result.regressed.is_empty() {
        out.push_str("  Regressed: 0 violations\n");
    } else {
        let breakdown = axis_breakdown(&result.regressed);
        out.push_str(&format!(
            "  Regressed: +{} violations ({})\n",
            result.regressed.len(),
            breakdown
        ));
    }

    // Unchanged
    out.push_str(&format!(
        "  Unchanged: {} violations\n",
        result.unchanged.len()
    ));

    // Net change
    let net = result.net_change();
    let sign = if net > 0 { "+" } else { "" };
    out.push_str(&format!("\n  Net change: {sign}{net} violations\n"));

    out
}

fn axis_breakdown(violations: &[UnifiedViolation]) -> String {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for v in violations {
        *counts.entry(v.axis.as_str()).or_insert(0) += 1;
    }
    let mut pairs: Vec<_> = counts.into_iter().collect();
    pairs.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    pairs
        .iter()
        .map(|(axis, count)| format!("{count} {axis}"))
        .collect::<Vec<_>>()
        .join(", ")
}

// ── ISO 8601 timestamp ─────────────────────────────────────────────

fn iso8601_now() -> String {
    let duration = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    // Convert to date/time components
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Compute year/month/day from days since epoch (1970-01-01)
    let (year, month, day) = days_to_ymd(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z"
    )
}

fn days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    days += 719468;
    let era = days / 146097;
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_placement_violation(file: &str, import: &str, from: &str, to: &str) -> UnifiedViolation {
        let v = super::super::placement::Violation {
            file: file.to_string(),
            line: 5,
            import: import.to_string(),
            from_layer: from.to_string(),
            to_layer: to.to_string(),
            reason: "test".to_string(),
        };
        UnifiedViolation::from(&v)
    }

    fn make_values_violation(file: &str, fn_name: &str, param: &str, raw: &str) -> UnifiedViolation {
        let v = super::super::values::Violation {
            file: file.to_string(),
            line: 10,
            function_name: fn_name.to_string(),
            param_name: param.to_string(),
            raw_type: raw.to_string(),
            suggested: vec!["UserId".to_string()],
        };
        UnifiedViolation::from(&v)
    }

    fn make_completeness_violation(file: &str, union_name: &str, missing: &[&str]) -> UnifiedViolation {
        let v = super::super::completeness::Violation {
            file: file.to_string(),
            line: 15,
            union_name: union_name.to_string(),
            missing_variants: missing.iter().map(|s| s.to_string()).collect(),
        };
        UnifiedViolation::from(&v)
    }

    fn make_purity_violation(file: &str, forbidden: &str, category: &str, layer: &str) -> UnifiedViolation {
        let v = super::super::purity::Violation {
            file: file.to_string(),
            line: 20,
            layer: layer.to_string(),
            forbidden: forbidden.to_string(),
            category: category.to_string(),
        };
        UnifiedViolation::from(&v)
    }

    // ── Identity key tests ───────────────────────────────────

    #[test]
    fn identity_key_placement() {
        let uv = make_placement_violation("src/models/types.py", "src/logic", "domain", "logic");
        assert_eq!(uv.identity, "B001:src/models/types.py:domain:logic:src/logic");
    }

    #[test]
    fn identity_key_values() {
        let uv = make_values_violation("src/api/handler.py", "get_user", "user_id", "str");
        assert_eq!(uv.identity, "B002:src/api/handler.py:get_user:user_id:str");
    }

    #[test]
    fn identity_key_completeness_sorted() {
        let uv = make_completeness_violation("src/logic/handler.py", "OrderStatus", &["Shipped", "Cancelled"]);
        // Variants should be sorted in identity
        assert_eq!(
            uv.identity,
            "B003:src/logic/handler.py:OrderStatus:Cancelled,Shipped"
        );
    }

    #[test]
    fn identity_key_purity() {
        let uv = make_purity_violation("src/logic/compute.py", "requests", "network_io", "lab");
        assert_eq!(
            uv.identity,
            "B004:src/logic/compute.py:network_io:requests"
        );
    }

    #[test]
    fn identity_excludes_line_number() {
        let v1 = super::super::placement::Violation {
            file: "f.py".to_string(),
            line: 5,
            import: "mod".to_string(),
            from_layer: "a".to_string(),
            to_layer: "b".to_string(),
            reason: "test".to_string(),
        };
        let v2 = super::super::placement::Violation {
            file: "f.py".to_string(),
            line: 99,
            import: "mod".to_string(),
            from_layer: "a".to_string(),
            to_layer: "b".to_string(),
            reason: "test".to_string(),
        };
        let uv1 = UnifiedViolation::from(&v1);
        let uv2 = UnifiedViolation::from(&v2);
        assert_eq!(uv1.identity, uv2.identity);
    }

    // ── CheckResult construction ─────────────────────────────

    #[test]
    fn check_result_summary() {
        let violations = vec![
            make_placement_violation("a.py", "mod", "x", "y"),
            make_placement_violation("b.py", "mod2", "x", "z"),
            make_values_violation("c.py", "foo", "bar", "str"),
        ];
        let result = CheckResult::new("basis.yaml", ".", vec!["placement".into(), "values".into()], violations);
        assert_eq!(result.summary.total, 3);
        assert_eq!(result.summary.by_axis["placement"], 2);
        assert_eq!(result.summary.by_axis["values"], 1);
        assert_eq!(result.axes_checked.len(), 2);
    }

    // ── Diff tests ───────────────────────────────────────────

    #[test]
    fn diff_identifies_improved_regressed_unchanged() {
        let baseline = CheckResult::new(
            "basis.yaml",
            ".",
            vec!["placement".into()],
            vec![
                make_placement_violation("a.py", "mod1", "x", "y"),
                make_placement_violation("b.py", "mod2", "x", "z"),
            ],
        );
        let current = CheckResult::new(
            "basis.yaml",
            ".",
            vec!["placement".into()],
            vec![
                make_placement_violation("b.py", "mod2", "x", "z"), // unchanged
                make_placement_violation("c.py", "mod3", "x", "w"), // regressed
            ],
        );
        let trend = diff(&baseline, &current);
        assert_eq!(trend.improved.len(), 1); // a.py removed
        assert_eq!(trend.regressed.len(), 1); // c.py added
        assert_eq!(trend.unchanged.len(), 1); // b.py still there
        assert!(trend.axes_mismatch.is_none());
    }

    #[test]
    fn diff_detects_axes_mismatch() {
        let baseline = CheckResult::new(
            "basis.yaml",
            ".",
            vec!["placement".into()],
            vec![],
        );
        let current = CheckResult::new(
            "basis.yaml",
            ".",
            vec!["placement".into(), "values".into()],
            vec![],
        );
        let trend = diff(&baseline, &current);
        assert!(trend.axes_mismatch.is_some());
    }

    #[test]
    fn diff_no_axes_mismatch_when_same() {
        let baseline = CheckResult::new("basis.yaml", ".", vec!["values".into()], vec![]);
        let current = CheckResult::new("basis.yaml", ".", vec!["values".into()], vec![]);
        let trend = diff(&baseline, &current);
        assert!(trend.axes_mismatch.is_none());
    }

    #[test]
    fn net_change_negative_is_improvement() {
        let baseline = CheckResult::new(
            "basis.yaml",
            ".",
            vec!["placement".into()],
            vec![
                make_placement_violation("a.py", "m1", "x", "y"),
                make_placement_violation("b.py", "m2", "x", "y"),
                make_placement_violation("c.py", "m3", "x", "y"),
            ],
        );
        let current = CheckResult::new(
            "basis.yaml",
            ".",
            vec!["placement".into()],
            vec![make_placement_violation("a.py", "m1", "x", "y")],
        );
        let trend = diff(&baseline, &current);
        assert_eq!(trend.net_change(), -2);
    }

    // ── Render tests ─────────────────────────────────────────

    #[test]
    fn render_trend_text_output() {
        let trend = TrendResult {
            baseline_path: ".basis-baseline.json".to_string(),
            improved: vec![make_placement_violation("a.py", "m", "x", "y")],
            regressed: vec![make_values_violation("b.py", "f", "p", "str")],
            unchanged: vec![make_purity_violation("c.py", "req", "net", "lab")],
            axes_mismatch: None,
        };
        let text = render_trend_text(&trend);
        assert!(text.contains("Improved:  -1 violations"));
        assert!(text.contains("Regressed: +1 violations"));
        assert!(text.contains("Unchanged: 1 violations"));
        assert!(text.contains("Net change: 0 violations"));
    }

    #[test]
    fn render_trend_with_axes_mismatch() {
        let trend = TrendResult {
            baseline_path: ".basis-baseline.json".to_string(),
            improved: vec![],
            regressed: vec![],
            unchanged: vec![],
            axes_mismatch: Some(AxesMismatch {
                baseline_axes: vec!["values".into()],
                current_axes: vec!["values".into(), "purity".into()],
            }),
        };
        let text = render_trend_text(&trend);
        assert!(text.contains("WARNING: axes mismatch"));
    }

    // ── JSON round-trip ──────────────────────────────────────

    #[test]
    fn check_result_json_round_trip() {
        let violations = vec![
            make_placement_violation("a.py", "mod", "x", "y"),
            make_values_violation("b.py", "fn", "p", "str"),
            make_completeness_violation("c.py", "Status", &["A", "B"]),
            make_purity_violation("d.py", "req", "net", "lab"),
        ];
        let result = CheckResult::new("basis.yaml", ".", vec!["placement".into(), "values".into(), "completeness".into(), "purity".into()], violations);
        let json = serde_json::to_string_pretty(&result).unwrap();
        let parsed: CheckResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.summary.total, 4);
        assert_eq!(parsed.violations.len(), 4);
        assert_eq!(parsed.violations[0].code, "B001");
        assert_eq!(parsed.violations[1].code, "B002");
        assert_eq!(parsed.violations[2].code, "B003");
        assert_eq!(parsed.violations[3].code, "B004");
    }

    // ── ISO 8601 helper ──────────────────────────────────────

    #[test]
    fn iso8601_produces_valid_format() {
        let ts = iso8601_now();
        assert!(ts.ends_with('Z'));
        assert_eq!(ts.len(), 20); // "2026-04-01T12:34:56Z"
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[7..8], "-");
        assert_eq!(&ts[10..11], "T");
        assert_eq!(&ts[13..14], ":");
        assert_eq!(&ts[16..17], ":");
    }

    #[test]
    fn days_to_ymd_epoch() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
    }

    #[test]
    fn days_to_ymd_known_date() {
        // 2024-01-01 is 19723 days after epoch
        let (y, m, d) = days_to_ymd(19723);
        assert_eq!((y, m, d), (2024, 1, 1));
    }
}
