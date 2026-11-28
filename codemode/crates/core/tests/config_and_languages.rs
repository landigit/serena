use std::collections::HashSet;
use std::path::Path;

use codemode_core::config::{ProjectConfig, load_project_config};
use codemode_core::languages::{LanguageId, all_languages, resolve_language};
use codemode_core::paths::{context_db_path, data_dir, memories_dir, project_hash};

#[test]
fn all_languages_covers_serena_non_alias_registry() {
    let langs = all_languages();
    assert!(
        langs.len() >= 40,
        "expected >= 40 languages, got {}",
        langs.len()
    );

    let keys: HashSet<&str> = langs.iter().map(|spec| spec.id.key()).collect();
    for expected in [
        "julia",
        "dart",
        "elixir",
        "zig",
        "haskell",
        "scala",
        "vue",
        "svelte",
        "astro",
        "bash",
        "powershell",
        "deno",
        "ansible",
        "angular",
        "lean4",
        "rust",
        "python",
        "typescript",
    ] {
        assert!(keys.contains(expected), "missing language key: {expected}");
    }
}

#[test]
fn config_languages_override_overlapping_extensions() {
    let deno_cfg = ProjectConfig {
        languages: vec!["deno".into()],
        ..ProjectConfig::default()
    };
    assert_eq!(
        resolve_language(Path::new("script.ts"), Some(&deno_cfg)),
        Some(LanguageId::Deno)
    );

    let ansible_cfg = ProjectConfig {
        languages: vec!["ansible".into()],
        ..ProjectConfig::default()
    };
    assert_eq!(
        resolve_language(Path::new("playbook.yml"), Some(&ansible_cfg)),
        Some(LanguageId::Ansible)
    );

    let angular_cfg = ProjectConfig {
        languages: vec!["angular".into()],
        ..ProjectConfig::default()
    };
    assert_eq!(
        resolve_language(Path::new("component.html"), Some(&angular_cfg)),
        Some(LanguageId::Angular)
    );

    // Without overrides, default priority chooses TypeScript / Yaml / Html.
    assert_eq!(
        resolve_language(Path::new("script.ts"), None),
        Some(LanguageId::TypeScript)
    );
    assert_eq!(
        resolve_language(Path::new("playbook.yml"), None),
        Some(LanguageId::Yaml)
    );
    assert_eq!(
        resolve_language(Path::new("component.html"), None),
        Some(LanguageId::Html)
    );
}

#[test]
fn project_hash_is_deterministic_and_distinct_across_worktrees() {
    let base = std::env::temp_dir().join(format!("codemode-core-paths-{}", std::process::id()));
    let wt_a = base.join("wt-a");
    let wt_b = base.join("wt-b");
    std::fs::create_dir_all(&wt_a).unwrap();
    std::fs::create_dir_all(&wt_b).unwrap();

    let h_a1 = project_hash(&wt_a);
    let h_a2 = project_hash(&wt_a);
    let h_b = project_hash(&wt_b);

    assert_eq!(h_a1, h_a2);
    assert!(h_a1.len() >= 16, "hash too short: {h_a1}");
    assert!(h_a1.bytes().all(|b| b.is_ascii_hexdigit()), "{h_a1}");
    assert_ne!(h_a1, h_b);

    let db_a = context_db_path(&wt_a);
    assert!(db_a.starts_with(data_dir()));
    assert!(db_a.ends_with("context.db"));
    assert_eq!(memories_dir(&wt_a), wt_a.join(".codemode").join("memories"));
}

#[test]
fn loads_optional_and_explicit_project_config() {
    let base = std::env::temp_dir().join(format!("codemode-core-cfg-{}", std::process::id()));
    let empty_root = base.join("empty");
    std::fs::create_dir_all(&empty_root).unwrap();
    let default_cfg = load_project_config(&empty_root).unwrap();
    assert!(default_cfg.languages.is_empty());

    let configured_root = base.join("configured");
    std::fs::create_dir_all(configured_root.join(".codemode")).unwrap();
    std::fs::write(
        configured_root.join(".codemode/config.toml"),
        r#"
languages = ["deno", "python"]

[lsp_overrides]
python = "ty server"

[ignore]
extra_patterns = ["*.generated.ts", "dist/"]

[policy]
inline_cap_bytes = 4096
deny_bulk_reads = true
"#,
    )
    .unwrap();
    let cfg = load_project_config(&configured_root).unwrap();
    assert_eq!(cfg.languages, vec!["deno", "python"]);
    assert_eq!(
        cfg.lsp_overrides.get("python").map(String::as_str),
        Some("ty server")
    );
    assert_eq!(cfg.ignore.extra_patterns, vec!["*.generated.ts", "dist/"]);
    assert_eq!(cfg.policy.inline_cap_bytes, 4096);
    assert!(cfg.policy.deny_bulk_reads);

    let broken_root = base.join("broken");
    std::fs::create_dir_all(broken_root.join(".codemode")).unwrap();
    std::fs::write(broken_root.join(".codemode/config.toml"), "languages = [").unwrap();
    assert!(load_project_config(&broken_root).is_err());
}

#[test]
fn handles_compound_extensions_lsp_overrides_and_xdg_env() {
    use codemode_core::languages::{server_command_for, workspace_root_for};
    use codemode_core::paths::data_dir_from_env;
    use std::ffi::OsStr;

    assert_eq!(
        resolve_language(Path::new("rebar.app.src"), None),
        Some(LanguageId::Erlang)
    );
    assert_eq!(
        LanguageId::TypeScript.lsp_id_for_path(Path::new("App.tsx")),
        "typescriptreact"
    );
    assert_eq!(LanguageId::Cpp.lsp_id_for_path(Path::new("main.c")), "c");

    let mut cfg = ProjectConfig::default();
    cfg.lsp_overrides
        .insert("python".into(), "custom-py-lsp --stdio".into());
    assert_eq!(
        server_command_for(LanguageId::Python, Some(&cfg)).unwrap(),
        vec!["custom-py-lsp", "--stdio"]
    );

    let xdg = data_dir_from_env(
        Some(OsStr::new("/tmp/xdg-data")),
        Some(OsStr::new("/home/u")),
    );
    assert_eq!(xdg, Path::new("/tmp/xdg-data/codemode"));
    let home = data_dir_from_env(None, Some(OsStr::new("/home/u")));
    assert_eq!(home, Path::new("/home/u/.local/share/codemode"));

    let base = std::env::temp_dir().join(format!("codemode-core-ws-{}", std::process::id()));
    let pkg = base.join("mono/pkg");
    std::fs::create_dir_all(pkg.join("src")).unwrap();
    std::fs::write(base.join("mono/.git"), "").unwrap();
    std::fs::write(pkg.join("Cargo.toml"), "[package]").unwrap();
    assert_eq!(
        workspace_root_for(&pkg.join("src/lib.rs"), LanguageId::Rust),
        pkg
    );
}
