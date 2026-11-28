//! Local SQLite FTS5 + trigram context store and session journal.

pub mod events;
pub mod schema;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use codemode_core::paths::context_db_path;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub use events::{EventKind, JournalEvent, StatsReport, StoredEvent};

static SOURCE_SEQ: AtomicU64 = AtomicU64::new(1);
const MAX_PREVIEW_BYTES: usize = 2048;

/// Errors raised by SQLite storage operations.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cannot prepare database directory {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Metadata describing an indexed source or spilled command output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRecord {
    pub source_id: String,
    pub label: String,
    pub kind: String,
    pub path: Option<String>,
    pub total_bytes: usize,
    pub token_count: usize,
    pub chunk_count: usize,
    pub created_at: i64,
}

/// Caller-supplied chunk ready for FTS5 and trigram insertion.
#[derive(Debug, Clone)]
pub struct StoredChunkInput {
    pub start_line: usize,
    pub end_line: usize,
    pub symbol_path: Option<String>,
    pub kind: Option<String>,
    pub content: String,
}

/// Persisted chunk row returned from search queries.
#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub id: i64,
    pub source_id: String,
    pub label: String,
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub symbol_path: Option<String>,
    pub kind: Option<String>,
    pub token_count: usize,
    pub content: String,
    pub score: f64,
}

/// Bounded handle returned when an oversized payload is spilled into `context.db`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpillHandle {
    pub source_id: String,
    pub label: String,
    pub preview: String,
    pub total_bytes: usize,
    pub token_count: usize,
    pub chunk_count: usize,
}

/// Thread-safe handle to `$XDG_DATA_HOME/codemode/<hash>/context.db`.
pub struct ContextDb {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl ContextDb {
    /// Opens (or creates and migrates) a `context.db` at `path`.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|source| StoreError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        schema::apply_migrations(&conn)?;
        Ok(Self {
            path: path.to_path_buf(),
            conn: Mutex::new(conn),
        })
    }

    /// Opens the per-project `context.db` under `$XDG_DATA_HOME/codemode/<blake3>/context.db`.
    pub fn open_for_project(project_root: &Path) -> Result<Self, StoreError> {
        Self::open(&context_db_path(project_root))
    }

    /// Returns the filesystem path backing this database.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Stores a spilled output with a newly generated `source_id` and returns a `<= 2048`-byte handle.
    pub fn store_spilled(
        &self,
        label: &str,
        kind: &str,
        path: Option<&str>,
        full_text: &str,
        chunks: Vec<StoredChunkInput>,
    ) -> Result<SpillHandle, StoreError> {
        let seq = SOURCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let now_nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut hasher = blake3::Hasher::new();
        hasher.update(label.as_bytes());
        hasher.update(&now_nanos.to_le_bytes());
        hasher.update(&seq.to_le_bytes());
        hasher.update(full_text.as_bytes());
        let hex = hasher.finalize().to_hex();
        let source_id = format!("src_{}", &hex[..12]);
        self.upsert_source(&source_id, label, kind, path, full_text, chunks)
    }

    /// Inserts or replaces a source and its chunks/trigrams atomically.
    pub fn upsert_source(
        &self,
        source_id: &str,
        label: &str,
        kind: &str,
        path: Option<&str>,
        full_text: &str,
        chunks: Vec<StoredChunkInput>,
    ) -> Result<SpillHandle, StoreError> {
        let normalized_chunks = if chunks.is_empty() && !full_text.is_empty() {
            let line_count = full_text.lines().count().max(1);
            vec![StoredChunkInput {
                start_line: 1,
                end_line: line_count,
                symbol_path: None,
                kind: Some(kind.to_string()),
                content: full_text.to_string(),
            }]
        } else {
            chunks
        };

        let total_bytes = full_text.len();
        let token_count = estimate_tokens(full_text);
        let chunk_count = normalized_chunks.len();
        let now = unix_secs();

        let mut conn = self.conn.lock().expect("sqlite mutex poisoned");
        let tx = conn.transaction()?;

        // Remove previous chunks and trigrams for idempotent re-indexing.
        tx.execute(
            "DELETE FROM chunks_trigram WHERE chunk_id IN (SELECT id FROM chunks WHERE source_id = ?1)",
            [source_id],
        )?;
        tx.execute("DELETE FROM chunks WHERE source_id = ?1", [source_id])?;
        tx.execute("DELETE FROM sources WHERE source_id = ?1", [source_id])?;

        tx.execute(
            "INSERT INTO sources(source_id, label, kind, path, total_bytes, token_count, chunk_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                source_id,
                label,
                kind,
                path,
                total_bytes as i64,
                token_count as i64,
                chunk_count as i64,
                now
            ],
        )?;

        {
            let mut insert_chunk = tx.prepare(
                "INSERT INTO chunks(source_id, label, chunk_index, start_line, end_line, symbol_path, kind, token_count, content)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            let mut insert_tri =
                tx.prepare("INSERT INTO chunks_trigram(chunk_id, trigram) VALUES (?1, ?2)")?;

            for (idx, chunk) in normalized_chunks.iter().enumerate() {
                let chunk_tokens = estimate_tokens(&chunk.content);
                insert_chunk.execute(params![
                    source_id,
                    label,
                    idx as i64,
                    chunk.start_line as i64,
                    chunk.end_line as i64,
                    chunk.symbol_path.as_deref(),
                    chunk.kind.as_deref(),
                    chunk_tokens as i64,
                    chunk.content
                ])?;
                let chunk_id = tx.last_insert_rowid();
                let mut tri_text = chunk.content.clone();
                if let Some(sym) = &chunk.symbol_path {
                    tri_text.push(' ');
                    tri_text.push_str(sym);
                }
                for tri in extract_trigrams(&tri_text) {
                    insert_tri.execute(params![chunk_id, tri])?;
                }
            }
        }

        tx.commit()?;

        Ok(SpillHandle {
            source_id: source_id.to_string(),
            label: label.to_string(),
            preview: utf8_preview(full_text, MAX_PREVIEW_BYTES),
            total_bytes,
            token_count,
            chunk_count,
        })
    }

    /// Fetches source metadata by `source_id`.
    pub fn get_source(&self, source_id: &str) -> Result<Option<SourceRecord>, StoreError> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        conn.query_row(
            "SELECT source_id, label, kind, path, total_bytes, token_count, chunk_count, created_at
             FROM sources WHERE source_id = ?1",
            [source_id],
            |row| {
                Ok(SourceRecord {
                    source_id: row.get(0)?,
                    label: row.get(1)?,
                    kind: row.get(2)?,
                    path: row.get(3)?,
                    total_bytes: row.get::<_, i64>(4)? as usize,
                    token_count: row.get::<_, i64>(5)? as usize,
                    chunk_count: row.get::<_, i64>(6)? as usize,
                    created_at: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(StoreError::from)
    }

    /// Queries chunks using FTS5 BM25 ranking with optional `source_id` and `label` filters.
    pub fn search_fts(
        &self,
        query: &str,
        source_id: Option<&str>,
        label: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChunkRecord>, StoreError> {
        let fts_expr = build_fts_query(query);
        if fts_expr.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT c.id, c.source_id, c.label, c.chunk_index, c.start_line, c.end_line,
                    c.symbol_path, c.kind, c.token_count, c.content, bm25(chunks_fts) AS rank
             FROM chunks_fts
             JOIN chunks c ON chunks_fts.rowid = c.id
             WHERE chunks_fts MATCH ?1
               AND (?2 IS NULL OR c.source_id = ?2)
               AND (?3 IS NULL OR c.label = ?3 OR c.label LIKE ?3 || '%')
             ORDER BY rank ASC
             LIMIT ?4",
        )?;

        let rows = stmt.query_map(params![fts_expr, source_id, label, limit as i64], |row| {
            let rank: f64 = row.get(10)?;
            Ok(ChunkRecord {
                id: row.get(0)?,
                source_id: row.get(1)?,
                label: row.get(2)?,
                chunk_index: row.get::<_, i64>(3)? as usize,
                start_line: row.get::<_, i64>(4)? as usize,
                end_line: row.get::<_, i64>(5)? as usize,
                symbol_path: row.get(6)?,
                kind: row.get(7)?,
                token_count: row.get::<_, i64>(8)? as usize,
                content: row.get(9)?,
                score: -rank,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Queries chunks by trigram overlap for typo/substring fallback.
    pub fn search_trigrams(
        &self,
        query: &str,
        source_id: Option<&str>,
        label: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChunkRecord>, StoreError> {
        let trigrams = extract_trigrams(query);
        if trigrams.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut counts: HashMap<i64, usize> = HashMap::new();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT t.chunk_id
             FROM chunks_trigram t
             JOIN chunks c ON c.id = t.chunk_id
             WHERE t.trigram = ?1
               AND (?2 IS NULL OR c.source_id = ?2)
               AND (?3 IS NULL OR c.label = ?3 OR c.label LIKE ?3 || '%')",
        )?;

        for tri in &trigrams {
            let mut rows = stmt.query(params![tri, source_id, label])?;
            while let Some(row) = rows.next()? {
                let chunk_id: i64 = row.get(0)?;
                *counts.entry(chunk_id).or_insert(0) += 1;
            }
        }

        if counts.is_empty() {
            return Ok(Vec::new());
        }

        let min_overlap = (trigrams.len() / 3).max(1);
        let mut ranked: Vec<(i64, usize)> = counts
            .into_iter()
            .filter(|(_, hits)| *hits >= min_overlap)
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(limit);

        let mut get_chunk = conn.prepare(
            "SELECT id, source_id, label, chunk_index, start_line, end_line,
                    symbol_path, kind, token_count, content
             FROM chunks WHERE id = ?1",
        )?;
        let total_q = trigrams.len() as f64;
        let mut out = Vec::with_capacity(ranked.len());
        for (chunk_id, hits) in ranked {
            if let Some(mut chunk) = get_chunk
                .query_row([chunk_id], |row| {
                    Ok(ChunkRecord {
                        id: row.get(0)?,
                        source_id: row.get(1)?,
                        label: row.get(2)?,
                        chunk_index: row.get::<_, i64>(3)? as usize,
                        start_line: row.get::<_, i64>(4)? as usize,
                        end_line: row.get::<_, i64>(5)? as usize,
                        symbol_path: row.get(6)?,
                        kind: row.get(7)?,
                        token_count: row.get::<_, i64>(8)? as usize,
                        content: row.get(9)?,
                        score: hits as f64 / total_q,
                    })
                })
                .optional()?
            {
                chunk.score = hits as f64 / total_q;
                out.push(chunk);
            }
        }
        Ok(out)
    }

    /// Appends one `JournalEvent` to the session journal.
    pub fn record_event(&self, event: &JournalEvent) -> Result<i64, StoreError> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let now = unix_secs();
        conn.execute(
            "INSERT INTO events(turn_number, kind, summary, detail, path, bytes_in, bytes_out, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.turn_number as i64,
                event.kind.as_str(),
                event.summary,
                event.detail.as_deref(),
                event.path.as_deref(),
                event.bytes_in as i64,
                event.bytes_out as i64,
                now
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Returns up to `limit` most recent journal events (ordered oldest-to-newest).
    pub fn list_events(&self, limit: usize) -> Result<Vec<StoredEvent>, StoreError> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, turn_number, kind, summary, detail, path, bytes_in, bytes_out, created_at
             FROM events ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |row| {
            let kind_str: String = row.get(2)?;
            Ok(StoredEvent {
                id: row.get(0)?,
                turn_number: row.get::<_, i64>(1)? as u64,
                kind: EventKind::parse(&kind_str).unwrap_or(EventKind::Cmd),
                summary: row.get(3)?,
                detail: row.get(4)?,
                path: row.get(5)?,
                bytes_in: row.get::<_, i64>(6)? as u64,
                bytes_out: row.get::<_, i64>(7)? as u64,
                created_at: row.get(8)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out.reverse();
        Ok(out)
    }

    /// Computes cumulative byte and estimated token savings across stored events and sources.
    pub fn stats_report(&self) -> Result<StatsReport, StoreError> {
        let conn = self.conn.lock().expect("sqlite mutex poisoned");
        let source_count: i64 = conn.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0))?;
        let chunk_count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
        let (event_count, total_in, total_out): (i64, i64, i64) = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(bytes_in), 0), COALESCE(SUM(bytes_out), 0) FROM events",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let total_bytes_in = total_in.max(0) as u64;
        let total_bytes_out = total_out.max(0) as u64;
        let bytes_saved = total_bytes_in.saturating_sub(total_bytes_out);
        Ok(StatsReport {
            source_count: source_count.max(0) as usize,
            chunk_count: chunk_count.max(0) as usize,
            event_count: event_count.max(0) as usize,
            total_bytes_in,
            total_bytes_out,
            bytes_saved,
            estimated_tokens_saved: bytes_saved / 4,
        })
    }

    /// Clears indexed sources, chunks, trigrams, and journal events.
    pub fn purge(&self) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().expect("sqlite mutex poisoned");
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM chunks_trigram", [])?;
        tx.execute("DELETE FROM chunks", [])?;
        tx.execute("DELETE FROM sources", [])?;
        tx.execute("DELETE FROM events", [])?;
        tx.commit()?;
        Ok(())
    }
}

/// Estimates token count deterministically without heavyweight BPE tables.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.split_whitespace().count().max(text.len().div_ceil(4))
}

/// Truncates `text` to at most `max_bytes` on a UTF-8 char boundary.
pub fn utf8_preview(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let marker = "\n...[truncated]";
    let budget = max_bytes.saturating_sub(marker.len());
    let mut end = budget.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + marker.len());
    out.push_str(&text[..end]);
    out.push_str(marker);
    out
}

/// Extracts unique lowercased 3-char trigrams for fuzzy/substring matching.
pub fn extract_trigrams(text: &str) -> Vec<String> {
    let normalized: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for token in normalized.split_whitespace() {
        let chars: Vec<char> = token.chars().collect();
        if chars.len() < 3 {
            continue;
        }
        for win in chars.windows(3) {
            let tri: String = win.iter().collect();
            if seen.insert(tri.clone()) {
                out.push(tri);
            }
        }
    }
    out
}

fn build_fts_query(raw: &str) -> String {
    let tokens: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|tok| !tok.is_empty())
        .map(|tok| format!("\"{}\"", tok.replace('"', "")))
        .collect();
    tokens.join(" OR ")
}

fn unix_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
