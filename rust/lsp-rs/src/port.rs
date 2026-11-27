//! The Rust port of `src/serena/lsp/`.
//!
//! Replace each `unimplemented!` with the port of the source function. Return `Err` where
//! the source throws or raises: the caller gets an error there too. After changing a
//! signature, run `rust-anything refresh` to regenerate the bindings. `rust-anything diff` checks
//! behavior against what the source recorded.

#![forbid(unsafe_code)]

/// Port of class `GroupedDiagnostics` (`src/serena/lsp/lsp_diagnostics.py:95`): holds the instance state.
pub struct GroupedDiagnostics {}

impl GroupedDiagnostics {
    /// Port of `GroupedDiagnostics` constructor (`src/serena/lsp/lsp_diagnostics.py:95`).
    pub fn new() -> Result<Self, String> {
        unimplemented!("port the `GroupedDiagnostics` constructor")
    }

    /// Port of `GroupedDiagnostics.add` (`src/serena/lsp/lsp_diagnostics.py:99`).
    pub fn add(
        &mut self,
        relative_path: String,
        name_path: String,
        diagnostic: serde_json::Value,
    ) -> Result<(), String> {
        let _ = (&relative_path, &name_path, &diagnostic);
        unimplemented!("port `GroupedDiagnostics.add`")
    }

    /// Port of `GroupedDiagnostics.get_dict` (`src/serena/lsp/lsp_diagnostics.py:105`).
    pub fn get_dict(&mut self) -> Result<serde_json::Value, String> {
        unimplemented!("port `GroupedDiagnostics.get_dict`")
    }
}

/// Port of class `DiagnosticsDiff` (`src/serena/lsp/lsp_diagnostics.py:151`): holds the instance state.
pub struct DiagnosticsDiff {}

impl DiagnosticsDiff {
    /// Port of `DiagnosticsDiff` constructor (`src/serena/lsp/lsp_diagnostics.py:151`).
    pub fn new(
        before_snapshot: serde_json::Value,
        edited_files: Vec<serde_json::Value>,
        symbol_retriever: serde_json::Value,
    ) -> Result<Self, String> {
        let _ = (&before_snapshot, &edited_files, &symbol_retriever);
        unimplemented!("port the `DiagnosticsDiff` constructor")
    }

    /// Port of `DiagnosticsDiff.get_grouped_diagnostics` (`src/serena/lsp/lsp_diagnostics.py:206`).
    pub fn get_grouped_diagnostics(&mut self) -> Result<serde_json::Value, String> {
        unimplemented!("port `DiagnosticsDiff.get_grouped_diagnostics`")
    }
}
