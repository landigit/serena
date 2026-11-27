//! The five tools. Every call is stateless and derives its workspace from the
//! file path, so parallel worktrees and subagents never collide.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::lsp::{LspManager, position_of};
use crate::ts;

const DEFAULT_MAX_CHARS: usize = 6000;

pub fn list() -> Value {
    json!({"tools": [
        {
            "name": "ast_outline",
            "description": "Hierarchical symbol outline of one source file (tree-sitter, no LSP). Use before reading a large file; it also finds handlers and hooks nested inside big components.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "Path to the source file"},
                    "max_depth": {"type": "integer", "minimum": 1, "description": "Limit nesting depth (default: all)"}
                },
                "required": ["file"]
            }
        },
        {
            "name": "read_symbol",
            "description": "Return the exact source of one symbol by name path (e.g. 'ClassA/method', 'inner', '/ClassA/method'). Output is capped; use ast_outline first when unsure of the path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "Path to the source file"},
                    "name_path": {"type": "string", "description": "Simple name, relative suffix path, or absolute /path"},
                    "substring": {"type": "boolean", "description": "Substring-match the last segment (default false)"},
                    "max_chars": {"type": "integer", "minimum": 500, "maximum": 20000, "description": "Output cap (default 6000)"}
                },
                "required": ["file", "name_path"]
            }
        },
        {
            "name": "find_references",
            "description": "Cross-file references, definition, or implementations via the language server. Give name_path, or line and column (1-based). Re-syncs the file from disk before querying, so native edits are never stale.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "Path to the source file"},
                    "name_path": {"type": "string", "description": "Symbol name path (preferred over line/column)"},
                    "line": {"type": "integer", "minimum": 1, "description": "1-based line"},
                    "column": {"type": "integer", "minimum": 1, "description": "1-based character column"},
                    "kind": {"type": "string", "enum": ["references", "definition", "implementation"], "description": "Default references"},
                    "include_declaration": {"type": "boolean", "description": "Include the declaration in references (default true)"},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 1000, "description": "Default 200"}
                },
                "required": ["file"]
            }
        },
        {
            "name": "edit_symbol",
            "description": "Replace one symbol/node by name path (tree-sitter), or rename it across the project via LSP. Returns a unified diff. Never edits outside the named symbol unless rename is requested.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "Path to the source file"},
                    "name_path": {"type": "string", "description": "Symbol name path to replace or rename"},
                    "mode": {"type": "string", "enum": ["replace", "rename"], "description": "Default replace"},
                    "content": {"type": "string", "description": "Replacement source (mode=replace)"},
                    "new_name": {"type": "string", "description": "New symbol name (mode=rename)"},
                    "dry_run": {"type": "boolean", "description": "Return the diff without writing (replace only)"}
                },
                "required": ["file", "name_path"]
            }
        },
        {
            "name": "diagnostics",
            "description": "Compiler/linter diagnostics via the language server without running a full build. Supports single-file (default) or project-wide scope with an ignore-aware file cap. Re-syncs from disk first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "Path to a source file (or project directory for scope='project')"},
                    "scope": {"type": "string", "enum": ["file", "project"], "description": "Default 'file' for a file path, or 'project' for a directory"},
                    "max_files": {"type": "integer", "minimum": 1, "maximum": 500, "description": "Maximum files to scan when scope='project' (default 50)"},
                    "min_severity": {"type": "string", "enum": ["error", "warning", "info", "hint", "all"], "description": "Default warning"},
                    "grouped": {"type": "boolean", "description": "Group diagnostics by severity and owning symbol name_path (default false)"}
                },
                "required": ["file"]
            }
        }
    ]})
}

pub fn call(params: &Value, manager: &Arc<LspManager>) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match run(name, &arguments, manager) {
        Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
        Err(error) => json!({
            "content": [{"type": "text", "text": format!("astlens: {error}")}],
            "isError": true
        }),
    }
}

fn run(name: &str, arguments: &Value, manager: &Arc<LspManager>) -> Result<String, String> {
    match name {
        "ast_outline" => outline(arguments),
        "read_symbol" => read_symbol(arguments),
        "find_references" => references(arguments, manager),
        "edit_symbol" => edit_symbol(arguments, manager),
        "diagnostics" => diagnostics(arguments, manager),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn string_argument(arguments: &Value, key: &str) -> Result<String, String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string argument '{key}'"))
}

fn bool_argument(arguments: &Value, key: &str, default: bool) -> bool {
    arguments
        .get(key)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn number_argument(arguments: &Value, key: &str) -> Option<u64> {
    arguments.get(key).and_then(Value::as_u64)
}

fn path_argument(arguments: &Value, key: &str) -> Result<PathBuf, String> {
    let raw = string_argument(arguments, key)?;
    let path = PathBuf::from(&raw);
    let path = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path)
    };
    Ok(std::fs::canonicalize(&path).unwrap_or(path))
}

fn outline(arguments: &Value) -> Result<String, String> {
    let path = path_argument(arguments, "file")?;
    let parsed = ts::parse(&path)?;
    let max_depth = number_argument(arguments, "max_depth").map(|depth| depth as usize);
    let mut symbols = prune(&parsed.symbols, max_depth, 1);
    let mut text = json!({
        "file": path.display().to_string(),
        "language": parsed.lang.name(),
        "symbols": symbols
    });
    let mut rendered = serde_json::to_string_pretty(&text).map_err(|error| error.to_string())?;
    if rendered.len() > 20000 {
        symbols = prune(&parsed.symbols, Some(1), 1);
        text = json!({
            "file": path.display().to_string(),
            "language": parsed.lang.name(),
            "truncated": true,
            "note": "outline exceeded 20 KB; showing top-level symbols only. Use read_symbol for a subtree.",
            "symbols": symbols
        });
        rendered = serde_json::to_string_pretty(&text).map_err(|error| error.to_string())?;
    }
    Ok(rendered)
}

fn prune(symbols: &[ts::Symbol], max_depth: Option<usize>, depth: usize) -> Vec<Value> {
    symbols
        .iter()
        .map(|symbol| {
            let mut value = json!({
                "name": symbol.name,
                "kind": symbol.kind,
                "name_path": symbol.name_path,
                "line": symbol.line,
                "end_line": symbol.end_line
            });
            if max_depth.is_none_or(|limit| depth < limit) && !symbol.children.is_empty() {
                value["children"] = Value::Array(prune(&symbol.children, max_depth, depth + 1));
            }
            value
        })
        .collect()
}

fn read_symbol(arguments: &Value) -> Result<String, String> {
    let path = path_argument(arguments, "file")?;
    let pattern = string_argument(arguments, "name_path")?;
    let substring = bool_argument(arguments, "substring", false);
    let max_chars =
        number_argument(arguments, "max_chars").unwrap_or(DEFAULT_MAX_CHARS as u64) as usize;
    let parsed = ts::parse(&path)?;
    let symbol = ts::best_match(&parsed, &pattern, substring).ok_or_else(|| {
        let names: Vec<String> = parsed
            .symbols
            .iter()
            .take(25)
            .map(|symbol| symbol.name_path.clone())
            .collect();
        format!(
            "no symbol matching '{pattern}' in {}; top-level symbols: {}",
            path.display(),
            names.join(", ")
        )
    })?;
    let (body, truncated) = ts::body(&parsed, symbol, max_chars);
    let mut text = format!(
        "{}:{}-{}  {}  [{}]\n\n{}",
        path.display(),
        symbol.line,
        symbol.end_line,
        symbol.name_path,
        symbol.kind,
        body
    );
    if truncated {
        text.push_str(&format!("\n\n[truncated at {max_chars} characters; raise max_chars or read the file range directly]"));
    }
    Ok(text)
}

fn resolve_position(
    parsed: &ts::Parsed,
    pattern: &str,
    substring: bool,
) -> Result<(u32, u32, String), String> {
    let symbol = ts::best_match(parsed, pattern, substring)
        .ok_or_else(|| format!("no symbol matching '{pattern}'"))?;
    let (line, character) = position_of(&parsed.source, symbol.name_byte);
    Ok((line, character, symbol.name_path.clone()))
}

fn position_from_arguments(
    path: &Path,
    arguments: &Value,
) -> Result<(u32, u32, Option<String>), String> {
    if let Some(pattern) = arguments.get("name_path").and_then(Value::as_str) {
        let parsed = ts::parse(path)?;
        let (line, character, name_path) = resolve_position(
            &parsed,
            pattern,
            bool_argument(arguments, "substring", false),
        )?;
        return Ok((line, character, Some(name_path)));
    }
    let line =
        number_argument(arguments, "line").ok_or("give name_path, or line and column")? as u32;
    let column = number_argument(arguments, "column").unwrap_or(1) as u32;
    let source = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let (line, character) = line_col_to_lsp(&source, line.max(1), column.max(1));
    Ok((line, character, None))
}

fn line_col_to_lsp(source: &str, line1: u32, column1: u32) -> (u32, u32) {
    let line0 = line1.saturating_sub(1);
    let text = source.lines().nth(line0 as usize).unwrap_or("");
    let mut byte = 0;
    let mut units = 0;
    for (index, character) in text.chars().enumerate() {
        if index as u32 >= column1.saturating_sub(1) {
            break;
        }
        byte += character.len_utf8();
        units += character.len_utf16() as u32;
    }
    let _ = byte;
    (line0, units)
}

fn references(arguments: &Value, manager: &Arc<LspManager>) -> Result<String, String> {
    let path = path_argument(arguments, "file")?;
    let kind = arguments
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("references");
    if !matches!(kind, "references" | "definition" | "implementation") {
        return Err(format!("unknown kind '{kind}'"));
    }
    let include_declaration = bool_argument(arguments, "include_declaration", true);
    let max_results = number_argument(arguments, "max_results").unwrap_or(200) as usize;
    let (line, character, name_path) = position_from_arguments(&path, arguments)?;
    let client = manager.client_for(&path)?;
    let mut locations = client.references(&path, line, character, kind, include_declaration)?;
    let total = locations.len();
    locations.truncate(max_results);
    let text = json!({
        "kind": kind,
        "file": path.display().to_string(),
        "symbol": name_path,
        "count": total,
        "results": locations
    });
    serde_json::to_string_pretty(&text).map_err(|error| error.to_string())
}

fn edit_symbol(arguments: &Value, manager: &Arc<LspManager>) -> Result<String, String> {
    let path = path_argument(arguments, "file")?;
    let pattern = string_argument(arguments, "name_path")?;
    let mode = arguments
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("replace");

    match mode {
        "rename" => {
            let new_name = string_argument(arguments, "new_name")?;
            let parsed = ts::parse(&path)?;
            let (line, character, name_path) = resolve_position(&parsed, &pattern, false)?;
            let client = manager.client_for(&path)?;
            let changed = client.rename(&path, line, character, &new_name)?;
            let files: Vec<String> = changed
                .iter()
                .map(|(file, _)| file.display().to_string())
                .collect();
            let diff = changed
                .iter()
                .map(|(_, diff)| diff.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let text = json!({
                "mode": "rename",
                "symbol": name_path,
                "new_name": new_name,
                "changed_files": files,
                "diff": diff
            });
            return serde_json::to_string_pretty(&text).map_err(|error| error.to_string());
        }
        "replace" => {}
        other => return Err(format!("unknown mode '{other}'")),
    }

    let content = string_argument(arguments, "content")?;
    let dry_run = bool_argument(arguments, "dry_run", false);
    let parsed = ts::parse(&path)?;
    let symbol = ts::best_match(
        &parsed,
        &pattern,
        bool_argument(arguments, "substring", false),
    )
    .ok_or_else(|| format!("no symbol matching '{pattern}' in {}", path.display()))?;
    let old = parsed.source.clone();
    let mut new = old.clone();
    new.replace_range(symbol.start_byte..symbol.end_byte, &content);
    let diff = crate::diff::unified(&path, &old, &new);
    if !dry_run {
        std::fs::write(&path, &new)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    let text = json!({
        "mode": "replace",
        "symbol": symbol.name_path,
        "line": symbol.line,
        "end_line": symbol.end_line,
        "changed": !dry_run,
        "diff": diff
    });
    serde_json::to_string_pretty(&text).map_err(|error| error.to_string())
}

fn diagnostics(arguments: &Value, manager: &Arc<LspManager>) -> Result<String, String> {
    let path = path_argument(arguments, "file")?;
    if !path.exists() {
        return Err(format!("path does not exist: {}", path.display()));
    }
    let scope = match arguments.get("scope").and_then(Value::as_str) {
        Some(value @ ("file" | "project")) => value,
        Some(other) => return Err(format!("unknown scope '{other}'")),
        None if path.is_dir() => "project",
        None => "file",
    };
    let min_severity = arguments
        .get("min_severity")
        .and_then(Value::as_str)
        .unwrap_or("warning");
    let grouped = bool_argument(arguments, "grouped", false);
    let allowed: &[&str] = match min_severity {
        "error" => &["error"],
        "warning" => &["error", "warning"],
        "info" => &["error", "warning", "info"],
        "hint" | "all" => &["error", "warning", "info", "hint", "unknown"],
        other => return Err(format!("unknown min_severity '{other}'")),
    };
    if scope == "project" {
        let max_files = number_argument(arguments, "max_files")
            .unwrap_or(50)
            .clamp(1, 500) as usize;
        return project_diagnostics(&path, min_severity, allowed, grouped, max_files, manager);
    }
    if path.is_dir() {
        return Err(format!(
            "{} is a directory; pass scope='project' for project-wide diagnostics",
            path.display()
        ));
    }
    let client = manager.client_for(&path)?;
    let mut diagnostics: Vec<_> = client
        .diagnostics(&path)?
        .into_iter()
        .filter(|diagnostic| allowed.contains(&diagnostic.severity.as_str()))
        .collect();
    let total = diagnostics.len();
    diagnostics.truncate(500);
    let text = if grouped {
        let grouped_map = group_diagnostics(&path, &diagnostics)?;
        json!({
            "file": path.display().to_string(),
            "min_severity": min_severity,
            "count": total,
            "truncated": total > diagnostics.len(),
            "grouped": grouped_map
        })
    } else {
        json!({
            "file": path.display().to_string(),
            "min_severity": min_severity,
            "count": total,
            "truncated": total > diagnostics.len(),
            "diagnostics": diagnostics
        })
    };
    serde_json::to_string_pretty(&text).map_err(|error| error.to_string())
}

fn project_diagnostics(
    path: &Path,
    min_severity: &str,
    allowed: &[&str],
    grouped: bool,
    max_files: usize,
    manager: &Arc<LspManager>,
) -> Result<String, String> {
    let (root, lang_filter, priority_file) = if path.is_dir() {
        (path.to_path_buf(), None, None)
    } else {
        let lang = ts::Lang::from_path(path)
            .ok_or_else(|| format!("unsupported file type: {}", path.display()))?;
        (
            crate::roots::workspace_root(path, lang),
            Some(lang),
            Some(path),
        )
    };
    let (files, files_capped) =
        crate::roots::discover_project_files(&root, lang_filter, priority_file, max_files);
    let files_scanned = files.len();

    let mut by_client: HashMap<(ts::Lang, PathBuf), Vec<PathBuf>> = HashMap::new();
    let mut client_order = Vec::new();
    for file in files {
        if let Some(lang) = ts::Lang::from_path(&file) {
            let key = (lang, crate::roots::workspace_root(&file, lang));
            if !by_client.contains_key(&key) {
                client_order.push(key.clone());
            }
            by_client.entry(key).or_default().push(file);
        }
    }

    let mut per_file: Vec<(PathBuf, crate::lsp::Diagnostic)> = Vec::new();
    let mut last_error: Option<String> = None;
    let mut succeeded_any = false;
    for key in client_order {
        let Some(client_files) = by_client.get(&key) else {
            continue;
        };
        let client = match manager.client_for(&client_files[0]) {
            Ok(client) => client,
            Err(error) => {
                if lang_filter.is_some() {
                    return Err(error);
                }
                last_error = Some(error);
                continue;
            }
        };
        let batch = match client.project_diagnostics(client_files) {
            Ok(batch) => batch,
            Err(error) => {
                if lang_filter.is_some() {
                    return Err(error);
                }
                last_error = Some(error);
                continue;
            }
        };
        succeeded_any = true;
        for (file, file_diags) in batch {
            for diag in file_diags {
                if allowed.contains(&diag.severity.as_str()) {
                    per_file.push((file.clone(), diag));
                }
            }
        }
    }
    if files_scanned > 0
        && !succeeded_any
        && let Some(error) = last_error
    {
        return Err(error);
    }

    render_project_diagnostics(
        &root,
        min_severity,
        files_scanned,
        files_capped,
        grouped,
        per_file,
    )
}

fn render_project_diagnostics(
    root: &Path,
    min_severity: &str,
    files_scanned: usize,
    files_capped: bool,
    grouped: bool,
    mut per_file: Vec<(PathBuf, crate::lsp::Diagnostic)>,
) -> Result<String, String> {
    per_file.sort_by(|(fa, da), (fb, db)| {
        (fa, da.line, da.column, &da.message).cmp(&(fb, db.line, db.column, &db.message))
    });
    let total = per_file.len();
    per_file.truncate(500);
    let truncated = total > per_file.len();

    let text = if grouped {
        let mut grouped_by_file: HashMap<PathBuf, Vec<crate::lsp::Diagnostic>> = HashMap::new();
        let mut file_order = Vec::new();
        for (file, diag) in per_file {
            if !grouped_by_file.contains_key(&file) {
                file_order.push(file.clone());
            }
            grouped_by_file.entry(file).or_default().push(diag);
        }
        let mut merged = serde_json::Map::new();
        for file in file_order {
            if let Some(diags) = grouped_by_file.get(&file)
                && let Value::Object(map) = group_diagnostics(&file, diags)?
            {
                for (key, value) in map {
                    merged.insert(key, value);
                }
            }
        }
        json!({
            "scope": "project",
            "root": root.display().to_string(),
            "min_severity": min_severity,
            "files_scanned": files_scanned,
            "files_capped": files_capped,
            "count": total,
            "truncated": truncated,
            "grouped": Value::Object(merged)
        })
    } else {
        let items: Vec<Value> = per_file
            .into_iter()
            .map(|(file, diag)| {
                let mut entry = serde_json::to_value(&diag).unwrap_or_else(|_| json!({}));
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert("file".to_string(), json!(file.display().to_string()));
                }
                entry
            })
            .collect();
        json!({
            "scope": "project",
            "root": root.display().to_string(),
            "min_severity": min_severity,
            "files_scanned": files_scanned,
            "files_capped": files_capped,
            "count": total,
            "truncated": truncated,
            "diagnostics": items
        })
    };
    serde_json::to_string_pretty(&text).map_err(|error| error.to_string())
}

fn owner_name_path(symbols: &[ts::Symbol], line: usize) -> String {
    ts::flatten(symbols)
        .into_iter()
        .filter(|symbol| symbol.line <= line && line <= symbol.end_line)
        // Smallest byte span wins so same-line nested symbols beat their parent.
        .min_by_key(|symbol| {
            (
                symbol.end_byte - symbol.start_byte,
                usize::MAX - symbol.name_path.len(),
            )
        })
        .map(|symbol| symbol.name_path.clone())
        .unwrap_or_else(|| "<file>".to_string())
}

fn group_diagnostics(path: &Path, diagnostics: &[crate::lsp::Diagnostic]) -> Result<Value, String> {
    let parsed = ts::parse(path).ok();
    let file_key = path.display().to_string();
    let entries: Vec<Value> = diagnostics
        .iter()
        .map(|diagnostic| {
            let name_path = parsed
                .as_ref()
                .map(|parsed| owner_name_path(&parsed.symbols, diagnostic.line as usize))
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
    crate::diagnostics_group::group(file_key, entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Diagnostic;

    #[test]
    fn groups_diagnostics_by_owning_symbol() {
        let dir = std::env::temp_dir().join(format!("astlens-tools-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.py");
        std::fs::write(
            &path,
            "class Store:\n    def add(self, item):\n        return item\n",
        )
        .unwrap();
        let diagnostics = vec![Diagnostic {
            line: 2,
            column: 9,
            end_line: 2,
            end_column: 12,
            severity: "warning".to_string(),
            message: "Unused parameter".to_string(),
            code: None,
            source: Some("pyright".to_string()),
        }];
        let grouped = group_diagnostics(&path, &diagnostics).unwrap();
        let file_key = path.display().to_string();
        assert_eq!(
            grouped[&file_key]["Warning"]["Store/add"][0]["message"],
            "Unused parameter"
        );
    }

    #[test]
    fn same_line_nested_symbol_picks_inner_owner() {
        let dir = std::env::temp_dir().join(format!("astlens-tools-inline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("inline.tsx");
        std::fs::write(
            &path,
            "export function App() { const handleClick = () => 1; }\n",
        )
        .unwrap();
        let parsed = ts::parse(&path).unwrap();
        assert_eq!(owner_name_path(&parsed.symbols, 1), "App/handleClick");
    }

    #[test]
    fn rejects_unknown_mode_and_kind() {
        let dir = std::env::temp_dir().join(format!("astlens-tools-err-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sample.py");
        std::fs::write(&path, "def x():\n    pass\n").unwrap();
        let manager = Arc::new(LspManager::new());

        let err_mode = run(
            "edit_symbol",
            &json!({"file": path, "name_path": "x", "mode": "bogus", "content": "def x(): pass"}),
            &manager,
        )
        .unwrap_err();
        assert!(err_mode.contains("unknown mode"), "{err_mode}");

        let err_kind = run(
            "find_references",
            &json!({"file": path, "name_path": "x", "kind": "bogus"}),
            &manager,
        )
        .unwrap_err();
        assert!(err_kind.contains("unknown kind"), "{err_kind}");

        let err_scope = run(
            "diagnostics",
            &json!({"file": path, "scope": "bogus"}),
            &manager,
        )
        .unwrap_err();
        assert!(err_scope.contains("unknown scope"), "{err_scope}");

        let err_dir_file_scope = run(
            "diagnostics",
            &json!({"file": dir, "scope": "file"}),
            &manager,
        )
        .unwrap_err();
        assert!(
            err_dir_file_scope.contains("is a directory"),
            "{err_dir_file_scope}"
        );
    }

    #[test]
    fn empty_project_diagnostics_returns_clean_summary() {
        let dir = std::env::temp_dir().join(format!("astlens-tools-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manager = Arc::new(LspManager::new());

        let out = run(
            "diagnostics",
            &json!({"file": dir, "scope": "project", "grouped": true}),
            &manager,
        )
        .unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["scope"], "project");
        assert_eq!(parsed["files_scanned"], 0);
        assert_eq!(parsed["files_capped"], false);
        assert_eq!(parsed["count"], 0);
        assert_eq!(parsed["grouped"], json!({}));
    }

    #[test]
    fn groups_diagnostics_under_overloaded_owner_symbol() {
        let dir = std::env::temp_dir().join(format!("astlens-tools-ovl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Sample.java");
        std::fs::write(
            &path,
            "class Store {\n  int add(int a) { return a; }\n  double add(double a) { return a; }\n}\n",
        )
        .unwrap();
        let diagnostics = vec![Diagnostic {
            line: 3,
            column: 10,
            end_line: 3,
            end_column: 13,
            severity: "error".to_string(),
            message: "bad overload".to_string(),
            code: None,
            source: Some("jdtls".to_string()),
        }];
        let grouped = group_diagnostics(&path, &diagnostics).unwrap();
        let file_key = path.display().to_string();
        assert_eq!(
            grouped[&file_key]["Error"]["Store/add[1]"][0]["message"],
            "bad overload"
        );
    }

    #[test]
    fn diagnostics_rejects_nonexistent_path_and_renders_multifile_project_output() {
        let dir = std::env::temp_dir().join(format!("astlens-tools-proj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let manager = Arc::new(LspManager::new());

        let missing = dir.join("missing_dir");
        let err = run(
            "diagnostics",
            &json!({"file": missing, "scope": "project"}),
            &manager,
        )
        .unwrap_err();
        assert!(err.contains("path does not exist"), "{err}");

        let a = dir.join("A.java");
        let b = dir.join("B.java");
        std::fs::write(
            &a,
            "class A {\n  void m(int x) {}\n  void m(String x) {}\n}\n",
        )
        .unwrap();
        std::fs::write(&b, "class B {\n  void go() {}\n}\n").unwrap();

        let diags = vec![
            (
                b.clone(),
                Diagnostic {
                    line: 2,
                    column: 3,
                    end_line: 2,
                    end_column: 5,
                    severity: "warning".to_string(),
                    message: "b warn".to_string(),
                    code: None,
                    source: Some("jdtls".to_string()),
                },
            ),
            (
                a.clone(),
                Diagnostic {
                    line: 3,
                    column: 3,
                    end_line: 3,
                    end_column: 5,
                    severity: "error".to_string(),
                    message: "a err".to_string(),
                    code: None,
                    source: Some("jdtls".to_string()),
                },
            ),
        ];

        let flat: Value = serde_json::from_str(
            &render_project_diagnostics(&dir, "warning", 2, true, false, diags.clone()).unwrap(),
        )
        .unwrap();
        assert_eq!(flat["files_capped"], true);
        assert_eq!(flat["count"], 2);
        assert_eq!(flat["diagnostics"][0]["file"], a.display().to_string());
        assert_eq!(flat["diagnostics"][1]["file"], b.display().to_string());

        let grouped_out: Value = serde_json::from_str(
            &render_project_diagnostics(&dir, "warning", 2, false, true, diags).unwrap(),
        )
        .unwrap();
        assert_eq!(
            grouped_out["grouped"][a.display().to_string()]["Error"]["A/m[1]"][0]["message"],
            "a err"
        );
        assert_eq!(
            grouped_out["grouped"][b.display().to_string()]["Warning"]["B/go"][0]["message"],
            "b warn"
        );
    }
}
