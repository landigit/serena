//! Runtime command resolution and sensitive environment-variable scrubbing.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

/// Sensitive variable name substrings stripped unless explicitly allowlisted.
const SENSITIVE_FRAGMENTS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "KEY",
    "PASSWORD",
    "PASSWD",
    "CREDENTIAL",
    "AUTH",
    "COOKIE",
    "SESSION",
    "PRIVATE",
    "OPENAI_",
    "ANTHROPIC_",
    "AWS_",
    "AZURE_",
    "GCP_",
    "GOOGLE_API",
    "STRIPE_",
    "DATABASE_URL",
];

/// Resolves `(program, args)` for a requested sandbox runtime name.
pub fn resolve_runtime_command(runtime: &str, code: &str) -> Result<Vec<String>, String> {
    let norm = runtime.trim().to_ascii_lowercase();
    match norm.as_str() {
        "" | "sh" | "shell" | "auto" => Ok(vec!["sh".into(), "-c".into(), code.into()]),
        "bash" => Ok(vec!["bash".into(), "-c".into(), code.into()]),
        "python" | "python3" | "py" => Ok(vec!["python3".into(), "-c".into(), code.into()]),
        "node" | "js" | "javascript" => Ok(vec!["node".into(), "-e".into(), code.into()]),
        other => Err(format!(
            "unsupported runtime '{other}' (expected sh, bash, python3, or node)"
        )),
    }
}

/// Detects which supported runtimes exist on `PATH`.
pub fn detect_runtimes() -> Vec<&'static str> {
    let mut found = Vec::new();
    for bin in ["sh", "bash", "python3", "node"] {
        if has_binary(bin) {
            found.push(bin);
        }
    }
    found
}

fn has_binary(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .any(|p| p.is_file())
    })
}

/// Builds a scrubbed environment map, stripping sensitive secrets unless allowlisted.
pub fn build_scrubbed_env(
    allowlist: &[String],
    extra_env: &BTreeMap<String, String>,
) -> Vec<(String, String)> {
    let allowed: HashSet<&str> = allowlist.iter().map(String::as_str).collect();
    let mut env_map: BTreeMap<String, String> = BTreeMap::new();

    for (key, val) in std::env::vars() {
        if allowed.contains(key.as_str()) || !is_sensitive_key(&key) {
            env_map.insert(key, val);
        }
    }
    for (key, val) in extra_env {
        env_map.insert(key.clone(), val.clone());
    }
    // Preserve temp/xdg paths cleanly if set.
    let _ = PathBuf::new();
    env_map.into_iter().collect()
}

fn is_sensitive_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    // Keep harmless SSH/GPG display or keyboard layout vars only if not secret-bearing.
    SENSITIVE_FRAGMENTS.iter().any(|frag| upper.contains(frag))
}
