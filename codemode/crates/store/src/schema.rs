//! SQLite schema and migrations for `codemode` FTS5 + trigram context store.

use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 1;

/// Initializes pragmas and runs idempotent schema migrations.
pub fn apply_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA busy_timeout = 5000;
        PRAGMA foreign_keys = ON;

        CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            applied_at INTEGER NOT NULL
        );
        ",
    )?;

    let current: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current < 1 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sources (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL UNIQUE,
                label TEXT NOT NULL,
                kind TEXT NOT NULL,
                path TEXT,
                total_bytes INTEGER NOT NULL,
                token_count INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS chunks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL,
                label TEXT NOT NULL,
                chunk_index INTEGER NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                symbol_path TEXT,
                kind TEXT,
                token_count INTEGER NOT NULL,
                content TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_source ON chunks(source_id);
            CREATE INDEX IF NOT EXISTS idx_chunks_label ON chunks(label);

            CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                content,
                symbol_path,
                label,
                source_id UNINDEXED,
                content='chunks',
                content_rowid='id',
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
                INSERT INTO chunks_fts(rowid, content, symbol_path, label, source_id)
                VALUES (new.id, new.content, COALESCE(new.symbol_path, ''), new.label, new.source_id);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, content, symbol_path, label, source_id)
                VALUES ('delete', old.id, old.content, COALESCE(old.symbol_path, ''), old.label, old.source_id);
            END;

            CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, content, symbol_path, label, source_id)
                VALUES ('delete', old.id, old.content, COALESCE(old.symbol_path, ''), old.label, old.source_id);
                INSERT INTO chunks_fts(rowid, content, symbol_path, label, source_id)
                VALUES (new.id, new.content, COALESCE(new.symbol_path, ''), new.label, new.source_id);
            END;

            CREATE TABLE IF NOT EXISTS chunks_trigram (
                chunk_id INTEGER NOT NULL,
                trigram TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_chunks_trigram_tri ON chunks_trigram(trigram);
            CREATE INDEX IF NOT EXISTS idx_chunks_trigram_chunk ON chunks_trigram(chunk_id);

            CREATE TABLE IF NOT EXISTS events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                turn_number INTEGER NOT NULL,
                kind TEXT NOT NULL,
                summary TEXT NOT NULL,
                detail TEXT,
                path TEXT,
                bytes_in INTEGER NOT NULL,
                bytes_out INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_events_turn ON events(turn_number);
            CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
            ",
        )?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version, applied_at) VALUES (1, ?1)",
            [now],
        )?;
    }

    Ok(())
}
