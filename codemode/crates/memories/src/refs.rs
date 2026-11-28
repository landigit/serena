//! Pure `mem:<name>` reference extraction, boundary-safe rewriting, and dangling-link graph.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

const MEMORY_REF_PREFIX: &str = "mem:";

/// A single unresolved `mem:<target>` reference inside a memory document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DanglingReference {
    pub source: String,
    pub target: String,
}

/// Directed `mem:` reference graph across all project memories.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceGraph {
    pub edges: BTreeMap<String, Vec<String>>,
    pub dangling: Vec<DanglingReference>,
}

fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/')
}

/// Extracts all `mem:<name>` targets in `content` using `[A-Za-z0-9_\-/]` boundary rules.
pub fn extract_references(content: &str) -> Vec<String> {
    let bytes = content.as_bytes();
    let prefix = MEMORY_REF_PREFIX.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i + prefix.len() < bytes.len() {
        if &bytes[i..i + prefix.len()] == prefix {
            let prev_ok = i == 0 || !is_name_char(bytes[i - 1]);
            if prev_ok {
                let mut end = i + prefix.len();
                while end < bytes.len() && is_name_char(bytes[end]) {
                    end += 1;
                }
                if end > i + prefix.len() {
                    out.push(content[i + prefix.len()..end].to_string());
                    i = end;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// Replaces exact `mem:<old_name>` tokens with `mem:<new_name>` without touching longer names.
pub fn rewrite_references(content: &str, old_name: &str, new_name: &str) -> (String, usize) {
    let old_ref = format!("{MEMORY_REF_PREFIX}{old_name}");
    let new_ref = format!("{MEMORY_REF_PREFIX}{new_name}");
    let bytes = content.as_bytes();
    let target = old_ref.as_bytes();

    let mut out = String::with_capacity(content.len());
    let mut last = 0usize;
    let mut count = 0usize;
    let mut i = 0usize;

    while i + target.len() <= bytes.len() {
        if &bytes[i..i + target.len()] == target {
            let prev_ok = i == 0 || !is_name_char(bytes[i - 1]);
            let next_ok = i + target.len() == bytes.len() || !is_name_char(bytes[i + target.len()]);
            if prev_ok && next_ok {
                out.push_str(&content[last..i]);
                out.push_str(&new_ref);
                i += target.len();
                last = i;
                count += 1;
                continue;
            }
        }
        i += 1;
    }

    if count == 0 {
        return (content.to_string(), 0);
    }
    out.push_str(&content[last..]);
    (out, count)
}

/// Builds a `ReferenceGraph` from `(name, content)` pairs.
pub fn build_reference_graph(memories: &[(String, String)]) -> ReferenceGraph {
    let existing: HashSet<&str> = memories.iter().map(|(n, _)| n.as_str()).collect();
    let mut edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut dangling = Vec::new();

    for (source, content) in memories {
        let mut targets = Vec::new();
        let mut seen_targets = HashSet::new();
        let mut seen_dangling = HashSet::new();
        for target in extract_references(content) {
            if seen_targets.insert(target.clone()) {
                targets.push(target.clone());
            }
            if !existing.contains(target.as_str()) && seen_dangling.insert(target.clone()) {
                dangling.push(DanglingReference {
                    source: source.clone(),
                    target,
                });
            }
        }
        edges.insert(source.clone(), targets);
    }

    ReferenceGraph { edges, dangling }
}
