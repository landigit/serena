//! Diagnostic severity grouping (ported from `diagnostics-group-rs`) and ignore-aware project discovery.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use codemode_core::config::ProjectConfig;
use codemode_core::languages::{LanguageId, resolve_language};
use codemode_ts::{Symbol, flatten};
use serde_json::{Map, Value, json};

use crate::client::Diagnostic;

const DEFAULT_MAX_PROJECT_FILES: usize = 500;

/// Maps an LSP numeric diagnostic severity to Serena's display name.
pub fn severity_name(severity: Option<i64>) -> &'static str {
    match severity {
        None => "Unknown",
        Some(1) => "Error",
        Some(2) => "Warning",
        Some(3) => "Information",
        Some(4) => "Hint",
        Some(_) => "Unknown",
    }
}

/// Maps an LSP numeric diagnostic severity to a custom string (matching `Severity_<n>`).
pub fn severity_label(severity: Option<i64>) -> String {
    match severity {
        None => "Unknown".into(),
        Some(1) => "Error".into(),
        Some(2) => "Warning".into(),
        Some(3) => "Information".into(),
        Some(4) => "Hint".into(),
        Some(n) => format!("Severity_{n}"),
    }
}

/// Normalizes one diagnostic JSON object for grouped output.
pub fn diagnostic_output(d: &Value) -> Result<Value, String> {
    if !d.is_object() {
        return Err("diagnostic entry must be a JSON object".to_string());
    }
    let message = d
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let range = d.get("range").cloned().unwrap_or_else(|| json!({}));
    let mut out = json!({ "message": message, "range": range });
    for key in ["code", "source"] {
        if let Some(v) = d.get(key) {
            out[key] = v.clone();
        }
    }
    Ok(out)
}

/// Groups `{name_path, diagnostic}` entries under `{relative_path: {Severity: {name_path: [...]}}}`.
pub fn group_entries(relative_path: &str, entries: &[Value]) -> Result<Value, String> {
    let mut file_map: Map<String, Value> = Map::new();
    for entry in entries {
        if !entry.is_object() {
            return Err("entry must be a JSON object".to_string());
        }
        let name_path = entry
            .get("name_path")
            .and_then(Value::as_str)
            .unwrap_or("<file>")
            .to_string();
        let diag = match entry.get("diagnostic") {
            Some(v) if !v.is_object() => {
                return Err("diagnostic field must be a JSON object".to_string());
            }
            Some(v) => v.clone(),
            None => json!({}),
        };
        let sev = severity_label(diag.get("severity").and_then(Value::as_i64));
        let out = diagnostic_output(&diag)?;
        file_map
            .entry(sev)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("severity map must be an object")?
            .entry(name_path)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("list must be an array")?
            .push(out);
    }
    Ok(json!({ relative_path: file_map }))
}

/// Finds the innermost owning symbol `name_path` covering 1-based `line`.
pub fn owner_name_path(symbols: &[Symbol], line: usize) -> String {
    flatten(symbols)
        .into_iter()
        .filter(|symbol| symbol.line <= line && line <= symbol.end_line)
        .min_by_key(|symbol| {
            (
                symbol.end_byte.saturating_sub(symbol.start_byte),
                usize::MAX - symbol.name_path.len(),
            )
        })
        .map(|symbol| symbol.name_path.clone())
        .unwrap_or_else(|| "<file>".to_string())
}

/// Groups `diagnostics` for `path` by severity and owning symbol.
pub fn group_diagnostics(
    path: &Path,
    diagnostics: &[Diagnostic],
    symbols_override: Option<&[Symbol]>,
    config: Option<&ProjectConfig>,
) -> Result<Value, String> {
    let owned_parsed = if symbols_override.is_none() {
        codemode_ts::parse_file(path, config).ok()
    } else {
        None
    };
    let symbols = symbols_override.or_else(|| owned_parsed.as_ref().map(|p| p.symbols.as_slice()));
    let file_key = path.display().to_string();
    let entries: Vec<Value> = diagnostics
        .iter()
        .map(|diagnostic| {
            let name_path = symbols
                .map(|syms| owner_name_path(syms, diagnostic.line as usize))
                .unwrap_or_else(|| "<file>".to_string());
            let mut lsp_diag = json!({
                "message": diagnostic.message,
                "range": {
                    "start": {"line": diagnostic.line.saturating_sub(1), "character": diagnostic.column.saturating_sub(1)},
                    "end": {"line": diagnostic.end_line.saturating_sub(1), "character": diagnostic.end_column.saturating_sub(1)}
                }
            });
            if let Some(sev) = match diagnostic.severity.as_str() {
                "error" => Some(1),
                "warning" => Some(2),
                "info" => Some(3),
                "hint" => Some(4),
                _ => None,
            } {
                lsp_diag["severity"] = json!(sev);
            }
            if let Some(code) = &diagnostic.code {
                lsp_diag["code"] = code.clone();
            }
            if let Some(source) = &diagnostic.source {
                lsp_diag["source"] = json!(source);
            }
            json!({
                "name_path": name_path,
                "diagnostic": lsp_diag
            })
        })
        .collect();
    group_entries(&file_key, &entries)
}

#[derive(Clone)]
struct IgnoreRule {
    negated: bool,
    dir_only: bool,
    anchored: bool,
    pattern: String,
}

#[derive(Clone)]
struct IgnoreScope {
    base: String,
    rules: Vec<IgnoreRule>,
}

fn is_default_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "venv"
                | "__pycache__"
                | "dist"
                | "build"
                | "out"
                | "vendor"
        )
}

fn parse_ignore_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<IgnoreRule> {
    let mut rules = Vec::new();
    for raw_line in lines {
        let line = raw_line.trim_end_matches('\r').trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negated, rest) = if let Some(stripped) = line.strip_prefix('!') {
            (true, stripped)
        } else {
            (false, line)
        };
        let rest = rest.strip_prefix("\\#").map_or(rest, |_| &rest[1..]);
        let (dir_only, rest) = if let Some(stripped) = rest.strip_suffix('/') {
            (true, stripped)
        } else {
            (false, rest)
        };
        if rest.is_empty() {
            continue;
        }
        let anchored = rest.contains('/');
        let pattern = rest.trim_start_matches('/').to_string();
        if !pattern.is_empty() {
            rules.push(IgnoreRule {
                negated,
                dir_only,
                anchored,
                pattern,
            });
        }
    }
    rules
}

fn parse_ignore_file(path: &Path) -> Vec<IgnoreRule> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_ignore_lines(content.lines())
}

fn seg_glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '\\' && pi + 1 < p.len() {
            if p[pi + 1] == t[ti] {
                pi += 2;
                ti += 1;
                continue;
            }
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
            continue;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
            continue;
        }
        if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn match_path_segments(pat_segs: &[&str], txt_segs: &[&str]) -> bool {
    if pat_segs.is_empty() {
        return txt_segs.is_empty();
    }
    if pat_segs[0] == "**" {
        if pat_segs.len() == 1 {
            return !txt_segs.is_empty();
        }
        if match_path_segments(&pat_segs[1..], txt_segs) {
            return true;
        }
        return !txt_segs.is_empty() && match_path_segments(pat_segs, &txt_segs[1..]);
    }
    !txt_segs.is_empty()
        && seg_glob_match(pat_segs[0], txt_segs[0])
        && match_path_segments(&pat_segs[1..], &txt_segs[1..])
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').collect();
    let txt_segs: Vec<&str> = text.split('/').collect();
    match_path_segments(&pat_segs, &txt_segs)
}

fn is_ignored(scopes: &[IgnoreScope], rel: &str, name: &str, is_dir: bool) -> bool {
    let mut ignored = false;
    for scope in scopes {
        let sub_rel = if scope.base.is_empty() {
            rel
        } else if let Some(rest) = rel.strip_prefix(&scope.base) {
            if let Some(after_slash) = rest.strip_prefix('/') {
                after_slash
            } else {
                continue;
            }
        } else {
            continue;
        };
        if sub_rel.is_empty() {
            continue;
        }
        for rule in &scope.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let matched = if rule.anchored {
                glob_match(&rule.pattern, sub_rel)
            } else {
                seg_glob_match(&rule.pattern, name)
            };
            if matched {
                ignored = !rule.negated;
            }
        }
    }
    ignored
}

#[allow(clippy::too_many_arguments)]
fn walk_project_dir(
    root: &Path,
    dir: &Path,
    lang_filter: Option<LanguageId>,
    config: Option<&ProjectConfig>,
    scopes: &mut Vec<IgnoreScope>,
    visited_dirs: &mut HashSet<PathBuf>,
    limit: usize,
    out: &mut Vec<PathBuf>,
    capped: &mut bool,
) {
    if *capped {
        return;
    }
    let Ok(canon_dir) = std::fs::canonicalize(dir) else {
        return;
    };
    if !visited_dirs.insert(canon_dir) {
        return;
    }

    let rel_base = dir
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();

    let mut pushed_rules = 0usize;
    for file_name in [".gitignore", ".ignore"] {
        let rules = parse_ignore_file(&dir.join(file_name));
        if !rules.is_empty() {
            scopes.push(IgnoreScope {
                base: rel_base.clone(),
                rules,
            });
            pushed_rules += 1;
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        for _ in 0..pushed_rules {
            scopes.pop();
        }
        return;
    };
    let mut children: Vec<(String, PathBuf, bool)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        children.push((name, entry.path(), file_type.is_dir()));
    }
    children.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, path, is_dir) in children {
        if *capped {
            break;
        }
        let rel = if rel_base.is_empty() {
            name.clone()
        } else {
            format!("{rel_base}/{name}")
        };
        if is_dir && is_default_ignored_dir(&name) {
            continue;
        }
        if is_ignored(scopes, &rel, &name, is_dir) {
            continue;
        }
        if is_dir {
            walk_project_dir(
                root,
                &path,
                lang_filter,
                config,
                scopes,
                visited_dirs,
                limit,
                out,
                capped,
            );
        } else if let Some(lang) = resolve_language(&path, config)
            && lang_filter.is_none_or(|target| target == lang)
        {
            if out.len() >= limit {
                *capped = true;
                break;
            }
            out.push(std::fs::canonicalize(&path).unwrap_or(path));
        }
    }

    for _ in 0..pushed_rules {
        scopes.pop();
    }
}

/// Discovers source files under `root`, honoring `.gitignore`, `.ignore`, config ignores, and `max_files`.
pub fn discover_project_files(
    root: &Path,
    lang_filter: Option<LanguageId>,
    priority_file: Option<&Path>,
    max_files: usize,
    config: Option<&ProjectConfig>,
) -> (Vec<PathBuf>, bool) {
    let limit = max_files.clamp(1, DEFAULT_MAX_PROJECT_FILES);
    let mut scopes = Vec::new();
    if let Some(cfg) = config
        && !cfg.ignore.extra_patterns.is_empty()
    {
        let rules = parse_ignore_lines(cfg.ignore.extra_patterns.iter().map(String::as_str));
        if !rules.is_empty() {
            scopes.push(IgnoreScope {
                base: String::new(),
                rules,
            });
        }
    }
    let mut visited = HashSet::new();
    let mut out = Vec::new();
    let mut capped = false;

    walk_project_dir(
        root,
        root,
        lang_filter,
        config,
        &mut scopes,
        &mut visited,
        limit,
        &mut out,
        &mut capped,
    );

    if let Some(priority) = priority_file {
        let priority = std::fs::canonicalize(priority).unwrap_or_else(|_| priority.to_path_buf());
        if priority.is_file() {
            if let Some(pos) = out.iter().position(|existing| existing == &priority) {
                out.remove(pos);
                out.insert(0, priority);
            } else if let Some(lang) = resolve_language(&priority, config)
                && lang_filter.is_none_or(|target| target == lang)
            {
                if out.len() >= limit {
                    out.pop();
                    capped = true;
                }
                out.insert(0, priority);
            }
        }
    }

    (out, capped)
}
