//! Tree-sitter structural parser, symbol lookup, body replacement, and chunking.

pub mod chunk;
pub mod grammars;
pub mod outline;

use std::path::{Path, PathBuf};

use codemode_core::config::ProjectConfig;
use codemode_core::languages::{LanguageId, resolve_language};
use serde::Serialize;
use similar::TextDiff;

pub use chunk::{TextChunk, chunk_file};
pub use grammars::grammar_for;
pub use outline::{
    PathComponent, Symbol, best_match_in_symbols, collect_symbols, finalize_paths, find_in_symbols,
    flatten, match_name_path, pattern_components,
};

/// Errors produced by structural parsing and symbol editing.
#[derive(Debug, thiserror::Error)]
pub enum TsError {
    #[error("unsupported file type: {0}")]
    UnknownLanguage(PathBuf),
    #[error("no compiled tree-sitter grammar for {0:?}")]
    UnsupportedGrammar(LanguageId),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{0} is not valid UTF-8")]
    InvalidUtf8(PathBuf),
    #[error("tree-sitter failed to parse {0}")]
    ParseFailed(PathBuf),
    #[error("no symbol matching '{pattern}' in {path}")]
    SymbolNotFound { pattern: String, path: PathBuf },
}

/// Parsed source file and extracted symbol hierarchy.
#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub lang: LanguageId,
    pub source: String,
    pub symbols: Vec<Symbol>,
}

/// Serializable outline response for a file.
#[derive(Debug, Clone, Serialize)]
pub struct OutlineResult {
    pub file: String,
    pub language: String,
    pub symbols: Vec<Symbol>,
}

/// Result of replacing one symbol's byte range.
#[derive(Debug, Clone, Serialize)]
pub struct ReplaceOutcome {
    pub symbol: String,
    pub line: usize,
    pub end_line: usize,
    pub changed: bool,
    pub diff: String,
}

/// Parses a file from disk using tree-sitter.
pub fn parse_file(path: &Path, config: Option<&ProjectConfig>) -> Result<ParsedFile, TsError> {
    let bytes = std::fs::read(path).map_err(|source| TsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let source = String::from_utf8(bytes).map_err(|_| TsError::InvalidUtf8(path.to_path_buf()))?;
    parse_source(path, &source, config)
}

/// Parses an in-memory UTF-8 source buffer associated with `path`.
pub fn parse_source(
    path: &Path,
    source: &str,
    config: Option<&ProjectConfig>,
) -> Result<ParsedFile, TsError> {
    let lang = resolve_language(path, config)
        .ok_or_else(|| TsError::UnknownLanguage(path.to_path_buf()))?;
    let grammar = grammar_for(lang, path).ok_or(TsError::UnsupportedGrammar(lang))?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&grammar)
        .map_err(|_| TsError::ParseFailed(path.to_path_buf()))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| TsError::ParseFailed(path.to_path_buf()))?;

    let symbols = collect_symbols(tree.root_node(), source);
    Ok(ParsedFile {
        lang,
        source: source.to_string(),
        symbols,
    })
}

/// Returns all symbols matching `pattern` in `parsed`.
pub fn find_symbols<'a>(parsed: &'a ParsedFile, pattern: &str, substring: bool) -> Vec<&'a Symbol> {
    find_in_symbols(&parsed.symbols, pattern, substring)
}

/// Returns the best matching symbol for `pattern` in `parsed`.
pub fn best_match<'a>(
    parsed: &'a ParsedFile,
    pattern: &str,
    substring: bool,
) -> Option<&'a Symbol> {
    best_match_in_symbols(&parsed.symbols, pattern, substring)
}

/// Extracts a symbol's source body truncated at `max_chars`.
pub fn symbol_body(parsed: &ParsedFile, symbol: &Symbol, max_chars: usize) -> (String, bool) {
    let raw = &parsed.source[symbol.start_byte..symbol.end_byte];
    let total = raw.chars().count();
    if total <= max_chars {
        return (raw.to_string(), false);
    }
    let cut: String = raw.chars().take(max_chars).collect();
    (cut, true)
}

/// Replaces one matched symbol's body on disk (unless `dry_run` is true) and returns a unified diff.
pub fn replace_symbol_body(
    path: &Path,
    pattern: &str,
    content: &str,
    dry_run: bool,
    config: Option<&ProjectConfig>,
) -> Result<ReplaceOutcome, TsError> {
    let parsed = parse_file(path, config)?;
    let symbol = best_match(&parsed, pattern, false).ok_or_else(|| TsError::SymbolNotFound {
        pattern: pattern.to_string(),
        path: path.to_path_buf(),
    })?;
    let old = parsed.source.clone();
    let mut new = old.clone();
    new.replace_range(symbol.start_byte..symbol.end_byte, content);
    let diff = unified_diff(path, &old, &new);
    if !dry_run {
        std::fs::write(path, &new).map_err(|source| TsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(ReplaceOutcome {
        symbol: symbol.name_path.clone(),
        line: symbol.line,
        end_line: symbol.end_line,
        changed: !dry_run,
        diff,
    })
}

/// Builds a compact unified diff headered with `path`.
pub fn unified_diff(path: &Path, old: &str, new: &str) -> String {
    let name = path.display().to_string();
    TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&name, &name)
        .to_string()
}

/// Returns a trimmed single-line preview (up to 200 chars) for 0-based `line0`.
pub fn line_preview(source: &str, line0: usize) -> String {
    source
        .lines()
        .nth(line0)
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect()
}
