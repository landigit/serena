//! Markdown project memory store (`.codemode/memories/*.md`) with `ContextDb` indexing and `mem:` graph.

pub mod refs;

use std::path::{Component, Path, PathBuf};

use codemode_core::paths::memories_dir;
use codemode_store::{ContextDb, EventKind, JournalEvent, StoredChunkInput};
use serde::{Deserialize, Serialize};

pub use refs::{
    DanglingReference, ReferenceGraph, build_reference_graph, extract_references,
    rewrite_references,
};

/// Errors raised while validating, reading, writing, or renaming project memories.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("invalid memory name: {0}")]
    InvalidName(String),
    #[error("memory '{0}' not found")]
    NotFound(String),
    #[error("memory '{0}' already exists")]
    AlreadyExists(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("store error: {0}")]
    Store(#[from] codemode_store::StoreError),
}

/// Metadata for a stored `.codemode/memories/<name>.md` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub name: String,
    pub path: PathBuf,
    pub bytes: usize,
}

/// Outcome of renaming a memory and propagating `mem:` references across sibling memories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameReport {
    pub old_name: String,
    pub new_name: String,
    pub rewritten_references: usize,
    pub updated_memories: Vec<String>,
}

/// Manages `.codemode/memories/*.md` CRUD and reference integrity for one project root.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    root: PathBuf,
    dir: PathBuf,
}

impl MemoryStore {
    /// Creates a store rooted at `<project_root>/.codemode/memories`.
    pub fn new(project_root: &Path) -> Self {
        Self {
            root: project_root.to_path_buf(),
            dir: memories_dir(project_root),
        }
    }

    /// Returns the `.codemode/memories` directory path.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Sanitizes an agent-provided memory name (stripping `mem:` prefix and `.md` suffix).
    pub fn sanitize_name(raw: &str) -> Result<String, MemoryError> {
        let mut name = raw.trim().replace('\\', "/");
        if let Some(stripped) = name.strip_prefix("mem:") {
            name = stripped.to_string();
        }
        if let Some(stripped) = name.strip_suffix(".md") {
            name = stripped.to_string();
        }
        if name.is_empty() || name.starts_with('/') {
            return Err(MemoryError::InvalidName(raw.to_string()));
        }
        for part in name.split('/') {
            if part.is_empty() || part == "." || part == ".." {
                return Err(MemoryError::InvalidName(raw.to_string()));
            }
        }
        Ok(name)
    }

    /// Resolves `<memories_dir>/<name>.md` after lexical sandbox containment validation.
    pub fn resolve_path(&self, raw_name: &str) -> Result<(String, PathBuf), MemoryError> {
        let name = Self::sanitize_name(raw_name)?;
        let mut rel = PathBuf::new();
        let parts: Vec<&str> = name.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            rel.push(part);
        }
        rel.push(format!("{}.md", parts[parts.len() - 1]));

        let candidate = self.dir.join(&rel);
        for comp in rel.components() {
            if !matches!(comp, Component::Normal(_)) {
                return Err(MemoryError::InvalidName(raw_name.to_string()));
            }
        }
        Ok((name, candidate))
    }

    /// Reads a memory body by name.
    pub fn read(&self, raw_name: &str) -> Result<String, MemoryError> {
        let (name, path) = self.resolve_path(raw_name)?;
        if !path.is_file() {
            return Err(MemoryError::NotFound(name));
        }
        std::fs::read_to_string(&path).map_err(|source| MemoryError::Io { path, source })
    }

    /// Writes a memory atomically and optionally indexes it into `ContextDb`.
    pub fn write(
        &self,
        raw_name: &str,
        content: &str,
        db: Option<&ContextDb>,
    ) -> Result<MemoryEntry, MemoryError> {
        let (name, path) = self.resolve_path(raw_name)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| MemoryError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        write_atomic(&path, content)?;
        if let Some(db) = db {
            index_memory(db, &name, &path, content)?;
        }
        Ok(MemoryEntry {
            name,
            path,
            bytes: content.len(),
        })
    }

    /// Deletes a memory file and clears its entry from `ContextDb` if provided.
    pub fn delete(&self, raw_name: &str, db: Option<&ContextDb>) -> Result<(), MemoryError> {
        let (name, path) = self.resolve_path(raw_name)?;
        if !path.is_file() {
            return Err(MemoryError::NotFound(name));
        }
        std::fs::remove_file(&path).map_err(|source| MemoryError::Io { path, source })?;
        if let Some(db) = db {
            let source_id = format!("mem:{name}");
            let label = format!("memory:{name}");
            db.upsert_source(&source_id, &label, "memory", None, "", Vec::new())?;
        }
        Ok(())
    }

    /// Lists all memories under `.codemode/memories/`, optionally filtered by topic prefix.
    pub fn list(&self, topic: Option<&str>) -> Result<Vec<MemoryEntry>, MemoryError> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut entries = Vec::new();
        collect_md(&self.dir, &self.dir, &mut entries)?;
        if let Some(prefix) = topic.filter(|t| !t.trim().is_empty()) {
            let norm = Self::sanitize_name(prefix)?;
            entries.retain(|e| e.name == norm || e.name.starts_with(&format!("{norm}/")));
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Renames a memory and propagates `mem:<old>` tokens across all `.codemode/memories/*.md` files.
    pub fn rename(
        &self,
        old_raw: &str,
        new_raw: &str,
        db: Option<&ContextDb>,
    ) -> Result<RenameReport, MemoryError> {
        let (old_name, old_path) = self.resolve_path(old_raw)?;
        let (new_name, new_path) = self.resolve_path(new_raw)?;
        if !old_path.is_file() {
            return Err(MemoryError::NotFound(old_name));
        }
        if new_path.exists() {
            return Err(MemoryError::AlreadyExists(new_name));
        }
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| MemoryError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::rename(&old_path, &new_path).map_err(|source| MemoryError::Io {
            path: old_path,
            source,
        })?;

        if let Some(db) = db {
            let old_sid = format!("mem:{old_name}");
            let old_lbl = format!("memory:{old_name}");
            db.upsert_source(&old_sid, &old_lbl, "memory", None, "", Vec::new())?;
            let new_content =
                std::fs::read_to_string(&new_path).map_err(|source| MemoryError::Io {
                    path: new_path.clone(),
                    source,
                })?;
            index_memory(db, &new_name, &new_path, &new_content)?;
        }

        let mut total_rewritten = 0usize;
        let mut updated_memories = Vec::new();
        for entry in self.list(None)? {
            let content =
                std::fs::read_to_string(&entry.path).map_err(|source| MemoryError::Io {
                    path: entry.path.clone(),
                    source,
                })?;
            let (updated, count) = rewrite_references(&content, &old_name, &new_name);
            if count > 0 {
                write_atomic(&entry.path, &updated)?;
                if let Some(db) = db {
                    index_memory(db, &entry.name, &entry.path, &updated)?;
                }
                total_rewritten += count;
                updated_memories.push(entry.name);
            }
        }

        let _ = self.root;
        Ok(RenameReport {
            old_name,
            new_name,
            rewritten_references: total_rewritten,
            updated_memories,
        })
    }

    /// Computes the `mem:` reference graph and dangling links across all memories.
    pub fn reference_graph(&self) -> Result<ReferenceGraph, MemoryError> {
        let entries = self.list(None)?;
        let mut loaded = Vec::with_capacity(entries.len());
        for entry in entries {
            let content =
                std::fs::read_to_string(&entry.path).map_err(|source| MemoryError::Io {
                    path: entry.path,
                    source,
                })?;
            loaded.push((entry.name, content));
        }
        Ok(build_reference_graph(&loaded))
    }
}

fn collect_md(base: &Path, dir: &Path, out: &mut Vec<MemoryEntry>) -> Result<(), MemoryError> {
    let rd = std::fs::read_dir(dir).map_err(|source| MemoryError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_md(base, &path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md")
            && let Ok(rel) = path.strip_prefix(base)
        {
            let name = rel.with_extension("").to_string_lossy().replace('\\', "/");
            let bytes = entry.metadata().map(|m| m.len() as usize).unwrap_or(0);
            out.push(MemoryEntry { name, path, bytes });
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<(), MemoryError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("memory.md");
    let tmp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, content.as_bytes()).map_err(|source| MemoryError::Io {
        path: tmp.clone(),
        source,
    })?;
    std::fs::rename(&tmp, path).map_err(|source| MemoryError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn index_memory(db: &ContextDb, name: &str, path: &Path, content: &str) -> Result<(), MemoryError> {
    let lines: Vec<&str> = content.lines().collect();
    let chunks = if lines.is_empty() {
        Vec::new()
    } else {
        vec![StoredChunkInput {
            start_line: 1,
            end_line: lines.len(),
            symbol_path: Some(format!("mem:{name}")),
            kind: Some("memory".into()),
            content: content.to_string(),
        }]
    };
    let source_id = format!("mem:{name}");
    let label = format!("memory:{name}");
    let path_str = path.display().to_string();
    db.upsert_source(
        &source_id,
        &label,
        "memory",
        Some(&path_str),
        content,
        chunks,
    )?;
    let _ = db.record_event(&JournalEvent {
        turn_number: 0,
        kind: EventKind::Edit,
        summary: format!("Updated memory {name}"),
        detail: None,
        path: Some(path_str),
        bytes_in: content.len() as u64,
        bytes_out: 64,
    });
    Ok(())
}
