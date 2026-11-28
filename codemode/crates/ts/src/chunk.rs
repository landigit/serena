//! Symbol-aligned and line-bounded source chunking for FTS5 indexing.

use std::path::Path;

use codemode_core::config::ProjectConfig;

use crate::outline::Symbol;
use crate::parse_source;

/// A bounded, line-indexed slice of a document or source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub start_line: usize,
    pub end_line: usize,
    pub symbol_path: Option<String>,
    pub kind: Option<String>,
    pub text: String,
}

/// Splits `source` into bounded chunks along tree-sitter symbol boundaries when available.
pub fn chunk_file(
    path: &Path,
    source: &str,
    max_chunk_bytes: usize,
    config: Option<&ProjectConfig>,
) -> Vec<TextChunk> {
    let max_bytes = max_chunk_bytes.max(64);
    let mut lines: Vec<&str> = source.lines().collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return Vec::new();
    }

    let symbols = parse_source(path, source, config)
        .map(|parsed| parsed.symbols)
        .unwrap_or_default();

    if symbols.is_empty() {
        return chunk_line_range(&lines, 1, lines.len(), None, None, max_bytes);
    }

    chunk_symbols(&lines, &symbols, max_bytes)
}

fn chunk_symbols(lines: &[&str], symbols: &[Symbol], max_bytes: usize) -> Vec<TextChunk> {
    let mut out = Vec::new();
    let mut cursor_line = 1usize;

    for sym in symbols {
        let start = sym.line.max(cursor_line);
        let end = sym.end_line.min(lines.len());
        if start > lines.len() || end < start {
            continue;
        }
        if cursor_line < start {
            out.extend(chunk_line_range(
                lines,
                cursor_line,
                start - 1,
                None,
                None,
                max_bytes,
            ));
        }

        let slice = join_lines(lines, start, end);
        if slice.len() <= max_bytes || sym.children.is_empty() {
            out.extend(chunk_line_range(
                lines,
                start,
                end,
                Some(sym.name_path.as_str()),
                Some(sym.kind.as_str()),
                max_bytes,
            ));
        } else {
            out.extend(chunk_symbol_with_children(
                lines, sym, start, end, max_bytes,
            ));
        }
        cursor_line = end + 1;
    }

    if cursor_line <= lines.len() {
        out.extend(chunk_line_range(
            lines,
            cursor_line,
            lines.len(),
            None,
            None,
            max_bytes,
        ));
    }

    out
}

fn chunk_symbol_with_children(
    lines: &[&str],
    parent: &Symbol,
    start: usize,
    end: usize,
    max_bytes: usize,
) -> Vec<TextChunk> {
    let mut out = Vec::new();
    let mut cursor = start;
    for child in &parent.children {
        let c_start = child.line.max(cursor);
        let c_end = child.end_line.min(end);
        if c_start > end || c_end < c_start {
            continue;
        }
        if cursor < c_start {
            out.extend(chunk_line_range(
                lines,
                cursor,
                c_start - 1,
                Some(parent.name_path.as_str()),
                Some(parent.kind.as_str()),
                max_bytes,
            ));
        }
        out.extend(chunk_line_range(
            lines,
            c_start,
            c_end,
            Some(child.name_path.as_str()),
            Some(child.kind.as_str()),
            max_bytes,
        ));
        cursor = c_end + 1;
    }
    if cursor <= end {
        out.extend(chunk_line_range(
            lines,
            cursor,
            end,
            Some(parent.name_path.as_str()),
            Some(parent.kind.as_str()),
            max_bytes,
        ));
    }
    out
}

fn chunk_line_range(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    symbol_path: Option<&str>,
    kind: Option<&str>,
    max_bytes: usize,
) -> Vec<TextChunk> {
    if start_line > end_line || start_line == 0 || end_line > lines.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut chunk_start = start_line;
    let mut current = String::new();

    for line_no in start_line..=end_line {
        let line = lines[line_no - 1];
        let added = if current.is_empty() {
            line.len()
        } else {
            line.len() + 1
        };
        if !current.is_empty() && current.len() + added > max_bytes {
            out.push(TextChunk {
                start_line: chunk_start,
                end_line: line_no - 1,
                symbol_path: symbol_path.map(str::to_string),
                kind: kind.map(str::to_string),
                text: std::mem::take(&mut current),
            });
            chunk_start = line_no;
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        out.push(TextChunk {
            start_line: chunk_start,
            end_line,
            symbol_path: symbol_path.map(str::to_string),
            kind: kind.map(str::to_string),
            text: current,
        });
    }
    out
}

fn join_lines(lines: &[&str], start_line: usize, end_line: usize) -> String {
    lines[start_line - 1..end_line].join("\n")
}
