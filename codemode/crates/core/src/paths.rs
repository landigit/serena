//! XDG runtime database paths, project hashing, and `.codemode/` layout.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Normalizes `path` via filesystem canonicalization or lexical cleanup when missing.
pub fn canonical_or_lexical(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let mut normalized = PathBuf::new();
    for component in abs.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Computes a deterministic BLAKE3 hex identifier for a project root.
pub fn project_hash(root: &Path) -> String {
    let canonical = canonical_or_lexical(root);
    let bytes = canonical.as_os_str().as_encoded_bytes();
    blake3::hash(bytes).to_hex().to_string()
}

/// Resolves the `codemode` XDG data directory from environment values.
pub fn data_dir_from_env(xdg_data_home: Option<&OsStr>, home: Option<&OsStr>) -> PathBuf {
    if let Some(xdg) = xdg_data_home.filter(|v| !v.is_empty()) {
        return PathBuf::from(xdg).join("codemode");
    }
    if let Some(home) = home.filter(|v| !v.is_empty()) {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("codemode");
    }
    std::env::temp_dir().join("codemode")
}

/// Returns `$XDG_DATA_HOME/codemode` (or `~/.local/share/codemode`).
pub fn data_dir() -> PathBuf {
    data_dir_from_env(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Returns `$XDG_DATA_HOME/codemode/<canonical-root-blake3>/context.db`.
pub fn context_db_path(root: &Path) -> PathBuf {
    data_dir().join(project_hash(root)).join("context.db")
}

/// Returns `<root>/.codemode/memories`.
pub fn memories_dir(root: &Path) -> PathBuf {
    root.join(".codemode").join("memories")
}

/// Returns `<root>/.codemode/config.toml`.
pub fn config_path(root: &Path) -> PathBuf {
    root.join(".codemode").join("config.toml")
}
