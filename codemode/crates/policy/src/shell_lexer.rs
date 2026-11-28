//! Clean-room shell lexer and recursive wrapper unwrapper (`env`, `bash -c`, pipes, subshells).

/// A parsed simple command with its executable name and arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Unwraps `command` into all underlying simple commands across pipes, chains, subshells, and `-c` wrappers.
pub fn unwrap_commands(command: &str) -> Vec<SimpleCommand> {
    let mut out = Vec::new();
    collect_commands(command, 0, &mut out);
    out
}

fn collect_commands(input: &str, depth: usize, out: &mut Vec<SimpleCommand>) {
    if depth > 8 {
        return;
    }
    let (segments, subshells) = split_pipeline_and_subshells(input);
    for sub in subshells {
        collect_commands(&sub, depth + 1, out);
    }
    for tokens in segments {
        unwrap_tokens(&tokens, depth, out);
    }
}

fn unwrap_tokens(tokens: &[String], depth: usize, out: &mut Vec<SimpleCommand>) {
    if tokens.is_empty() || depth > 8 {
        return;
    }
    let mut idx = 0usize;
    // Skip leading environment assignments (e.g. FOO=1 bar).
    while idx < tokens.len() && is_env_assignment(&tokens[idx]) {
        idx += 1;
    }
    if idx >= tokens.len() {
        return;
    }

    let base = basename(&tokens[idx]);
    match base.as_str() {
        "env" => {
            idx += 1;
            while idx < tokens.len() {
                let tok = &tokens[idx];
                if tok == "--" {
                    idx += 1;
                    break;
                }
                if tok == "-u" || tok == "-C" || tok == "--unset" || tok == "--chdir" {
                    idx += 2;
                    continue;
                }
                if tok.starts_with('-') || is_env_assignment(tok) {
                    idx += 1;
                    continue;
                }
                break;
            }
            unwrap_tokens(&tokens[idx..], depth + 1, out);
        }
        "command" | "nohup" | "time" => {
            idx += 1;
            while idx < tokens.len() && tokens[idx].starts_with('-') {
                idx += 1;
            }
            unwrap_tokens(&tokens[idx..], depth + 1, out);
        }
        "nice" => {
            idx += 1;
            while idx < tokens.len() {
                if tokens[idx] == "-n" {
                    idx += 2;
                } else if tokens[idx].starts_with('-') {
                    idx += 1;
                } else {
                    break;
                }
            }
            unwrap_tokens(&tokens[idx..], depth + 1, out);
        }
        "timeout" => {
            idx += 1;
            while idx < tokens.len() && tokens[idx].starts_with('-') {
                idx += 1;
            }
            if idx < tokens.len() {
                idx += 1; // duration
            }
            unwrap_tokens(&tokens[idx..], depth + 1, out);
        }
        "sh" | "bash" | "zsh" | "dash" | "ksh" | "ash" | "fish" => {
            let mut i = idx + 1;
            let mut inline_script: Option<&str> = None;
            while i < tokens.len() {
                let flag = tokens[i].as_str();
                if (flag == "-c" || flag == "-lc" || flag == "-ec" || flag == "-xc")
                    && i + 1 < tokens.len()
                {
                    inline_script = Some(&tokens[i + 1]);
                    break;
                }
                i += 1;
            }
            if let Some(script) = inline_script {
                collect_commands(script, depth + 1, out);
            } else {
                out.push(SimpleCommand {
                    program: base,
                    args: tokens[idx + 1..].to_vec(),
                });
            }
        }
        "pwsh" | "powershell" => {
            let mut i = idx + 1;
            let mut inline_script: Option<&str> = None;
            while i < tokens.len() {
                let lower = tokens[i].to_ascii_lowercase();
                if (lower == "-c" || lower == "-command") && i + 1 < tokens.len() {
                    inline_script = Some(&tokens[i + 1]);
                    break;
                }
                i += 1;
            }
            if let Some(script) = inline_script {
                collect_commands(script, depth + 1, out);
            } else {
                out.push(SimpleCommand {
                    program: base,
                    args: tokens[idx + 1..].to_vec(),
                });
            }
        }
        _ => {
            out.push(SimpleCommand {
                program: base,
                args: tokens[idx + 1..].to_vec(),
            });
        }
    }
}

fn is_env_assignment(tok: &str) -> bool {
    if let Some((name, _)) = tok.split_once('=') {
        !name.is_empty()
            && !name.starts_with('-')
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    } else {
        false
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn split_pipeline_and_subshells(input: &str) -> (Vec<Vec<String>>, Vec<String>) {
    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut current_seg: Vec<String> = Vec::new();
    let mut current_tok = String::new();
    let mut subshells: Vec<String> = Vec::new();

    let chars: Vec<char> = input.chars().collect();
    let mut i = 0usize;
    let mut in_single = false;
    let mut in_double = false;

    while i < chars.len() {
        let ch = chars[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                current_tok.push(ch);
            }
            i += 1;
            continue;
        }

        if ch == '\\' && i + 1 < chars.len() {
            current_tok.push(chars[i + 1]);
            i += 2;
            continue;
        }

        if ch == '$' && i + 1 < chars.len() && chars[i + 1] == '(' {
            let (inner, next_i) = extract_balanced_parens(&chars, i + 2);
            subshells.push(inner);
            i = next_i;
            continue;
        }

        if ch == '`' {
            let mut inner = String::new();
            i += 1;
            while i < chars.len() && chars[i] != '`' {
                inner.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            subshells.push(inner);
            continue;
        }

        if in_double {
            if ch == '"' {
                in_double = false;
            } else {
                current_tok.push(ch);
            }
            i += 1;
            continue;
        }

        match ch {
            '\'' => {
                in_single = true;
                i += 1;
            }
            '"' => {
                in_double = true;
                i += 1;
            }
            '(' => {
                let (inner, next_i) = extract_balanced_parens(&chars, i + 1);
                subshells.push(inner);
                i = next_i;
            }
            '|' | '&' | ';' | '\n' => {
                if !current_tok.is_empty() {
                    current_seg.push(std::mem::take(&mut current_tok));
                }
                if !current_seg.is_empty() {
                    segments.push(std::mem::take(&mut current_seg));
                }
                if (ch == '|' || ch == '&') && i + 1 < chars.len() && chars[i + 1] == ch {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            c if c.is_whitespace() => {
                if !current_tok.is_empty() {
                    current_seg.push(std::mem::take(&mut current_tok));
                }
                i += 1;
            }
            other => {
                current_tok.push(other);
                i += 1;
            }
        }
    }

    if !current_tok.is_empty() {
        current_seg.push(current_tok);
    }
    if !current_seg.is_empty() {
        segments.push(current_seg);
    }

    (segments, subshells)
}

fn extract_balanced_parens(chars: &[char], mut i: usize) -> (String, usize) {
    let mut out = String::new();
    let mut depth = 1usize;
    let mut in_single = false;
    let mut in_double = false;
    while i < chars.len() && depth > 0 {
        let ch = chars[i];
        if in_single {
            if ch == '\'' {
                in_single = false;
            }
            out.push(ch);
            i += 1;
            continue;
        }
        if ch == '\\' && i + 1 < chars.len() {
            out.push(ch);
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if in_double {
            if ch == '"' {
                in_double = false;
            }
            out.push(ch);
            i += 1;
            continue;
        }
        match ch {
            '\'' => in_single = true,
            '"' => in_double = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    i += 1;
                    break;
                }
            }
            _ => {}
        }
        out.push(ch);
        i += 1;
    }
    (out, i)
}
