//! Project-level `.codemode/config.toml` schema and loader.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::config_path;

/// Errors raised while loading or validating project configuration.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {source}")]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// Additional ignore patterns configured in `.codemode/config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct IgnoreConfig {
    #[serde(alias = "patterns")]
    pub extra_patterns: Vec<String>,
}

/// Sandbox, hook, and output-budget policy settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicyConfig {
    pub inline_cap_bytes: usize,
    pub read_cap_bytes: usize,
    pub deny_bulk_reads: bool,
    pub allow_env: Vec<String>,
    pub timeout_ms: u64,
    pub read_only_memories: Vec<String>,
    pub ignored_memories: Vec<String>,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            inline_cap_bytes: 6144,
            read_cap_bytes: 8192,
            deny_bulk_reads: true,
            allow_env: Vec::new(),
            timeout_ms: 30_000,
            read_only_memories: Vec::new(),
            ignored_memories: Vec::new(),
        }
    }
}

/// Parsed `.codemode/config.toml` configuration for a project root.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectConfig {
    pub languages: Vec<String>,
    pub lsp_overrides: BTreeMap<String, String>,
    pub ignore: IgnoreConfig,
    pub policy: PolicyConfig,
}

/// Loads `.codemode/config.toml` under `root`, returning defaults if absent.
pub fn load_project_config(root: &Path) -> Result<ProjectConfig, CoreError> {
    let path = config_path(root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(ProjectConfig::default()),
        Err(source) => return Err(CoreError::Io { path, source }),
    };
    toml::from_str(&raw).map_err(|source| CoreError::Toml { path, source })
}

/// Finds the nearest ancestor containing `.codemode` or `.git`, or falls back to `start`.
pub fn find_project_root(start: &Path) -> PathBuf {
    let dir = if start.is_dir() {
        start
    } else {
        start.parent().unwrap_or(start)
    };
    for ancestor in dir.ancestors() {
        if ancestor.join(".codemode").is_dir() || ancestor.join(".git").exists() {
            return ancestor.to_path_buf();
        }
    }
    dir.to_path_buf()
}
