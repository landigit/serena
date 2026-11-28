//! Lazy per-worktree LSP multiplexer, `documentSymbol` fallback, and fast diagnostics.

pub mod client;
pub mod diagnostics;
pub mod document_symbol;

use std::path::PathBuf;
use std::time::Duration;

pub use client::{
    Diagnostic, Location, LspClient, LspClientStatus, LspManager, byte_of, evict_idle_clients,
    file_uri, position_of, uri_to_path,
};
pub use diagnostics::{
    diagnostic_output, discover_project_files, group_diagnostics, group_entries, owner_name_path,
    severity_label, severity_name,
};
pub use document_symbol::{document_symbols_outline, lsp_symbol_kind_label};

/// Errors produced by LSP lifecycle management and JSON-RPC queries.
#[derive(Debug, thiserror::Error)]
pub enum LspError {
    #[error("unsupported file type: {0}")]
    UnsupportedFile(PathBuf),
    #[error("{0}")]
    ServerUnavailable(String),
    #[error("language server exited")]
    ServerExited,
    #[error("language server did not answer {method} in {timeout:?}")]
    Timeout { method: String, timeout: Duration },
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0}")]
    Protocol(String),
}
