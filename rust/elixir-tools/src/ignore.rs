//! Ignore rules, source filename matching, and Elixir symbol name normalization.
//!
//! Mirrors `ElixirTools.is_ignored_dirname`, `ElixirTools.is_ignored_path`,
//! `ElixirTools._normalize_symbol_name`, and the Elixir `FilenameMatcher`
//! (`.ex`, `.exs`) from `src/solidlsp/`.

use std::path::Path;

/// Base ignored directories from `SolidLanguageServer._ALWAYS_IGNORED_DIRS`.
pub const ALWAYS_IGNORED_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    ".bzr",
    ".venv",
    ".env",
    ".cache",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
    ".idea",
    ".serena",
    ".vscode",
];

/// Elixir-specific ignored directories from `ElixirTools.is_ignored_dirname`.
pub const ELIXIR_IGNORED_DIRS: &[&str] = &[
    "_build",
    "deps",
    "node_modules",
    ".elixir_ls",
    ".expert",
    "cover",
];

/// Supported Elixir source extensions from `LanguageServerId.ELIXIR`.
pub const SOURCE_EXTENSIONS: &[&str] = &[".ex", ".exs"];

/// Normalizes raw Expert document symbol names for Elixir functions/macros.
///
/// Mirrors `ElixirTools._normalize_symbol_name`:
/// `symbol["name"].removeprefix("defp ").removeprefix("def ").split("(", 1)[0].strip()`
pub fn normalize_symbol_name(raw_name: &str) -> String {
    let stripped_defp = raw_name.strip_prefix("defp ").unwrap_or(raw_name);
    let stripped_def = stripped_defp.strip_prefix("def ").unwrap_or(stripped_defp);
    let head = stripped_def
        .split_once('(')
        .map_or(stripped_def, |(prefix, _)| prefix);
    head.trim().to_string()
}

/// Returns `true` if `dirname` should always be ignored for Elixir projects.
pub fn is_ignored_dirname(dirname: &str) -> bool {
    ALWAYS_IGNORED_DIRS.contains(&dirname) || ELIXIR_IGNORED_DIRS.contains(&dirname)
}

/// Returns `true` if `filename` has a supported Elixir source extension (`.ex` or `.exs`).
pub fn is_relevant_elixir_filename(filename: &str) -> bool {
    SOURCE_EXTENSIONS.iter().any(|ext| filename.ends_with(ext))
}

#[derive(Debug, Clone)]
struct WildRule {
    negated: bool,
    dir_only: bool,
    anchored: bool,
    pattern_segments: Vec<String>,
}

/// Gitwildmatch-style ignore specification matching `LanguageServerConfig.ignored_paths`.
#[derive(Debug, Clone, Default)]
pub struct IgnoreSpec {
    rules: Vec<WildRule>,
}

impl IgnoreSpec {
    /// Parses gitwildmatch patterns into an [`IgnoreSpec`].
    pub fn from_patterns<I, S>(patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut rules = Vec::new();
        for raw in patterns {
            if let Some(rule) = parse_rule(raw.as_ref()) {
                rules.push(rule);
            }
        }
        Self { rules }
    }

    /// Evaluates whether `relative_path` matches this specification, using
    /// `serena.util.file_system.match_path` semantics.
    pub fn matches(
        &self,
        relative_path: &str,
        repository_root: Option<&Path>,
        is_dir: Option<bool>,
    ) -> bool {
        if self.rules.is_empty() || relative_path.is_empty() || relative_path == "." {
            return false;
        }

        let mut normalized = relative_path.replace('\\', "/");
        if !normalized.starts_with('/') {
            normalized.insert(0, '/');
        }

        let resolved_is_dir = if normalized.ends_with('/') {
            true
        } else if let Some(dir_flag) = is_dir {
            dir_flag
        } else if let Some(root) = repository_root {
            root.join(relative_path).is_dir()
        } else {
            false
        };

        if resolved_is_dir && !normalized.ends_with('/') {
            normalized.push('/');
        }

        let path_is_dir = normalized.ends_with('/');
        let trimmed = normalized.trim_matches('/');
        if trimmed.is_empty() {
            return false;
        }
        let path_segments: Vec<&str> = trimmed.split('/').collect();

        let mut matched = false;
        for rule in &self.rules {
            if rule_matches(rule, &path_segments, path_is_dir) {
                matched = !rule.negated;
            }
        }
        matched
    }
}

fn parse_rule(raw: &str) -> Option<WildRule> {
    let trimmed_end = raw.trim_end_matches(['\r', '\n']);
    if trimmed_end.trim().is_empty() {
        return None;
    }
    if trimmed_end.starts_with('#') {
        return None;
    }

    let (negated, after_neg) = if let Some(rest) = trimmed_end.strip_prefix('!') {
        (true, rest)
    } else if trimmed_end.starts_with("\\!") || trimmed_end.starts_with("\\#") {
        (false, &trimmed_end[1..])
    } else {
        (false, trimmed_end)
    };

    if after_neg.is_empty() {
        return None;
    }

    let dir_only = after_neg.ends_with('/');
    let without_trailing = after_neg.trim_end_matches('/');
    if without_trailing.is_empty() {
        return None;
    }

    let anchored = without_trailing.contains('/');
    let without_leading = without_trailing.trim_start_matches('/');
    if without_leading.is_empty() {
        return None;
    }

    let pattern_segments = without_leading
        .split('/')
        .map(ToString::to_string)
        .collect();

    Some(WildRule {
        negated,
        dir_only,
        anchored,
        pattern_segments,
    })
}

fn rule_matches(rule: &WildRule, path_segments: &[&str], path_is_dir: bool) -> bool {
    if rule.anchored {
        if matches_segments(
            &rule.pattern_segments,
            path_segments,
            rule.dir_only,
            path_is_dir,
        ) {
            return true;
        }
        // Directory-level anchored pattern `/scripts` also ignores descendants `/scripts/a.ex`.
        if !rule.dir_only
            && path_segments.len() > rule.pattern_segments.len()
            && matches_segments(
                &rule.pattern_segments,
                &path_segments[..rule.pattern_segments.len()],
                false,
                true,
            )
        {
            return true;
        }
        return false;
    }

    // Unanchored single-segment pattern (e.g. `scripts`, `*cripts`, `ignored_*`, `*.tmp`).
    let pat = &rule.pattern_segments[0];
    if rule.dir_only {
        // Matches any directory component (all prefix components, or the final component if path_is_dir).
        let limit = if path_is_dir {
            path_segments.len()
        } else {
            path_segments.len().saturating_sub(1)
        };
        return path_segments[..limit]
            .iter()
            .any(|seg| glob_match(pat, seg));
    }

    // Non-dir-only unanchored pattern matches the target itself or any parent directory.
    path_segments.iter().any(|seg| glob_match(pat, seg))
}

fn matches_segments(
    pattern_segs: &[String],
    path_segs: &[&str],
    dir_only: bool,
    path_is_dir: bool,
) -> bool {
    if dir_only && !path_is_dir && path_segs.len() <= pattern_segs.len() {
        // `scripts/` should not match a non-directory `scripts` file, only `scripts/` or `scripts/x`.
        return matches_segments_dp(pattern_segs, path_segs, true, path_is_dir);
    }
    matches_segments_dp(pattern_segs, path_segs, dir_only, path_is_dir)
}

fn matches_segments_dp(
    pattern_segs: &[String],
    path_segs: &[&str],
    dir_only: bool,
    path_is_dir: bool,
) -> bool {
    let m = pattern_segs.len();
    let n = path_segs.len();
    if m == 0 {
        return n == 0;
    }

    if pattern_segs.iter().all(|s| s != "**") {
        if dir_only && n > m {
            return pattern_segs
                .iter()
                .zip(&path_segs[..m])
                .all(|(p, s)| glob_match(p, s));
        }
        if m != n {
            return false;
        }
        if dir_only && !path_is_dir {
            return false;
        }
        return pattern_segs
            .iter()
            .zip(path_segs)
            .all(|(p, s)| glob_match(p, s));
    }

    let mut dp = vec![vec![false; n + 1]; m + 1];
    dp[0][0] = true;
    for i in 1..=m {
        if pattern_segs[i - 1] == "**" {
            dp[i][0] = dp[i - 1][0];
        }
    }
    for i in 1..=m {
        for j in 1..=n {
            if pattern_segs[i - 1] == "**" {
                dp[i][j] = dp[i - 1][j] || dp[i][j - 1];
            } else if glob_match(&pattern_segs[i - 1], path_segs[j - 1]) {
                dp[i][j] = dp[i - 1][j - 1];
            }
        }
    }
    if dp[m][n] {
        return !dir_only || path_is_dir;
    }
    if dir_only {
        return dp[m].iter().take(n).skip(1).any(|&matched| matched);
    }
    false
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_chars(&p, &t)
}

fn glob_match_chars(p: &[char], t: &[char]) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi: Option<usize> = None;
    let mut star_ti = 0;

    while ti < t.len() {
        if pi < p.len() && p[pi] == '\\' && pi + 1 < p.len() {
            if p[pi + 1] == t[ti] {
                pi += 2;
                ti += 1;
                continue;
            }
        } else if pi < p.len() && p[pi] == '?' {
            pi += 1;
            ti += 1;
            continue;
        } else if pi < p.len() && p[pi] == '[' {
            if let Some((matched, next_pi)) = match_char_class(&p[pi..], t[ti]) {
                if matched {
                    pi += next_pi;
                    ti += 1;
                    continue;
                }
            } else if p[pi] == t[ti] {
                pi += 1;
                ti += 1;
                continue;
            }
        } else if pi < p.len() && p[pi] == '*' {
            star_pi = Some(pi);
            star_ti = ti;
            pi += 1;
            continue;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
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

fn match_char_class(p: &[char], ch: char) -> Option<(bool, usize)> {
    // Expects p[0] == '['
    let mut i = 1;
    if i >= p.len() {
        return None;
    }
    let negated = p[i] == '!' || p[i] == '^';
    if negated {
        i += 1;
    }
    let start = i;
    let mut matched = false;
    while i < p.len() {
        if p[i] == ']' && i > start {
            return Some((if negated { !matched } else { matched }, i + 1));
        }
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            let lo = p[i];
            let hi = p[i + 2];
            if lo <= ch && ch <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if p[i] == ch {
                matched = true;
            }
            i += 1;
        }
    }
    None
}

/// Checks whether `relative_path` should be ignored for symbol indexing.
///
/// Mirrors `ElixirTools.is_ignored_path` and `SolidLanguageServer.is_ignored_path`.
pub fn is_ignored_path(
    relative_path: &str,
    repository_root: &Path,
    ignore_unsupported_files: bool,
    check_path_for_ignored_dirnames: bool,
    ignore_spec: &IgnoreSpec,
) -> bool {
    if relative_path.ends_with("mix.exs") {
        return true;
    }

    let abs_path = repository_root.join(relative_path);
    let (is_file, is_dir) = if let Ok(meta) = std::fs::metadata(&abs_path) {
        (meta.is_file(), meta.is_dir())
    } else {
        (Path::new(relative_path).extension().is_some(), false)
    };

    if check_path_for_ignored_dirnames {
        let normalized = relative_path.replace('\\', "/");
        let mut parts: Vec<&str> = normalized
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .collect();
        if is_file && !parts.is_empty() {
            parts.pop();
        }
        for part in parts {
            if is_ignored_dirname(part) {
                return true;
            }
        }
    }

    if is_file && ignore_unsupported_files {
        let abs_str = abs_path.to_string_lossy();
        if !is_relevant_elixir_filename(&abs_str) {
            return true;
        }
    }

    ignore_spec.matches(relative_path, Some(repository_root), Some(is_dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn ignored_dirnames_cover_base_and_elixir_dirs() {
        for dir in [
            "_build",
            "deps",
            "node_modules",
            ".elixir_ls",
            ".expert",
            "cover",
        ] {
            assert!(is_ignored_dirname(dir), "{dir} should be ignored");
        }
        for dir in [".git", ".venv", ".serena", ".vscode"] {
            assert!(is_ignored_dirname(dir), "{dir} should be ignored");
        }
        for dir in ["lib", "test", "config", "priv"] {
            assert!(!is_ignored_dirname(dir), "{dir} should not be ignored");
        }
    }

    #[test]
    fn ignored_path_handles_mix_exs_ignored_dirs_extensions_and_globs() {
        let root = PathBuf::from("/nonexistent/repo");
        let spec = IgnoreSpec::from_patterns(["scripts", "ignored_*", "*.tmp"]);

        assert!(is_ignored_path("mix.exs", &root, true, true, &spec));
        assert!(is_ignored_path("mix.exs", &root, false, false, &spec));
        assert!(is_ignored_path("server/mix.exs", &root, true, true, &spec));
        assert!(is_ignored_path(
            "_build/dev/lib/foo.ex",
            &root,
            true,
            true,
            &spec
        ));
        assert!(is_ignored_path(
            ".expert/cache.ex",
            &root,
            true,
            true,
            &spec
        ));
        assert!(is_ignored_path(
            "deps/plug/lib/plug.ex",
            &root,
            true,
            true,
            &spec
        ));
        assert!(is_ignored_path("lib/readme.md", &root, true, true, &spec));
        assert!(!is_ignored_path("lib/readme.md", &root, false, true, &spec));
        assert!(is_ignored_path(
            "scripts/seed.exs",
            &root,
            true,
            true,
            &spec
        ));
        assert!(is_ignored_path(
            "ignored_dir/helper.ex",
            &root,
            true,
            true,
            &spec
        ));
        assert!(!is_ignored_path("lib/models.ex", &root, true, true, &spec));
        assert!(!is_ignored_path(
            "test/models_test.exs",
            &root,
            true,
            true,
            &spec
        ));
        assert!(!is_ignored_path("lib/models.ex", &root, true, false, &spec));
        assert!(!is_ignored_path(
            "_build/dev/lib/foo.ex",
            &root,
            true,
            false,
            &spec
        ));
    }

    #[test]
    fn ignore_spec_handles_negation_anchoring_dir_only_doublestar_and_empty() {
        let root = PathBuf::from("/nonexistent/repo");
        let spec = IgnoreSpec::from_patterns([
            "# comment",
            "   ",
            "*.tmp",
            "!keep.tmp",
            "/root_only.ex",
            "logs/",
            "**/gen/*.ex",
            "item_[0-9].ex",
        ]);

        assert!(!spec.matches("", Some(&root), None));
        assert!(!spec.matches(".", Some(&root), None));
        assert!(spec.matches("a/b.tmp", Some(&root), Some(false)));
        assert!(!spec.matches("keep.tmp", Some(&root), Some(false)));
        assert!(spec.matches("root_only.ex", Some(&root), Some(false)));
        assert!(!spec.matches("sub/root_only.ex", Some(&root), Some(false)));
        assert!(!spec.matches("logs", Some(&root), Some(false)));
        assert!(spec.matches("logs", Some(&root), Some(true)));
        assert!(spec.matches("logs/app.log", Some(&root), Some(false)));
        assert!(spec.matches("a/b/gen/schema.ex", Some(&root), Some(false)));
        assert!(spec.matches("gen/schema.ex", Some(&root), Some(false)));
        assert!(spec.matches("item_7.ex", Some(&root), Some(false)));
        assert!(!spec.matches("item_x.ex", Some(&root), Some(false)));
        assert!(is_ignored_path(
            "_build\\dev\\foo.ex",
            &root,
            true,
            true,
            &spec
        ));
    }

    #[test]
    fn normalize_symbol_name_matches_elixir_tools() {
        assert_eq!(
            normalize_symbol_name("def create_user(pid, attrs)"),
            "create_user"
        );
        assert_eq!(normalize_symbol_name("defp helper(x)"), "helper");
        assert_eq!(normalize_symbol_name("def new()"), "new");
        assert_eq!(
            normalize_symbol_name("  def spaced_fn (a, b) "),
            "def spaced_fn"
        );
        assert_eq!(normalize_symbol_name("def   spaced_fn(a, b)"), "spaced_fn");
        assert_eq!(normalize_symbol_name("defp def nested(x)"), "nested");
        assert_eq!(normalize_symbol_name("TestRepo.Models"), "TestRepo.Models");
        assert_eq!(normalize_symbol_name("defp "), "");
    }
}
