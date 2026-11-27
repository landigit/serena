//! Workspace roots and language-server commands. Stateless: everything is derived
//! from the file path on each call, so parallel worktrees never share state.

use std::path::{Path, PathBuf};

use crate::ts::Lang;

/// Nearest directory that owns the file: language marker first, then `.git`.
pub fn workspace_root(file: &Path, lang: Lang) -> PathBuf {
    let start = file.parent().unwrap_or(file);
    let markers: &[&str] = match lang {
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            &["tsconfig.json", "jsconfig.json", "package.json", ".git"]
        }
        Lang::Rust => &["Cargo.toml", ".git"],
        Lang::Python => &["pyproject.toml", "setup.py", "setup.cfg", ".git"],
        Lang::Go => &["go.mod", ".git"],
        Lang::Java => &["pom.xml", "build.gradle", "build.gradle.kts", ".git"],
        Lang::C | Lang::Cpp => &["compile_commands.json", "CMakeLists.txt", ".git"],
    };
    for dir in start.ancestors() {
        if markers.iter().any(|marker| dir.join(marker).exists()) {
            return dir.to_path_buf();
        }
    }
    start.to_path_buf()
}

fn on_path(binary: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(binary).is_file()))
        .unwrap_or(false)
}

/// Command that starts the language server, or an actionable error.
pub fn server_command(lang: Lang) -> Result<Vec<String>, String> {
    let env_key = format!("ASTLENS_LSP_{}", lang.name().to_uppercase());
    if let Ok(custom) = std::env::var(&env_key) {
        let parts: Vec<String> = custom.split_whitespace().map(str::to_string).collect();
        if !parts.is_empty() {
            return Ok(parts);
        }
    }

    let candidates: &[(&str, &[&str])] = match lang {
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => {
            &[("typescript-language-server", &["--stdio"])]
        }
        Lang::Rust => &[("rust-analyzer", &[])],
        Lang::Python => &[
            ("pyright-langserver", &["--stdio"]),
            ("basedpyright-langserver", &["--stdio"]),
        ],
        Lang::Go => &[("gopls", &[])],
        Lang::Java => &[("jdtls", &[])],
        Lang::C | Lang::Cpp => &[("clangd", &[])],
    };

    for (binary, args) in candidates {
        if on_path(binary) {
            let mut command = vec![binary.to_string()];
            command.extend(args.iter().map(|arg| arg.to_string()));
            return Ok(command);
        }
    }

    let names: Vec<&str> = candidates.iter().map(|(binary, _)| *binary).collect();
    Err(format!(
        "no language server for {} on PATH (tried {}); install one or set {}",
        lang.name(),
        names.join(", "),
        env_key
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_language_marker_wins_over_git() {
        let root = std::env::temp_dir().join(format!("astlens-root-{}", std::process::id()));
        let nested = root.join("packages/app/src");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("package.json"), "{}").unwrap();
        std::fs::write(root.join(".git"), "").unwrap();
        let file = nested.join("index.ts");
        assert_eq!(workspace_root(&file, Lang::TypeScript), root);
    }
}
