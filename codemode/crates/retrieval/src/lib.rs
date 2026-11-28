//! Clean-room BM25 + trigram search, HTML extraction, gitignore indexing, and resume summaries.

pub mod bm25;
pub mod html;

pub use bm25::{
    IndexSummary, ResumeSummary, SearchHit, SearchQuery, build_resume_summary,
    index_path_gitignore, search_index,
};
pub use html::html_to_text;

/// Errors returned by retrieval and indexing workflows.
#[derive(Debug, thiserror::Error)]
pub enum RetrievalError {
    #[error("store error: {0}")]
    Store(#[from] codemode_store::StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
