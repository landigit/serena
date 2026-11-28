use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::Value;

fn packaging_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("packaging")
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_codemode")
}

#[test]
fn validates_all_host_manifests_and_hook_smoke_calls() {
    let pkg = packaging_root();

    // Antigravity
    let ag_plugin: Value = serde_json::from_str(
        &std::fs::read_to_string(pkg.join("antigravity/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ag_plugin["name"], "codemode");

    let ag_mcp: Value = serde_json::from_str(
        &std::fs::read_to_string(pkg.join("antigravity/mcp_config.json")).unwrap(),
    )
    .unwrap();
    assert!(ag_mcp["mcpServers"]["codemode"]["command"].is_string());

    let ag_hooks: Value =
        serde_json::from_str(&std::fs::read_to_string(pkg.join("antigravity/hooks.json")).unwrap())
            .unwrap();
    for ev in ["PreToolUse", "PostToolUse", "SessionStart", "PreCompact"] {
        assert!(ag_hooks["hooks"][ev].is_array(), "missing {ev}");
    }
    let skill = std::fs::read_to_string(pkg.join("antigravity/skills/codemode/SKILL.md")).unwrap();
    assert!(skill.contains("code_outline"));
    assert!(!skill.contains("FORBIDDEN"));

    // OpenCode
    let oc: Value =
        serde_json::from_str(&std::fs::read_to_string(pkg.join("opencode/opencode.json")).unwrap())
            .unwrap();
    assert_eq!(oc["mcp"]["codemode"]["type"], "local");
    let oc_ts = std::fs::read_to_string(pkg.join("opencode/plugins/codemode-hooks.ts")).unwrap();
    assert!(oc_ts.contains("codemode"));

    // Claude Code
    let cc: Value = serde_json::from_str(
        &std::fs::read_to_string(pkg.join("claude-code/.claude-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(cc["name"], "codemode");
    let cc_mcp: Value =
        serde_json::from_str(&std::fs::read_to_string(pkg.join("claude-code/.mcp.json")).unwrap())
            .unwrap();
    assert!(cc_mcp["mcpServers"]["codemode"].is_object());

    // Codex TOML
    let codex_raw = std::fs::read_to_string(pkg.join("codex/config.toml")).unwrap();
    let codex_toml: toml::Value = toml::from_str(&codex_raw).unwrap();
    assert_eq!(
        codex_toml["mcp_servers"]["codemode"]["command"].as_str(),
        Some("codemode")
    );

    // README & install script
    let readme = std::fs::read_to_string(pkg.join("../README.md")).unwrap();
    assert!(readme.contains("GPL-3.0-or-later"));
    assert!(pkg.join("install.sh").is_file());

    // Smoke run hook events against the compiled binary
    for (ev, input) in [
        ("pre-tool", r#"{"tool_name":"Bash","command":"git status"}"#),
        ("post-tool", r#"{"tool_name":"Bash","output":"ok"}"#),
        ("session-start", r#"{"turn_number":1,"prompt":"smoke"}"#),
        ("pre-compact", r#"{}"#),
    ] {
        let mut child = Command::new(bin())
            .args(["hook", ev])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{ev} failed: {:?}", out);
    }
}
