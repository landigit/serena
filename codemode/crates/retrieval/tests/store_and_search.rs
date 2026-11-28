use std::path::Path;

use codemode_retrieval::{
    SearchQuery, build_resume_summary, html_to_text, index_path_gitignore, search_index,
};
use codemode_store::{ContextDb, EventKind, JournalEvent, StoredChunkInput};

#[test]
fn spills_200kb_output_and_reopens_for_bm25_and_trigram_search() {
    let base = std::env::temp_dir().join(format!("codemode-store-spill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let db_path = base.join("context.db");

    let mut big = String::new();
    for i in 0..5000 {
        if i == 2500 {
            big.push_str("line 2500: error in AuthenticationTokenValidator::refresh_session\n");
        } else {
            big.push_str(&format!("line {i}: normal worker heartbeat status ok\n"));
        }
    }
    assert!(big.len() > 200_000);

    let handle = {
        let db = ContextDb::open(&db_path).unwrap();
        let chunks: Vec<StoredChunkInput> = big
            .lines()
            .enumerate()
            .collect::<Vec<_>>()
            .chunks(100)
            .map(|group| {
                let start_line = group.first().unwrap().0 + 1;
                let end_line = group.last().unwrap().0 + 1;
                let content = group
                    .iter()
                    .map(|(_, line)| *line)
                    .collect::<Vec<_>>()
                    .join("\n");
                StoredChunkInput {
                    start_line,
                    end_line,
                    symbol_path: None,
                    kind: Some("log".into()),
                    content,
                }
            })
            .collect();
        db.store_spilled("cmd:cargo test", "exec", None, &big, chunks)
            .unwrap()
    };

    assert!(handle.preview.len() <= 2048);
    assert!(handle.total_bytes > 200_000);
    assert!(handle.token_count > 1000);
    assert_eq!(handle.chunk_count, 50);

    // Reopen from disk and search by source_id and label.
    let db = ContextDb::open(&db_path).unwrap();
    let hits = search_index(
        &db,
        &SearchQuery {
            query: "AuthenticationTokenValidator refresh_session".into(),
            source_id: Some(handle.source_id.clone()),
            label: Some("cmd:cargo test".into()),
            limit: 5,
        },
    )
    .unwrap();
    assert!(!hits.is_empty());
    assert!(hits[0].snippet.contains("AuthenticationTokenValidator"));
    assert!(hits[0].start_line <= 2501 && hits[0].end_line >= 2501);

    // Typo / substring trigram fallback: "AuthentcationTokenValidtor" (missing letters).
    let fuzzy = search_index(
        &db,
        &SearchQuery {
            query: "AuthentcationTokenValidtor".into(),
            source_id: Some(handle.source_id),
            label: None,
            limit: 5,
        },
    )
    .unwrap();
    assert!(
        !fuzzy.is_empty(),
        "expected trigram fallback to recover typo match"
    );
    assert!(fuzzy[0].snippet.contains("AuthenticationTokenValidator"));
}

#[test]
fn extracts_html_text_and_indexes_directory_respecting_gitignore() {
    let base = std::env::temp_dir().join(format!("codemode-retrieval-idx-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(repo.join("ignored_dir")).unwrap();
    std::fs::write(repo.join(".gitignore"), "ignored_dir/\n*.secret\n").unwrap();
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub fn compute_bm25_score() -> f64 {\n    42.0\n}\n",
    )
    .unwrap();
    std::fs::write(repo.join("ignored_dir/skip.rs"), "pub fn secret_fn() {}\n").unwrap();
    std::fs::write(repo.join("key.secret"), "TOP_SECRET_VALUE\n").unwrap();

    let html = "<html><head><style>.x{color:red}</style><script>alert(1)</script></head><body><h1>API Guide</h1><p>Use <code>code_search</code> to query FTS5.</p></body></html>";
    let text = html_to_text(html);
    assert!(text.contains("API Guide"));
    assert!(text.contains("code_search"));
    assert!(!text.contains("alert(1)"));
    assert!(!text.contains("color:red"));

    let db = ContextDb::open(&base.join("context.db")).unwrap();
    let indexed = index_path_gitignore(&db, &repo, None, 512).unwrap();
    assert_eq!(indexed.files_indexed, 1);

    let hits = search_index(
        &db,
        &SearchQuery {
            query: "compute_bm25_score".into(),
            source_id: None,
            label: None,
            limit: 5,
        },
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start_line, 1);

    let secret_hits = search_index(
        &db,
        &SearchQuery {
            query: "TOP_SECRET_VALUE".into(),
            source_id: None,
            label: None,
            limit: 5,
        },
    )
    .unwrap();
    assert!(secret_hits.is_empty());
}

#[test]
fn records_all_event_kinds_computes_stats_and_builds_resume_summary() {
    let base = std::env::temp_dir().join(format!("codemode-journal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let db = ContextDb::open(&base.join("context.db")).unwrap();

    let events = [
        (
            EventKind::Prompt,
            1,
            "Implement FTS5 spill store",
            None,
            200,
            0,
        ),
        (
            EventKind::Index,
            1,
            "Indexed src/schema.rs",
            Some("src/schema.rs"),
            12000,
            600,
        ),
        (
            EventKind::Edit,
            2,
            "Updated schema migrations",
            Some("src/schema.rs"),
            4000,
            500,
        ),
        (
            EventKind::Cmd,
            2,
            "cargo test -p codemode-store",
            None,
            50000,
            1200,
        ),
        (
            EventKind::ToolErr,
            2,
            "missing trigram index",
            Some("src/bm25.rs"),
            800,
            400,
        ),
        (EventKind::Cost, 3, "turn budget", None, 10000, 1000),
        (
            EventKind::Decision,
            3,
            "Use porter unicode61 plus trigram table",
            None,
            300,
            150,
        ),
    ];

    for (kind, turn, summary, path, bytes_in, bytes_out) in events {
        db.record_event(&JournalEvent {
            turn_number: turn,
            kind,
            summary: summary.into(),
            detail: None,
            path: path.map(str::to_string),
            bytes_in,
            bytes_out,
        })
        .unwrap();
    }

    let stats = db.stats_report().unwrap();
    assert_eq!(stats.event_count, 7);
    assert_eq!(stats.total_bytes_in, 77_300);
    assert_eq!(stats.total_bytes_out, 3_850);
    assert_eq!(stats.bytes_saved, 73_450);

    let resume = build_resume_summary(&db).unwrap();
    assert_eq!(
        resume.last_task.as_deref(),
        Some("Implement FTS5 spill store")
    );
    assert!(resume.changed_files.contains(&"src/schema.rs".to_string()));
    assert!(resume.open_errors.iter().any(|e| e.contains("trigram")));
    assert!(
        resume
            .decisions
            .iter()
            .any(|d| d.contains("porter unicode61"))
    );
    assert!(!resume.search_terms.is_empty());
    let _ = Path::new(".");
}
