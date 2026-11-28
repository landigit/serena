//! Workspace roots and language-server commands. Stateless: everything is derived
//! from the file path on each call, so parallel worktrees never share state.

use std::path::{Path, PathBuf};

use crate::ts::Lang;

const DEFAULT_MAX_PROJECT_FILES: usize = 500;

struct IgnoreRule {
    negated: bool,
    dir_only: bool,
    anchored: bool,
    pattern: String,
}

struct IgnoreScope {
    base: String,
    rules: Vec<IgnoreRule>,
}

fn is_default_ignored_dir(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "node_modules"
                | "target"
                | "venv"
                | "__pycache__"
                | "dist"
                | "build"
                | "out"
                | "vendor"
        )
}

fn parse_ignore_file(path: &Path) -> Vec<IgnoreRule> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim_end_matches('\r').trim_end();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (negated, rest) = if let Some(stripped) = line.strip_prefix('!') {
            (true, stripped)
        } else {
            (false, line)
        };
        let rest = rest.strip_prefix("\\#").map_or(rest, |_| &rest[1..]);
        let (dir_only, rest) = if let Some(stripped) = rest.strip_suffix('/') {
            (true, stripped)
        } else {
            (false, rest)
        };
        if rest.is_empty() {
            continue;
        }
        let anchored = rest.contains('/');
        let pattern = rest.trim_start_matches('/').to_string();
        if !pattern.is_empty() {
            rules.push(IgnoreRule {
                negated,
                dir_only,
                anchored,
                pattern,
            });
        }
    }
    rules
}

fn seg_glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star_pi, mut star_ti) = (None, 0usize);
    while ti < t.len() {
        if pi < p.len() && p[pi] == '\\' && pi + 1 < p.len() {
            if p[pi + 1] == t[ti] {
                pi += 2;
                ti += 1;
                continue;
            }
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
            continue;
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
            continue;
        }
        if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn match_path_segments(pat_segs: &[&str], txt_segs: &[&str]) -> bool {
    if pat_segs.is_empty() {
        return txt_segs.is_empty();
    }
    if pat_segs[0] == "**" {
        if pat_segs.len() == 1 {
            return !txt_segs.is_empty();
        }
        if match_path_segments(&pat_segs[1..], txt_segs) {
            return true;
        }
        return !txt_segs.is_empty() && match_path_segments(pat_segs, &txt_segs[1..]);
    }
    !txt_segs.is_empty()
        && seg_glob_match(pat_segs[0], txt_segs[0])
        && match_path_segments(&pat_segs[1..], &txt_segs[1..])
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let pat_segs: Vec<&str> = pattern.split('/').collect();
    let txt_segs: Vec<&str> = text.split('/').collect();
    match_path_segments(&pat_segs, &txt_segs)
}

fn is_ignored(scopes: &[IgnoreScope], rel: &str, name: &str, is_dir: bool) -> bool {
    let mut ignored = false;
    for scope in scopes {
        let sub_rel = if scope.base.is_empty() {
            rel
        } else if let Some(rest) = rel.strip_prefix(&scope.base) {
            if let Some(after_slash) = rest.strip_prefix('/') {
                after_slash
            } else {
                continue;
            }
        } else {
            continue;
        };
        if sub_rel.is_empty() {
            continue;
        }
        for rule in &scope.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let matched = if rule.anchored {
                glob_match(&rule.pattern, sub_rel)
            } else {
                seg_glob_match(&rule.pattern, name)
            };
            if matched {
                ignored = !rule.negated;
            }
        }
    }
    ignored
}

fn collect_files(
    dir: &Path,
    rel_prefix: &str,
    scopes: &mut Vec<IgnoreScope>,
    lang_filter: Option<Lang>,
    max_files: usize,
    out: &mut Vec<PathBuf>,
    capped: &mut bool,
) {
    if *capped {
        return;
    }
    let mut rules = parse_ignore_file(&dir.join(".gitignore"));
    rules.extend(parse_ignore_file(&dir.join(".ignore")));
    let pushed = !rules.is_empty();
    if pushed {
        scopes.push(IgnoreScope {
            base: rel_prefix.to_string(),
            rules,
        });
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        if pushed {
            scopes.pop();
        }
        return;
    };
    let mut entries: Vec<_> = read_dir.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if *capped {
            break;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };
        let path = entry.path();
        let rel = if rel_prefix.is_empty() {
            name.to_string()
        } else {
            format!("{rel_prefix}/{name}")
        };
        if file_type.is_dir() {
            if is_default_ignored_dir(name) || is_ignored(scopes, &rel, name, true) {
                continue;
            }
            collect_files(&path, &rel, scopes, lang_filter, max_files, out, capped);
        } else if file_type.is_file() {
            let Some(lang) = Lang::from_path(&path) else {
                continue;
            };
            if lang_filter.is_some_and(|target| target != lang) {
                continue;
            }
            if is_ignored(scopes, &rel, name, false) {
                continue;
            }
            if out.len() >= max_files {
                *capped = true;
                break;
            }
            out.push(path);
        }
    }

    if pushed {
        scopes.pop();
    }
}

/// Discover non-ignored source files in `root` up to `max_files`.
/// Returns `(files, capped)` where `capped` is true when more matching files exist.
pub fn discover_project_files(
    root: &Path,
    lang_filter: Option<Lang>,
    priority_file: Option<&Path>,
    max_files: usize,
) -> (Vec<PathBuf>, bool) {
    let limit = max_files.clamp(1, DEFAULT_MAX_PROJECT_FILES);
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut out = Vec::new();
    let mut capped = false;
    let mut scopes = Vec::new();
    collect_files(
        &root,
        "",
        &mut scopes,
        lang_filter,
        limit,
        &mut out,
        &mut capped,
    );

    if let Some(priority) = priority_file {
        let priority = std::fs::canonicalize(priority).unwrap_or_else(|_| priority.to_path_buf());
        if priority.is_file() {
            if let Some(pos) = out.iter().position(|existing| existing == &priority) {
                out.remove(pos);
                out.insert(0, priority);
            } else if let Some(lang) = Lang::from_path(&priority)
                && lang_filter.is_none_or(|target| target == lang)
            {
                if out.len() >= limit {
                    out.pop();
                    capped = true;
                }
                out.insert(0, priority);
            }
        }
    }

    (out, capped)
}

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

    #[test]
    fn discovers_project_files_with_gitignore_and_cap() {
        let base = std::env::temp_dir().join(format!("astlens-discover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src")).unwrap();
        std::fs::create_dir_all(base.join("target")).unwrap();
        std::fs::create_dir_all(base.join("generated")).unwrap();
        std::fs::create_dir_all(base.join(".git")).unwrap();

        std::fs::write(
            base.join(".gitignore"),
            "generated/\n*.gen.rs\n!keep.gen.rs\n",
        )
        .unwrap();
        std::fs::write(base.join("src/a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(base.join("src/b.rs"), "fn b() {}\n").unwrap();
        std::fs::write(base.join("src/c.rs"), "fn c() {}\n").unwrap();
        std::fs::write(base.join("src/skip.gen.rs"), "fn skip() {}\n").unwrap();
        std::fs::write(base.join("src/keep.gen.rs"), "fn keep() {}\n").unwrap();
        std::fs::write(base.join("target/out.rs"), "fn target() {}\n").unwrap();
        std::fs::write(base.join("generated/g.rs"), "fn gen() {}\n").unwrap();

        let (all, capped_all) = discover_project_files(&base, Some(Lang::Rust), None, 20);
        assert!(!capped_all);
        let names: Vec<String> = all
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(names, vec!["a.rs", "b.rs", "c.rs", "keep.gen.rs"]);

        let (capped_files, capped) =
            discover_project_files(&base, Some(Lang::Rust), Some(&base.join("src/c.rs")), 2);
        assert!(capped);
        assert_eq!(capped_files.len(), 2);
        assert_eq!(capped_files[0].file_name().unwrap(), "c.rs");
    }

    #[test]
    fn handles_nested_gitignore_globs_and_symlinks() {
        let base =
            std::env::temp_dir().join(format!("astlens-discover-edge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("app/sub")).unwrap();
        std::fs::create_dir_all(base.join("app_other")).unwrap();

        std::fs::write(
            base.join(".gitignore"),
            "/root_only.rs\n**/nested_skip.rs\na_glob/**\n!a_glob/keep.rs\n",
        )
        .unwrap();
        std::fs::write(base.join("app/.gitignore"), "local_skip.rs\n").unwrap();
        std::fs::create_dir_all(base.join("a_glob")).unwrap();

        std::fs::write(base.join("root_only.rs"), "fn r() {}\n").unwrap();
        std::fs::write(base.join("app/root_only.rs"), "fn ar() {}\n").unwrap();
        std::fs::write(base.join("app/local_skip.rs"), "fn ls() {}\n").unwrap();
        std::fs::write(base.join("app/sub/nested_skip.rs"), "fn ns() {}\n").unwrap();
        std::fs::write(base.join("app_other/local_skip.rs"), "fn ok() {}\n").unwrap();
        std::fs::write(base.join("a_glob/skip.rs"), "fn s() {}\n").unwrap();
        std::fs::write(base.join("a_glob/keep.rs"), "fn k() {}\n").unwrap();

        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(base.join("app"), base.join("app/loop"));
        }

        let (files, capped) = discover_project_files(&base, Some(Lang::Rust), None, 20);
        assert!(!capped);
        let rels: Vec<String> = files
            .iter()
            .map(|p| {
                p.strip_prefix(&base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        assert_eq!(
            rels,
            vec![
                "a_glob/keep.rs",
                "app/root_only.rs",
                "app_other/local_skip.rs"
            ]
        );
    }
}
