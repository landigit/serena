//! Workspace roots and language-server commands. Stateless: everything is derived
//! from the file path on each call, so parallel worktrees never share state.

use std::path::{Path, PathBuf};

use crate::ts::Lang;

fn dir_has_marker(dir: &Path, marker: &str) -> bool {
    if let Some(ext) = marker.strip_prefix("*.") {
        return std::fs::read_dir(dir)
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .and_then(|value| value.to_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(ext))
            });
    }
    dir.join(marker).exists()
}

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
        Lang::Ruby => &["Gemfile", "Rakefile", ".ruby-version", ".git"],
        Lang::Php => &["composer.json", ".git"],
        Lang::CSharp => &["*.sln", "*.csproj", "global.json", ".git"],
        Lang::Kotlin => &[
            "build.gradle.kts",
            "build.gradle",
            "settings.gradle.kts",
            "settings.gradle",
            "pom.xml",
            ".git",
        ],
        Lang::Lean => &["lean-toolchain", "lakefile.toml", "lakefile.lean", ".git"],
    };
    for dir in start.ancestors() {
        if markers.iter().any(|marker| dir_has_marker(dir, marker)) {
            return dir.to_path_buf();
        }
    }
    start.to_path_buf()
}

fn find_binary(binary: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH")
        && let Some(found) = std::env::split_paths(&path)
            .map(|dir| dir.join(binary))
            .find(|candidate| candidate.is_file())
    {
        return Some(found);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for sub in [".cargo/bin", ".elan/bin", ".local/bin"] {
            let candidate = home.join(sub).join(binary);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
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
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => &[
            ("tsgo", &["--lsp", "--stdio"]),
            ("vtsls", &["--stdio"]),
            ("typescript-language-server", &["--stdio"]),
        ],
        Lang::Rust => &[("rust-analyzer", &[])],
        Lang::Python => &[
            ("ty", &["server"]),
            ("pyrefly", &["lsp"]),
            ("pyright-langserver", &["--stdio"]),
            ("basedpyright-langserver", &["--stdio"]),
        ],
        Lang::Go => &[("gopls", &[])],
        Lang::Java => &[("jdtls", &[])],
        Lang::C | Lang::Cpp => &[("clangd", &[])],
        Lang::Ruby => &[("ruby-lsp", &[]), ("solargraph", &["stdio"])],
        Lang::Php => &[
            ("intelephense", &["--stdio"]),
            ("phpactor", &["language-server"]),
        ],
        Lang::CSharp => &[("csharp-ls", &["--stdio"]), ("OmniSharp", &["-lsp"])],
        Lang::Kotlin => &[
            ("kotlin-lsp", &["--stdio"]),
            ("kotlin-language-server", &["--stdio"]),
        ],
        Lang::Lean => &[("lean", &["--server"])],
    };

    for (binary, args) in candidates {
        if let Some(path) = find_binary(binary) {
            let mut command = vec![path.display().to_string()];
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

    #[test]
    fn finds_csharp_glob_and_lean_toolchain_roots() {
        let base = std::env::temp_dir().join(format!("astlens-roots2-{}", std::process::id()));
        let cs_dir = base.join("cs/Src");
        std::fs::create_dir_all(&cs_dir).unwrap();
        std::fs::write(base.join("cs/App.csproj"), "<Project/>").unwrap();
        assert_eq!(
            workspace_root(&cs_dir.join("Program.cs"), Lang::CSharp),
            base.join("cs")
        );

        let lean_dir = base.join("lean/Math");
        std::fs::create_dir_all(&lean_dir).unwrap();
        std::fs::write(base.join("lean/lean-toolchain"), "leanprover/lean4:v4.12.0").unwrap();
        assert_eq!(
            workspace_root(&lean_dir.join("Basic.lean"), Lang::Lean),
            base.join("lean")
        );
    }
}
