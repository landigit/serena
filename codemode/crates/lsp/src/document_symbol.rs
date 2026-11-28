//! Translates LSP `DocumentSymbol[]` and `SymbolInformation[]` into `codemode_ts::Symbol` trees.

use std::collections::HashMap;

use codemode_ts::{Symbol, finalize_paths};
use serde_json::Value;

use crate::client::byte_of;

/// Maps an LSP `SymbolKind` integer to a normalized `codemode` symbol kind label.
pub fn lsp_symbol_kind_label(kind: Option<u64>) -> &'static str {
    match kind {
        Some(1) => "file",
        Some(2) | Some(3) | Some(4) => "module",
        Some(5) => "class",
        Some(6) | Some(12) => "function",
        Some(7) | Some(8) | Some(13) => "variable",
        Some(9) => "constructor",
        Some(10) => "enum",
        Some(11) => "interface",
        Some(14) => "const",
        Some(22) => "const",
        Some(23) => "struct",
        Some(25) => "function",
        Some(26) => "type",
        _ => "declaration",
    }
}

/// Converts an LSP `textDocument/documentSymbol` response into a `codemode_ts::Symbol` tree.
pub fn document_symbols_outline(value: &Value, source: &str) -> Vec<Symbol> {
    let Some(items) = value.as_array() else {
        return Vec::new();
    };
    if items.is_empty() {
        return Vec::new();
    }

    let is_hierarchical = items
        .iter()
        .any(|item| item.get("range").is_some() || item.get("selectionRange").is_some());

    let mut symbols = if is_hierarchical {
        items
            .iter()
            .filter_map(|item| convert_document_symbol(item, source))
            .collect()
    } else {
        convert_symbol_information(items, source)
    };
    finalize_paths(&mut symbols, "", &[]);
    symbols
}

fn convert_document_symbol(item: &Value, source: &str) -> Option<Symbol> {
    let name = item.get("name").and_then(Value::as_str)?.to_string();
    let kind = lsp_symbol_kind_label(item.get("kind").and_then(Value::as_u64)).to_string();
    let range = item.get("range")?;
    let sel_range = item.get("selectionRange").unwrap_or(range);
    let (line, end_line, start_byte, end_byte) = range_offsets(range, source)?;
    let (_, _, name_byte, _) =
        range_offsets(sel_range, source).unwrap_or((line, end_line, start_byte, end_byte));
    let children = item
        .get("children")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|child| convert_document_symbol(child, source))
                .collect()
        })
        .unwrap_or_default();

    Some(Symbol {
        name,
        kind,
        name_path: String::new(),
        overload_idx: None,
        path_components: Vec::new(),
        line,
        end_line,
        start_byte,
        name_byte,
        end_byte,
        children,
    })
}

fn convert_symbol_information(items: &[Value], source: &str) -> Vec<Symbol> {
    struct FlatItem {
        container: Option<String>,
        symbol: Symbol,
    }

    let mut flat: Vec<FlatItem> = Vec::with_capacity(items.len());
    for item in items {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(range) = item.get("location").and_then(|loc| loc.get("range")) else {
            continue;
        };
        let Some((line, end_line, start_byte, end_byte)) = range_offsets(range, source) else {
            continue;
        };
        let kind = lsp_symbol_kind_label(item.get("kind").and_then(Value::as_u64)).to_string();
        let container = item
            .get("containerName")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        flat.push(FlatItem {
            container,
            symbol: Symbol {
                name: name.to_string(),
                kind,
                name_path: String::new(),
                overload_idx: None,
                path_components: Vec::new(),
                line,
                end_line,
                start_byte,
                name_byte: start_byte,
                end_byte,
                children: Vec::new(),
            },
        });
    }

    let mut roots: Vec<Symbol> = Vec::new();
    let mut name_to_indices: HashMap<String, Vec<usize>> = HashMap::new();
    let nodes: Vec<(Option<String>, Symbol)> =
        flat.into_iter().map(|f| (f.container, f.symbol)).collect();

    for (idx, (_, sym)) in nodes.iter().enumerate() {
        name_to_indices
            .entry(sym.name.clone())
            .or_default()
            .push(idx);
    }

    // Build parent-child links (attach each item to the nearest preceding matching container).
    let mut children_map: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut root_indices: Vec<usize> = Vec::new();
    for (idx, (container, sym)) in nodes.iter().enumerate() {
        let parent_idx = container.as_ref().and_then(|cname| {
            name_to_indices.get(cname).and_then(|candidates| {
                candidates
                    .iter()
                    .copied()
                    .filter(|&cand| cand != idx && nodes[cand].1.start_byte <= sym.start_byte)
                    .max_by_key(|&cand| nodes[cand].1.start_byte)
                    .or_else(|| candidates.iter().copied().find(|&cand| cand != idx))
            })
        });
        if let Some(p) = parent_idx {
            children_map.entry(p).or_default().push(idx);
        } else {
            root_indices.push(idx);
        }
    }

    fn take_subtree(
        idx: usize,
        nodes: &mut [Option<Symbol>],
        children_map: &HashMap<usize, Vec<usize>>,
        visiting: &mut Vec<usize>,
    ) -> Option<Symbol> {
        if visiting.contains(&idx) {
            return None;
        }
        let mut sym = nodes[idx].take()?;
        visiting.push(idx);
        if let Some(child_ids) = children_map.get(&idx) {
            for &cid in child_ids {
                if let Some(child) = take_subtree(cid, nodes, children_map, visiting) {
                    sym.children.push(child);
                }
            }
        }
        visiting.pop();
        Some(sym)
    }

    let mut opt_nodes: Vec<Option<Symbol>> = nodes.into_iter().map(|(_, s)| Some(s)).collect();
    let mut visiting = Vec::new();
    for idx in root_indices {
        if let Some(sym) = take_subtree(idx, &mut opt_nodes, &children_map, &mut visiting) {
            roots.push(sym);
        }
    }
    for idx in 0..opt_nodes.len() {
        if opt_nodes[idx].is_some()
            && let Some(sym) = take_subtree(idx, &mut opt_nodes, &children_map, &mut visiting)
        {
            roots.push(sym);
        }
    }
    roots
}

fn range_offsets(range: &Value, source: &str) -> Option<(usize, usize, usize, usize)> {
    let start = range.get("start")?;
    let end = range.get("end")?;
    let s_line = start.get("line")?.as_u64()? as u32;
    let s_char = start.get("character")?.as_u64()? as u32;
    let e_line = end.get("line")?.as_u64()? as u32;
    let e_char = end.get("character")?.as_u64()? as u32;
    let start_byte = byte_of(source, s_line, s_char);
    let end_byte = byte_of(source, e_line, e_char).max(start_byte);
    Some((
        s_line as usize + 1,
        e_line as usize + 1,
        start_byte,
        end_byte,
    ))
}
