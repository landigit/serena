//! Post-tool output throttling and SQLite FTS5 spill-on-overflow handling.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use codemode_store::{ContextDb, EventKind, JournalEvent, StoredChunkInput, utf8_preview};

const DEFAULT_INLINE_CAP: usize = 6144;
const CHUNK_LINES: usize = 80;

static REPEAT_COUNTS: OnceLock<Mutex<HashMap<u64, usize>>> = OnceLock::new();

/// Outcome of handling a post-tool output payload synchronously.
#[derive(Debug, Clone)]
pub struct SpoolOutcome {
    pub throttled: bool,
    pub spilled_source_id: Option<String>,
    pub output: String,
}

/// Throttles repeated noisy tool outputs and spills oversized outputs into `ContextDb`.
pub fn spool_post_tool(
    tool_name: &str,
    raw_output: &str,
    inline_cap_bytes: usize,
    db: Option<&ContextDb>,
) -> Result<SpoolOutcome, codemode_store::StoreError> {
    let cap = if inline_cap_bytes == 0 {
        DEFAULT_INLINE_CAP
    } else {
        inline_cap_bytes
    };

    let digest = hash_u64(tool_name, raw_output);
    let seen_count = {
        let map = REPEAT_COUNTS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut guard = map.lock().expect("throttle map poisoned");
        if guard.len() > 2048 {
            guard.clear();
        }
        let entry = guard.entry(digest).or_insert(0);
        *entry += 1;
        *entry
    };

    if seen_count > 1 && raw_output.len() > cap {
        return Ok(SpoolOutcome {
            throttled: true,
            spilled_source_id: None,
            output: format!(
                "[throttled repeated {tool_name} output ({} bytes, repeat #{seen_count}); use code_search for prior indexed slice]",
                raw_output.len()
            ),
        });
    }

    if raw_output.len() <= cap {
        return Ok(SpoolOutcome {
            throttled: false,
            spilled_source_id: None,
            output: raw_output.to_string(),
        });
    }

    let Some(db) = db else {
        return Ok(SpoolOutcome {
            throttled: false,
            spilled_source_id: None,
            output: utf8_preview(raw_output, cap),
        });
    };

    let lines: Vec<&str> = raw_output.lines().collect();
    let chunks: Vec<StoredChunkInput> = lines
        .chunks(CHUNK_LINES)
        .enumerate()
        .map(|(idx, group)| {
            let start_line = idx * CHUNK_LINES + 1;
            let end_line = start_line + group.len() - 1;
            StoredChunkInput {
                start_line,
                end_line,
                symbol_path: None,
                kind: Some("hook_spool".into()),
                content: group.join("\n"),
            }
        })
        .collect();

    let label = format!("hook:{tool_name}");
    let handle = db.store_spilled(&label, "hook", None, raw_output, chunks)?;
    let bounded = utf8_preview(
        &format!(
            "[spilled {} bytes -> source_id={} ({} chunks); query with code_search]\n{}",
            handle.total_bytes, handle.source_id, handle.chunk_count, handle.preview
        ),
        cap.max(256),
    );
    let _ = db.record_event(&JournalEvent {
        turn_number: 0,
        kind: EventKind::Index,
        summary: format!("Spooled {tool_name} output as {}", handle.source_id),
        detail: None,
        path: None,
        bytes_in: handle.total_bytes as u64,
        bytes_out: bounded.len() as u64,
    });

    Ok(SpoolOutcome {
        throttled: false,
        spilled_source_id: Some(handle.source_id),
        output: bounded,
    })
}

fn hash_u64(tool_name: &str, content: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in tool_name.bytes().chain([0xff]).chain(content.bytes()) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}
