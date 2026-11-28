//! BM25/trigram search, gitignore-aware indexing, and compaction resume summaries.

use std::collections::HashSet;
use std::path::Path;

use codemode_core::config::ProjectConfig;
use codemode_store::{
    ChunkRecord, ContextDb, EventKind, JournalEvent, StoredChunkInput, utf8_preview,
};
use codemode_ts::chunk_file;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

use crate::RetrievalError;
use crate::html::html_to_text;

const MAX_SNIPPET_BYTES: usize = 800;

/// Search parameters for `search_index`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub source_id: Option<String>,
    pub label: Option<String>,
    pub limit: usize,
}

/// Ranked snippet hit returned from `search_index`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub source_id: String,
    pub label: String,
    pub start_line: usize,
    pub end_line: usize,
    pub symbol_path: Option<String>,
    pub score: f64,
    pub match_kind: String,
    pub snippet: String,
}

/// Summary of an indexing run over a file or directory tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSummary {
    pub files_indexed: usize,
    pub chunks_indexed: usize,
    pub bytes_indexed: usize,
    pub source_ids: Vec<String>,
}

/// Deterministic compaction recovery summary built from the session event journal.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResumeSummary {
    pub last_task: Option<String>,
    pub changed_files: Vec<String>,
    pub open_errors: Vec<String>,
    pub decisions: Vec<String>,
    pub recent_commands: Vec<String>,
    pub search_terms: Vec<String>,
}

/// Searches `db` via FTS5 BM25 first, falling back to trigram matching for typos/substrings.
pub fn search_index(db: &ContextDb, query: &SearchQuery) -> Result<Vec<SearchHit>, RetrievalError> {
    let limit = query.limit.clamp(1, 50);
    let fts_hits = db.search_fts(
        &query.query,
        query.source_id.as_deref(),
        query.label.as_deref(),
        limit,
    )?;

    let mut seen: HashSet<(String, usize, usize)> = HashSet::new();
    let mut results = Vec::new();
    for chunk in fts_hits {
        let key = (chunk.source_id.clone(), chunk.start_line, chunk.end_line);
        if seen.insert(key) {
            results.push(to_hit(&chunk, &query.query, "bm25"));
        }
    }

    if results.len() < limit {
        let remaining = limit - results.len();
        let tri_hits = db.search_trigrams(
            &query.query,
            query.source_id.as_deref(),
            query.label.as_deref(),
            remaining * 2,
        )?;
        for chunk in tri_hits {
            let key = (chunk.source_id.clone(), chunk.start_line, chunk.end_line);
            if seen.insert(key) {
                results.push(to_hit(&chunk, &query.query, "trigram"));
                if results.len() >= limit {
                    break;
                }
            }
        }
    }

    Ok(results)
}

fn to_hit(chunk: &ChunkRecord, query: &str, match_kind: &str) -> SearchHit {
    SearchHit {
        source_id: chunk.source_id.clone(),
        label: chunk.label.clone(),
        start_line: chunk.start_line,
        end_line: chunk.end_line,
        symbol_path: chunk.symbol_path.clone(),
        score: chunk.score,
        match_kind: match_kind.to_string(),
        snippet: extract_snippet(&chunk.content, query),
    }
}

fn extract_snippet(content: &str, query: &str) -> String {
    if content.len() <= MAX_SNIPPET_BYTES {
        return content.to_string();
    }
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_ascii_lowercase())
        .collect();
    let lines: Vec<&str> = content.lines().collect();
    let mut best_idx = 0usize;
    let mut best_score = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        let score = terms.iter().filter(|t| lower.contains(t.as_str())).count();
        if score > best_score {
            best_score = score;
            best_idx = idx;
        }
    }
    if best_score == 0 {
        let q_tris = codemode_store::extract_trigrams(query);
        for (idx, line) in lines.iter().enumerate() {
            let lower = line.to_ascii_lowercase();
            let score = q_tris
                .iter()
                .filter(|tri| lower.contains(tri.as_str()))
                .count();
            if score > best_score {
                best_score = score;
                best_idx = idx;
            }
        }
    }

    let start = best_idx.saturating_sub(3);
    let end = (best_idx + 8).min(lines.len());
    let window = lines[start..end].join("\n");
    utf8_preview(&window, MAX_SNIPPET_BYTES)
}

/// Indexes a file or directory into `db`, respecting `.gitignore` and `.codemode/config.toml`.
pub fn index_path_gitignore(
    db: &ContextDb,
    target: &Path,
    config: Option<&ProjectConfig>,
    max_chunk_bytes: usize,
) -> Result<IndexSummary, RetrievalError> {
    let mut files_indexed = 0usize;
    let mut chunks_indexed = 0usize;
    let mut bytes_indexed = 0usize;
    let mut source_ids = Vec::new();

    let extra_ignores: Vec<String> = config
        .map(|c| c.ignore.extra_patterns.clone())
        .unwrap_or_default();

    let walker = WalkBuilder::new(target)
        .hidden(true)
        .require_git(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(target)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_extra_ignored(&rel, &extra_ignores) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(raw_text) = String::from_utf8(bytes) else {
            continue;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        let is_html = matches!(ext.as_str(), "html" | "htm");
        let text = if is_html {
            html_to_text(&raw_text)
        } else {
            raw_text
        };
        if text.trim().is_empty() {
            continue;
        }

        let chunks: Vec<StoredChunkInput> = chunk_file(path, &text, max_chunk_bytes, config)
            .into_iter()
            .map(|c| StoredChunkInput {
                start_line: c.start_line,
                end_line: c.end_line,
                symbol_path: c.symbol_path,
                kind: c.kind,
                content: c.text,
            })
            .collect();

        let label = if rel.is_empty() {
            format!(
                "file:{}",
                path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
            )
        } else {
            format!("file:{rel}")
        };
        let source_id = format!("file_{}", blake3_short(&label));
        let path_str = path.display().to_string();
        let handle = db.upsert_source(
            &source_id,
            &label,
            if is_html { "html" } else { "file" },
            Some(&path_str),
            &text,
            chunks,
        )?;
        files_indexed += 1;
        chunks_indexed += handle.chunk_count;
        bytes_indexed += handle.total_bytes;
        source_ids.push(handle.source_id);
    }

    let _ = db.record_event(&JournalEvent {
        turn_number: 0,
        kind: EventKind::Index,
        summary: format!("Indexed {files_indexed} file(s) under {}", target.display()),
        detail: None,
        path: Some(target.display().to_string()),
        bytes_in: bytes_indexed as u64,
        bytes_out: (files_indexed * 64) as u64,
    });

    Ok(IndexSummary {
        files_indexed,
        chunks_indexed,
        bytes_indexed,
        source_ids,
    })
}

fn is_extra_ignored(rel: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        let p = pat.trim().trim_start_matches('/');
        if p.is_empty() {
            return false;
        }
        if let Some(dir) = p.strip_suffix('/') {
            rel.starts_with(&format!("{dir}/")) || rel.contains(&format!("/{dir}/"))
        } else if let Some(ext) = p.strip_prefix("*.") {
            rel.ends_with(&format!(".{ext}"))
        } else {
            rel == p || rel.ends_with(&format!("/{p}"))
        }
    })
}

fn blake3_short(input: &str) -> String {
    let hex = blake3::hash(input.as_bytes()).to_hex();
    hex[..12].to_string()
}

/// Builds a deterministic compaction recovery summary from recent session journal events.
pub fn build_resume_summary(db: &ContextDb) -> Result<ResumeSummary, RetrievalError> {
    let events = db.list_events(500)?;
    let mut summary = ResumeSummary::default();
    let mut seen_files = HashSet::new();
    let mut seen_decisions = HashSet::new();
    let mut seen_terms = HashSet::new();

    for ev in &events {
        match ev.kind {
            EventKind::Prompt => {
                summary.last_task = Some(ev.summary.clone());
                collect_terms(&ev.summary, &mut seen_terms, &mut summary.search_terms);
            }
            EventKind::Edit => {
                if let Some(p) = &ev.path
                    && seen_files.insert(p.clone())
                {
                    summary.changed_files.push(p.clone());
                }
                collect_terms(&ev.summary, &mut seen_terms, &mut summary.search_terms);
            }
            EventKind::ToolErr => {
                summary.open_errors.push(ev.summary.clone());
                collect_terms(&ev.summary, &mut seen_terms, &mut summary.search_terms);
            }
            EventKind::Decision => {
                if seen_decisions.insert(ev.summary.clone()) {
                    summary.decisions.push(ev.summary.clone());
                }
                collect_terms(&ev.summary, &mut seen_terms, &mut summary.search_terms);
            }
            EventKind::Cmd => {
                summary.recent_commands.push(ev.summary.clone());
            }
            EventKind::Index | EventKind::Cost => {}
        }
    }

    if summary.recent_commands.len() > 10 {
        let start = summary.recent_commands.len() - 10;
        summary.recent_commands = summary.recent_commands[start..].to_vec();
    }
    if summary.open_errors.len() > 10 {
        let start = summary.open_errors.len() - 10;
        summary.open_errors = summary.open_errors[start..].to_vec();
    }
    summary.search_terms.truncate(15);
    Ok(summary)
}

fn collect_terms(text: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    for raw in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        let tok = raw.trim();
        if tok.len() >= 4 {
            let lower = tok.to_ascii_lowercase();
            if seen.insert(lower) {
                out.push(tok.to_string());
            }
        }
    }
}
