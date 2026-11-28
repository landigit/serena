//! Session event journal types and savings statistics.

use serde::{Deserialize, Serialize};

/// Event categories persisted across turns for compaction recovery and token accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    Prompt,
    Edit,
    Cmd,
    ToolErr,
    Index,
    Cost,
    Decision,
}

impl EventKind {
    /// Returns the canonical lowercase SQLite representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Edit => "edit",
            Self::Cmd => "cmd",
            Self::ToolErr => "toolerr",
            Self::Index => "index",
            Self::Cost => "cost",
            Self::Decision => "decision",
        }
    }

    /// Parses a persisted event kind string.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "prompt" => Some(Self::Prompt),
            "edit" => Some(Self::Edit),
            "cmd" => Some(Self::Cmd),
            "toolerr" => Some(Self::ToolErr),
            "index" => Some(Self::Index),
            "cost" => Some(Self::Cost),
            "decision" => Some(Self::Decision),
            _ => None,
        }
    }
}

/// Input payload for recording a session journal event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEvent {
    pub turn_number: u64,
    pub kind: EventKind,
    pub summary: String,
    pub detail: Option<String>,
    pub path: Option<String>,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

/// Stored event row retrieved from `context.db`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: i64,
    pub turn_number: u64,
    pub kind: EventKind,
    pub summary: String,
    pub detail: Option<String>,
    pub path: Option<String>,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub created_at: i64,
}

/// Aggregated context-savings report computed from stored sources and journal events.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StatsReport {
    pub source_count: usize,
    pub chunk_count: usize,
    pub event_count: usize,
    pub total_bytes_in: u64,
    pub total_bytes_out: u64,
    pub bytes_saved: u64,
    pub estimated_tokens_saved: u64,
}
