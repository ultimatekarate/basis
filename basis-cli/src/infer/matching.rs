use super::RawMatchCases;
use std::collections::HashSet;

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
