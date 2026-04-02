use super::RawParam;
use crate::language;

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
pub(super) fn extract_js_fn_name(line: &str) -> String {
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
