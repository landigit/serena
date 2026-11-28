//! Unified 12-tool `code_*` MCP schema registry, dispatch, and bounded FTS5 spill wrapper.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codemode_core::config::{ProjectConfig, find_project_root, load_project_config};
use codemode_core::languages::{all_languages, resolve_language};
use codemode_core::paths::{canonical_or_lexical, context_db_path};
use codemode_lsp::{LspManager, discover_project_files, group_diagnostics, position_of};
use codemode_memories::MemoryStore;
use codemode_retrieval::{
    SearchQuery, build_resume_summary, html_to_text, index_path_gitignore, search_index,
};
use codemode_sandbox::{
    ExecDispatch, ExecRequest, JobId, JobRegistry, detect_runtimes, execute_sandboxed, job_status,
};
use codemode_store::{ContextDb, EventKind, JournalEvent, StoredChunkInput, utf8_preview};
use codemode_ts::{Symbol, TsError, best_match_in_symbols, parse_file, symbol_body, unified_diff};
use dashmap::DashMap;
use serde_json::{Value, json};

const DEFAULT_MAX_CHARS: usize = 6000;
const DEFAULT_REPLY_CAP_BYTES: usize = 8192;
const OUTLINE_CAP_BYTES: usize = 20_000;

/// Shared stateless runtime context for MCP tool calls (keyed per canonical project root).
pub struct ToolRuntime {
    pub lsp: Arc<LspManager>,
    pub dbs: DashMap<PathBuf, Arc<ContextDb>>,
    pub jobs: JobRegistry,
}

impl Default for ToolRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRuntime {
    pub fn new() -> Self {
        Self {
            lsp: Arc::new(LspManager::new()),
            dbs: DashMap::new(),
            jobs: JobRegistry::new(),
        }
    }

    pub fn db_for(&self, root: &Path) -> Result<Arc<ContextDb>, String> {
        let canon = canonical_or_lexical(root);
        if let Some(existing) = self.dbs.get(&canon) {
            return Ok(Arc::clone(existing.value()));
        }
        let db = Arc::new(ContextDb::open_for_project(&canon).map_err(|e| e.to_string())?);
        self.dbs.insert(canon, Arc::clone(&db));
        Ok(db)
    }
}

/// Returns the MCP `tools/list` schema payload for all 12 `code_*` tools.
pub fn list() -> Value {
    json!({"tools": [
        {
            "name": "code_outline",
            "description": "Hierarchical symbol outline of a source file (arborium tree-sitter, with automatic LSP documentSymbol fallback). Bounded to 20 KB.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string", "description": "Path to source file"},
                    "max_depth": {"type": "integer", "minimum": 1, "description": "Limit symbol nesting depth"}
                },
                "required": ["file"]
            }
        },
        {
            "name": "code_read",
            "description": "Read exact symbol body by Serena name_path (e.g. 'ClassA/method[1]', '/ClassA/method'). Oversized bodies spill to FTS5.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string"},
                    "name_path": {"type": "string"},
                    "substring": {"type": "boolean"},
                    "max_chars": {"type": "integer", "minimum": 200, "maximum": 50000}
                },
                "required": ["file", "name_path"]
            }
        },
        {
            "name": "code_references",
            "description": "Cross-file LSP references, definition, or implementations. Re-syncs disk state before querying.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string"},
                    "name_path": {"type": "string"},
                    "line": {"type": "integer", "minimum": 1},
                    "column": {"type": "integer", "minimum": 1},
                    "kind": {"type": "string", "enum": ["references", "definition", "implementation"]},
                    "include_declaration": {"type": "boolean"},
                    "max_results": {"type": "integer", "minimum": 1, "maximum": 1000}
                },
                "required": ["file"]
            }
        },
        {
            "name": "code_diagnostics",
            "description": "LSP diagnostics for one file or the project, with optional severity/symbol grouping and fast clean-file path.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string"},
                    "scope": {"type": "string", "enum": ["file", "project"]},
                    "min_severity": {"type": "string", "enum": ["error", "warning", "info", "hint", "all"]},
                    "grouped": {"type": "boolean"},
                    "max_files": {"type": "integer", "minimum": 1, "maximum": 500}
                },
                "required": ["file"]
            }
        },
        {
            "name": "code_edit",
            "description": "Replace one symbol body (mode=replace, dry_run supported) or rename across workspace via LSP (mode=rename). Returns unified diff.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file": {"type": "string"},
                    "name_path": {"type": "string"},
                    "mode": {"type": "string", "enum": ["replace", "rename"]},
                    "content": {"type": "string"},
                    "new_name": {"type": "string"},
                    "dry_run": {"type": "boolean"},
                    "substring": {"type": "boolean"}
                },
                "required": ["file", "name_path"]
            }
        },
        {
            "name": "code_execute",
            "description": "Execute code/commands in a scrubbed, PGID-isolated sandbox (or poll background job_id). Oversized stdout/stderr spills to FTS5.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": {"type": "string"},
                    "runtime": {"type": "string", "enum": ["sh", "bash", "python3", "node", "auto"]},
                    "cwd": {"type": "string"},
                    "project_root": {"type": "string"},
                    "timeout_ms": {"type": "integer", "minimum": 50},
                    "inline_cap_bytes": {"type": "integer", "minimum": 128},
                    "allow_env": {"type": "array", "items": {"type": "string"}},
                    "background": {"type": "boolean"},
                    "job_id": {"type": "string"},
                    "label": {"type": "string"}
                }
            }
        },
        {
            "name": "code_index",
            "description": "Index files/directories (gitignore-aware), HTML, or raw text into the project FTS5 store.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "text": {"type": "string"},
                    "html": {"type": "string"},
                    "label": {"type": "string"},
                    "project_root": {"type": "string"},
                    "max_chunk_bytes": {"type": "integer", "minimum": 128}
                }
            }
        },
        {
            "name": "code_search",
            "description": "Unified BM25 + trigram search across indexed outputs, repo files, memories, and spilled logs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "source_id": {"type": "string"},
                    "label": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                    "project_root": {"type": "string"}
                },
                "required": ["query"]
            }
        },
        {
            "name": "code_resume",
            "description": "Return compaction recovery summary (last task, changed files, open errors, decisions, search terms).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project_root": {"type": "string"}
                }
            }
        },
        {
            "name": "code_memory",
            "description": "CRUD and mem: reference graph operations on .codemode/memories/*.md (auto-indexed into FTS5).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["read", "write", "list", "delete", "rename", "graph"]},
                    "name": {"type": "string"},
                    "new_name": {"type": "string"},
                    "content": {"type": "string"},
                    "topic": {"type": "string"},
                    "project_root": {"type": "string"}
                },
                "required": ["action"]
            }
        },
        {
            "name": "code_batch",
            "description": "Execute multiple code_* calls in parallel and return one bounded summary.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "calls": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "arguments": {"type": "object"}
                            },
                            "required": ["name", "arguments"]
                        }
                    },
                    "project_root": {"type": "string"}
                },
                "required": ["calls"]
            }
        },
        {
            "name": "code_admin",
            "description": "Operational commands: stats, doctor, purge, upgrade, lsp_status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["stats", "doctor", "purge", "upgrade", "lsp_status"]},
                    "project_root": {"type": "string"}
                },
                "required": ["action"]
            }
        }
    ]})
}

/// Dispatches a JSON-RPC `tools/call` request against `ToolRuntime`.
pub fn call(params: &Value, rt: &ToolRuntime) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match run(name, &arguments, rt) {
        Ok(text) => json!({"content": [{"type": "text", "text": text}]}),
        Err(error) => json!({
            "content": [{"type": "text", "text": format!("codemode: {error}")}],
            "isError": true
        }),
    }
}

/// Executes one named `code_*` tool with `arguments`.
pub fn run(name: &str, arguments: &Value, rt: &ToolRuntime) -> Result<String, String> {
    match name {
        "code_outline" => tool_outline(arguments, rt),
        "code_read" => tool_read(arguments, rt),
        "code_references" => tool_references(arguments, rt),
        "code_diagnostics" => tool_diagnostics(arguments, rt),
        "code_edit" => tool_edit(arguments, rt),
        "code_execute" => tool_execute(arguments, rt),
        "code_index" => tool_index(arguments, rt),
        "code_search" => tool_search(arguments, rt),
        "code_resume" => tool_resume(arguments, rt),
        "code_memory" => tool_memory(arguments, rt),
        "code_batch" => tool_batch(arguments, rt),
        "code_admin" => tool_admin(arguments, rt),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn string_arg(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing string argument '{key}'"))
}

fn bool_arg(args: &Value, key: &str, default: bool) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn num_arg(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(Value::as_u64)
}

fn path_arg(args: &Value, key: &str) -> Result<PathBuf, String> {
    let raw = string_arg(args, key)?;
    let p = PathBuf::from(raw);
    let abs = if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?.join(p)
    };
    Ok(canonical_or_lexical(&abs))
}

fn project_root_from(args: &Value, fallback_path: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(raw) = args.get("project_root").and_then(Value::as_str) {
        let p = PathBuf::from(raw);
        let abs = if p.is_absolute() {
            p
        } else {
            std::env::current_dir().map_err(|e| e.to_string())?.join(p)
        };
        return Ok(canonical_or_lexical(&abs));
    }
    if let Some(p) = fallback_path {
        return Ok(canonical_or_lexical(&find_project_root(p)));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    Ok(canonical_or_lexical(&find_project_root(&cwd)))
}

fn load_config(root: &Path) -> Result<ProjectConfig, String> {
    load_project_config(root).map_err(|e| e.to_string())
}

fn parse_with_fallback(
    path: &Path,
    cfg: &ProjectConfig,
    rt: &ToolRuntime,
) -> Result<(String, String, Vec<Symbol>), String> {
    match parse_file(path, Some(cfg)) {
        Ok(parsed) => Ok((parsed.lang.key().to_string(), parsed.source, parsed.symbols)),
        Err(TsError::UnsupportedGrammar(lang)) => {
            let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
            let client = rt
                .lsp
                .client_for(path, Some(cfg))
                .map_err(|e| e.to_string())?;
            let symbols = client.document_symbols(path).map_err(|e| e.to_string())?;
            Ok((lang.key().to_string(), source, symbols))
        }
        Err(e) => Err(e.to_string()),
    }
}

fn prune_symbols(symbols: &[Symbol], max_depth: Option<usize>, depth: usize) -> Vec<Value> {
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
                value["children"] =
                    Value::Array(prune_symbols(&symbol.children, max_depth, depth + 1));
            }
            value
        })
        .collect()
}

fn bound_reply(
    label: &str,
    text: String,
    cap_bytes: usize,
    db: &ContextDb,
) -> Result<String, String> {
    if text.len() <= cap_bytes {
        return Ok(text);
    }
    let lines: Vec<&str> = text.lines().collect();
    let chunks: Vec<StoredChunkInput> = lines
        .chunks(100)
        .enumerate()
        .map(|(i, grp)| StoredChunkInput {
            start_line: i * 100 + 1,
            end_line: i * 100 + grp.len(),
            symbol_path: None,
            kind: Some("reply".into()),
            content: grp.join("\n"),
        })
        .collect();
    let handle = db
        .store_spilled(label, "reply", None, &text, chunks)
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&json!({
        "spilled": handle,
        "note": "output exceeded inline cap; query full contents with code_search using source_id"
    }))
    .map_err(|e| e.to_string())
}

fn tool_outline(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let path = path_arg(args, "file")?;
    let root = project_root_from(args, Some(&path))?;
    let cfg = load_config(&root)?;
    let (language, _source, symbols) = parse_with_fallback(&path, &cfg, rt)?;
    let max_depth = num_arg(args, "max_depth").map(|d| d as usize);
    let pruned = prune_symbols(&symbols, max_depth, 1);
    let mut rendered = serde_json::to_string_pretty(&json!({
        "file": path.display().to_string(),
        "language": language,
        "symbols": pruned
    }))
    .map_err(|e| e.to_string())?;
    if rendered.len() > OUTLINE_CAP_BYTES {
        let top = prune_symbols(&symbols, Some(1), 1);
        rendered = serde_json::to_string_pretty(&json!({
            "file": path.display().to_string(),
            "language": language,
            "truncated": true,
            "note": "outline exceeded 20 KB; showing top-level symbols only",
            "symbols": top
        }))
        .map_err(|e| e.to_string())?;
    }
    Ok(rendered)
}

fn tool_read(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let path = path_arg(args, "file")?;
    let pattern = string_arg(args, "name_path")?;
    let substring = bool_arg(args, "substring", false);
    let max_chars = num_arg(args, "max_chars").unwrap_or(DEFAULT_MAX_CHARS as u64) as usize;
    let root = project_root_from(args, Some(&path))?;
    let cfg = load_config(&root)?;
    let (_lang, source, symbols) = parse_with_fallback(&path, &cfg, rt)?;
    let symbol = best_match_in_symbols(&symbols, &pattern, substring).ok_or_else(|| {
        let top: Vec<&str> = symbols
            .iter()
            .take(25)
            .map(|s| s.name_path.as_str())
            .collect();
        format!(
            "no symbol matching '{pattern}' in {}; top-level: {}",
            path.display(),
            top.join(", ")
        )
    })?;
    let parsed_stub = codemode_ts::ParsedFile {
        lang: resolve_language(&path, Some(&cfg))
            .unwrap_or(codemode_core::languages::LanguageId::Python),
        source: source.clone(),
        symbols: symbols.clone(),
    };
    let (body, truncated) = symbol_body(&parsed_stub, symbol, max_chars);
    let header = format!(
        "{}:{}-{}  {}  [{}]\n\n{}",
        path.display(),
        symbol.line,
        symbol.end_line,
        symbol.name_path,
        symbol.kind,
        body
    );
    if !truncated {
        return Ok(header);
    }
    let db = rt.db_for(&root)?;
    let full = &source[symbol.start_byte..symbol.end_byte];
    let handle = db
        .store_spilled(
            &format!("symbol:{}", symbol.name_path),
            "symbol",
            Some(&path.display().to_string()),
            full,
            vec![StoredChunkInput {
                start_line: symbol.line,
                end_line: symbol.end_line,
                symbol_path: Some(symbol.name_path.clone()),
                kind: Some(symbol.kind.clone()),
                content: full.to_string(),
            }],
        )
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "{header}\n\n[truncated at {max_chars} chars; full body spilled to source_id={}]",
        handle.source_id
    ))
}

fn line_col_to_lsp(source: &str, line1: u32, col1: u32) -> (u32, u32) {
    let line0 = line1.saturating_sub(1);
    let text = source.lines().nth(line0 as usize).unwrap_or("");
    let mut units = 0u32;
    for (idx, ch) in text.chars().enumerate() {
        if idx as u32 >= col1.saturating_sub(1) {
            break;
        }
        units += ch.len_utf16() as u32;
    }
    (line0, units)
}

fn resolve_pos(
    path: &Path,
    args: &Value,
    cfg: &ProjectConfig,
    rt: &ToolRuntime,
) -> Result<(u32, u32, Option<String>), String> {
    if let Some(pattern) = args.get("name_path").and_then(Value::as_str) {
        let (_lang, source, symbols) = parse_with_fallback(path, cfg, rt)?;
        let sym = best_match_in_symbols(&symbols, pattern, bool_arg(args, "substring", false))
            .ok_or_else(|| format!("no symbol matching '{pattern}'"))?;
        let (line, ch) = position_of(&source, sym.name_byte);
        return Ok((line, ch, Some(sym.name_path.clone())));
    }
    let line = num_arg(args, "line").ok_or("give name_path, or line and column")? as u32;
    let col = num_arg(args, "column").unwrap_or(1) as u32;
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let (l0, c0) = line_col_to_lsp(&source, line.max(1), col.max(1));
    Ok((l0, c0, None))
}

fn tool_references(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let path = path_arg(args, "file")?;
    let root = project_root_from(args, Some(&path))?;
    let cfg = load_config(&root)?;
    let kind = args
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("references");
    if !matches!(kind, "references" | "definition" | "implementation") {
        return Err(format!("unknown kind '{kind}'"));
    }
    let include_decl = bool_arg(args, "include_declaration", true);
    let max_results = num_arg(args, "max_results").unwrap_or(200) as usize;
    let (line, character, name_path) = resolve_pos(&path, args, &cfg, rt)?;
    let client = rt
        .lsp
        .client_for(&path, Some(&cfg))
        .map_err(|e| e.to_string())?;
    let mut locations = client
        .references(&path, line, character, kind, include_decl)
        .map_err(|e| e.to_string())?;
    let total = locations.len();
    locations.truncate(max_results);
    let rendered = serde_json::to_string_pretty(&json!({
        "kind": kind,
        "file": path.display().to_string(),
        "symbol": name_path,
        "count": total,
        "results": locations
    }))
    .map_err(|e| e.to_string())?;
    let db = rt.db_for(&root)?;
    bound_reply(
        &format!("refs:{kind}"),
        rendered,
        cfg.policy.inline_cap_bytes.max(DEFAULT_REPLY_CAP_BYTES),
        &db,
    )
}

fn tool_diagnostics(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let path = path_arg(args, "file")?;
    let root = project_root_from(args, Some(&path))?;
    let cfg = load_config(&root)?;
    let scope = args.get("scope").and_then(Value::as_str).unwrap_or("file");
    let min_severity = args
        .get("min_severity")
        .and_then(Value::as_str)
        .unwrap_or("warning");
    let grouped = bool_arg(args, "grouped", false);
    let allowed: &[&str] = match min_severity {
        "error" => &["error"],
        "warning" => &["error", "warning"],
        "info" => &["error", "warning", "info"],
        "hint" | "all" => &["error", "warning", "info", "hint", "unknown"],
        other => return Err(format!("unknown min_severity '{other}'")),
    };
    let client = rt
        .lsp
        .client_for(&path, Some(&cfg))
        .map_err(|e| e.to_string())?;
    let payload = match scope {
        "file" => {
            let mut diags: Vec<_> = client
                .diagnostics(&path)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter(|d| allowed.contains(&d.severity.as_str()))
                .collect();
            let total = diags.len();
            diags.truncate(500);
            if grouped {
                let g = group_diagnostics(&path, &diags, None, Some(&cfg))?;
                json!({
                    "scope": "file",
                    "file": path.display().to_string(),
                    "min_severity": min_severity,
                    "count": total,
                    "grouped": g
                })
            } else {
                json!({
                    "scope": "file",
                    "file": path.display().to_string(),
                    "min_severity": min_severity,
                    "count": total,
                    "diagnostics": diags
                })
            }
        }
        "project" => {
            let max_files = num_arg(args, "max_files").unwrap_or(50) as usize;
            let lang_filter = resolve_language(&path, Some(&cfg));
            let (files, capped) =
                discover_project_files(&root, lang_filter, Some(&path), max_files, Some(&cfg));
            let per_file = client
                .project_diagnostics(&files)
                .map_err(|e| e.to_string())?;
            let mut total = 0usize;
            let mut entries = Vec::new();
            for (fpath, diags) in per_file {
                let filtered: Vec<_> = diags
                    .into_iter()
                    .filter(|d| allowed.contains(&d.severity.as_str()))
                    .collect();
                if filtered.is_empty() {
                    continue;
                }
                total += filtered.len();
                if grouped {
                    entries.push(group_diagnostics(&fpath, &filtered, None, Some(&cfg))?);
                } else {
                    entries.push(
                        json!({"file": fpath.display().to_string(), "diagnostics": filtered}),
                    );
                }
            }
            json!({
                "scope": "project",
                "files_checked": files.len(),
                "capped": capped,
                "min_severity": min_severity,
                "count": total,
                "results": entries
            })
        }
        other => return Err(format!("unknown scope '{other}'")),
    };
    let rendered = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
    let db = rt.db_for(&root)?;
    bound_reply(
        "diagnostics",
        rendered,
        cfg.policy.inline_cap_bytes.max(DEFAULT_REPLY_CAP_BYTES),
        &db,
    )
}

fn tool_edit(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let path = path_arg(args, "file")?;
    let pattern = string_arg(args, "name_path")?;
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("replace");
    let root = project_root_from(args, Some(&path))?;
    let cfg = load_config(&root)?;
    let db = rt.db_for(&root)?;

    match mode {
        "rename" => {
            let new_name = string_arg(args, "new_name")?;
            let (_lang, source, symbols) = parse_with_fallback(&path, &cfg, rt)?;
            let sym = best_match_in_symbols(&symbols, &pattern, false)
                .ok_or_else(|| format!("no symbol matching '{pattern}'"))?;
            let (line, character) = position_of(&source, sym.name_byte);
            let client = rt
                .lsp
                .client_for(&path, Some(&cfg))
                .map_err(|e| e.to_string())?;
            let changed = client
                .rename(&path, line, character, &new_name)
                .map_err(|e| e.to_string())?;
            let files: Vec<String> = changed
                .iter()
                .map(|(f, _)| f.display().to_string())
                .collect();
            for f in &files {
                let _ = db.record_event(&JournalEvent {
                    turn_number: 0,
                    kind: EventKind::Edit,
                    summary: format!("Renamed {} -> {new_name}", sym.name_path),
                    detail: None,
                    path: Some(f.clone()),
                    bytes_in: 0,
                    bytes_out: 128,
                });
            }
            let diff: String = changed
                .iter()
                .map(|(_, d)| d.clone())
                .collect::<Vec<_>>()
                .join("\n");
            serde_json::to_string_pretty(&json!({
                "mode": "rename",
                "symbol": sym.name_path,
                "new_name": new_name,
                "changed_files": files,
                "diff": diff
            }))
            .map_err(|e| e.to_string())
        }
        "replace" => {
            let content = string_arg(args, "content")?;
            let dry_run = bool_arg(args, "dry_run", false);
            let (_lang, old, symbols) = parse_with_fallback(&path, &cfg, rt)?;
            let symbol =
                best_match_in_symbols(&symbols, &pattern, bool_arg(args, "substring", false))
                    .ok_or_else(|| {
                        format!("no symbol matching '{pattern}' in {}", path.display())
                    })?;
            let mut new = old.clone();
            new.replace_range(symbol.start_byte..symbol.end_byte, &content);
            let diff = unified_diff(&path, &old, &new);
            if !dry_run {
                std::fs::write(&path, &new).map_err(|e| e.to_string())?;
                let _ = db.record_event(&JournalEvent {
                    turn_number: 0,
                    kind: EventKind::Edit,
                    summary: format!("Edited {}", symbol.name_path),
                    detail: None,
                    path: Some(path.display().to_string()),
                    bytes_in: old.len() as u64,
                    bytes_out: diff.len() as u64,
                });
            }
            serde_json::to_string_pretty(&json!({
                "mode": "replace",
                "symbol": symbol.name_path,
                "line": symbol.line,
                "end_line": symbol.end_line,
                "changed": !dry_run,
                "diff": diff
            }))
            .map_err(|e| e.to_string())
        }
        other => Err(format!("unknown mode '{other}'")),
    }
}

fn tool_execute(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    if let Some(job_id) = args.get("job_id").and_then(Value::as_str) {
        let status = job_status(&rt.jobs, &JobId(job_id.to_string()));
        return serde_json::to_string_pretty(&status).map_err(|e| e.to_string());
    }
    let code = string_arg(args, "code")?;
    let cwd = if let Some(raw) = args.get("cwd").and_then(Value::as_str) {
        canonical_or_lexical(Path::new(raw))
    } else {
        std::env::current_dir().map_err(|e| e.to_string())?
    };
    let root = project_root_from(args, Some(&cwd))?;
    let cfg = load_config(&root)?;
    let db = rt.db_for(&root)?;

    let runtime = args
        .get("runtime")
        .and_then(Value::as_str)
        .unwrap_or("sh")
        .to_string();
    let timeout_ms = num_arg(args, "timeout_ms").unwrap_or(cfg.policy.timeout_ms);
    let inline_cap_bytes =
        num_arg(args, "inline_cap_bytes").unwrap_or(cfg.policy.inline_cap_bytes as u64) as usize;
    let allow_env: Vec<String> = args
        .get("allow_env")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| cfg.policy.allow_env.clone());
    let background = bool_arg(args, "background", false);
    let label = args
        .get("label")
        .and_then(Value::as_str)
        .map(str::to_string);

    let dispatch = execute_sandboxed(
        ExecRequest {
            runtime,
            code,
            cwd,
            timeout_ms,
            inline_cap_bytes,
            allow_env,
            extra_env: BTreeMap::new(),
            background,
            label,
        },
        db,
        &rt.jobs,
    )
    .map_err(|e| e.to_string())?;

    match dispatch {
        ExecDispatch::Completed(outcome) => {
            serde_json::to_string_pretty(&outcome).map_err(|e| e.to_string())
        }
        ExecDispatch::Background(job_id) => {
            serde_json::to_string_pretty(&json!({"job_id": job_id, "status": "running"}))
                .map_err(|e| e.to_string())
        }
    }
}

fn tool_index(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let path_opt = args.get("path").and_then(Value::as_str).map(PathBuf::from);
    let root = project_root_from(args, path_opt.as_deref())?;
    let cfg = load_config(&root)?;
    let db = rt.db_for(&root)?;
    let max_chunk_bytes = num_arg(args, "max_chunk_bytes").unwrap_or(1024) as usize;

    if let Some(p) = path_opt {
        let abs = if p.is_absolute() { p } else { root.join(p) };
        let summary = index_path_gitignore(&db, &abs, Some(&cfg), max_chunk_bytes)
            .map_err(|e| e.to_string())?;
        return serde_json::to_string_pretty(&summary).map_err(|e| e.to_string());
    }

    let label = args
        .get("label")
        .and_then(Value::as_str)
        .unwrap_or("inline:text");
    let (kind, content) = if let Some(html) = args.get("html").and_then(Value::as_str) {
        ("html", html_to_text(html))
    } else if let Some(text) = args.get("text").and_then(Value::as_str) {
        ("text", text.to_string())
    } else {
        return Err("provide 'path', 'html', or 'text'".into());
    };

    let lines: Vec<&str> = content.lines().collect();
    let chunks: Vec<StoredChunkInput> = lines
        .chunks(60)
        .enumerate()
        .map(|(i, grp)| StoredChunkInput {
            start_line: i * 60 + 1,
            end_line: i * 60 + grp.len(),
            symbol_path: None,
            kind: Some(kind.into()),
            content: grp.join("\n"),
        })
        .collect();
    let handle = db
        .store_spilled(label, kind, None, &content, chunks)
        .map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&handle).map_err(|e| e.to_string())
}

fn tool_search(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let query = string_arg(args, "query")?;
    let root = project_root_from(args, None)?;
    let db = rt.db_for(&root)?;
    let source_id = args
        .get("source_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let label = args
        .get("label")
        .and_then(Value::as_str)
        .map(str::to_string);
    let limit = num_arg(args, "limit").unwrap_or(10) as usize;

    let hits = search_index(
        &db,
        &SearchQuery {
            query,
            source_id,
            label,
            limit,
        },
    )
    .map_err(|e| e.to_string())?;
    let rendered = serde_json::to_string_pretty(&json!({
        "count": hits.len(),
        "hits": hits
    }))
    .map_err(|e| e.to_string())?;
    Ok(utf8_preview(&rendered, DEFAULT_REPLY_CAP_BYTES))
}

fn tool_resume(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let root = project_root_from(args, None)?;
    let db = rt.db_for(&root)?;
    let summary = build_resume_summary(&db).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&summary).map_err(|e| e.to_string())
}

fn tool_memory(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let action = string_arg(args, "action")?;
    let root = project_root_from(args, None)?;
    let db = rt.db_for(&root)?;
    let store = MemoryStore::new(&root);

    match action.as_str() {
        "read" => {
            let name = string_arg(args, "name")?;
            let content = store.read(&name).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&json!({"name": name, "content": content}))
                .map_err(|e| e.to_string())
        }
        "write" => {
            let name = string_arg(args, "name")?;
            let content = string_arg(args, "content")?;
            let entry = store
                .write(&name, &content, Some(&db))
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())
        }
        "list" => {
            let topic = args.get("topic").and_then(Value::as_str);
            let entries = store.list(topic).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&json!({"memories": entries})).map_err(|e| e.to_string())
        }
        "delete" => {
            let name = string_arg(args, "name")?;
            store.delete(&name, Some(&db)).map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&json!({"deleted": name})).map_err(|e| e.to_string())
        }
        "rename" => {
            let old_name = string_arg(args, "name")?;
            let new_name = string_arg(args, "new_name")?;
            let report = store
                .rename(&old_name, &new_name, Some(&db))
                .map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
        }
        "graph" => {
            let graph = store.reference_graph().map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&graph).map_err(|e| e.to_string())
        }
        other => Err(format!("unknown memory action '{other}'")),
    }
}

fn tool_batch(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let calls = args
        .get("calls")
        .and_then(Value::as_array)
        .ok_or("missing array argument 'calls'")?;
    let root = project_root_from(args, None)?;
    let db = rt.db_for(&root)?;

    let results: Vec<Value> = std::thread::scope(|scope| {
        let handles: Vec<_> = calls
            .iter()
            .map(|entry| {
                let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
                let sub_args = entry.get("arguments").cloned().unwrap_or_else(|| json!({}));
                scope.spawn(move || {
                    if name == "code_batch" {
                        return json!({"name": name, "ok": false, "error": "nested code_batch is disallowed"});
                    }
                    match run(name, &sub_args, rt) {
                        Ok(text) => {
                            let parsed: Value = match serde_json::from_str(&text) {
                                Ok(v) => v,
                                Err(_) => Value::String(text),
                            };
                            json!({"name": name, "ok": true, "result": parsed})
                        }
                        Err(err) => json!({"name": name, "ok": false, "error": err}),
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| json!({"ok": false, "error": "worker panicked"}))
            })
            .collect()
    });

    let rendered =
        serde_json::to_string_pretty(&json!({"results": results})).map_err(|e| e.to_string())?;
    bound_reply("batch", rendered, DEFAULT_REPLY_CAP_BYTES, &db)
}

fn tool_admin(args: &Value, rt: &ToolRuntime) -> Result<String, String> {
    let action = string_arg(args, "action")?;
    let root = project_root_from(args, None)?;
    let db = rt.db_for(&root)?;

    match action.as_str() {
        "stats" => {
            let report = db.stats_report().map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())
        }
        "purge" => {
            db.purge().map_err(|e| e.to_string())?;
            serde_json::to_string_pretty(&json!({"purged": true, "db": db.path().display().to_string()}))
                .map_err(|e| e.to_string())
        }
        "lsp_status" => {
            let status = rt.lsp.status();
            serde_json::to_string_pretty(&json!({"clients": status})).map_err(|e| e.to_string())
        }
        "doctor" => serde_json::to_string_pretty(&doctor_report(&root)).map_err(|e| e.to_string()),
        "upgrade" => serde_json::to_string_pretty(&json!({
            "version": env!("CARGO_PKG_VERSION"),
            "command": "cargo install --locked --git https://github.com/landigit/codemode codemode-cli"
        }))
        .map_err(|e| e.to_string()),
        other => Err(format!("unknown admin action '{other}'")),
    }
}

/// Builds a fast diagnostic report for `codemode doctor` and `code_admin(action="doctor")`.
pub fn doctor_report(root: &Path) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "project_root": canonical_or_lexical(root).display().to_string(),
        "context_db_path": context_db_path(root).display().to_string(),
        "languages_count": all_languages().len(),
        "runtimes": detect_runtimes(),
    })
}
