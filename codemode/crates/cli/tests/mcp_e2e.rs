use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_codemode")
}

fn rpc(
    stdin: &mut impl Write,
    stdout: &mut BufReader<impl std::io::Read>,
    id: i64,
    method: &str,
    params: Value,
) -> Value {
    let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    writeln!(stdin, "{req}").unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn call_tool(
    stdin: &mut impl Write,
    stdout: &mut BufReader<impl std::io::Read>,
    id: i64,
    name: &str,
    arguments: Value,
) -> Value {
    let resp = rpc(
        stdin,
        stdout,
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    );
    assert!(
        resp.get("error").is_none(),
        "{name} returned JSON-RPC error: {resp}"
    );
    let result = &resp["result"];
    assert_ne!(
        result.get("isError").and_then(Value::as_bool),
        Some(true),
        "{name} returned tool error: {result}"
    );
    let text = result["content"][0]["text"].as_str().unwrap();
    serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
}

#[test]
fn mcp_stdio_lists_12_tools_and_executes_full_roundtrip() {
    let base = std::env::temp_dir().join(format!("codemode-e2e-mcp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    let xdg = base.join("xdg");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::create_dir_all(&xdg).unwrap();

    let py_file = repo.join("src/calc.py");
    std::fs::write(
        &py_file,
        "class Calc:\n    def add(self, a, b):\n        return a + b\n",
    )
    .unwrap();

    let mut child = Command::new(bin())
        .arg("serve")
        .env("XDG_DATA_HOME", &xdg)
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let init = rpc(&mut stdin, &mut stdout, 1, "initialize", json!({}));
    assert_eq!(init["result"]["serverInfo"]["name"], "codemode");

    let list = rpc(&mut stdin, &mut stdout, 2, "tools/list", json!({}));
    let tools: Vec<String> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(tools.len(), 12, "{tools:?}");
    for expected in [
        "code_outline",
        "code_read",
        "code_references",
        "code_diagnostics",
        "code_edit",
        "code_execute",
        "code_index",
        "code_search",
        "code_resume",
        "code_memory",
        "code_batch",
        "code_admin",
    ] {
        assert!(tools.contains(&expected.to_string()), "missing {expected}");
    }

    // 1. code_outline + code_read + code_edit
    let outline = call_tool(
        &mut stdin,
        &mut stdout,
        3,
        "code_outline",
        json!({"file": py_file}),
    );
    assert_eq!(outline["language"], "python");
    assert_eq!(
        outline["symbols"][0]["children"][0]["name_path"],
        "Calc/add"
    );

    let read = call_tool(
        &mut stdin,
        &mut stdout,
        4,
        "code_read",
        json!({"file": py_file, "name_path": "Calc/add"}),
    );
    assert!(read.as_str().unwrap().contains("return a + b"));

    let edited = call_tool(
        &mut stdin,
        &mut stdout,
        5,
        "code_edit",
        json!({
            "file": py_file,
            "name_path": "Calc/add",
            "mode": "replace",
            "content": "def add(self, a, b):\n        return a + b + 1"
        }),
    );
    assert_eq!(edited["changed"], true);
    assert!(
        std::fs::read_to_string(&py_file)
            .unwrap()
            .contains("a + b + 1")
    );

    // 2. code_execute (spills oversized output) -> code_search roundtrip
    let exec = call_tool(
        &mut stdin,
        &mut stdout,
        6,
        "code_execute",
        json!({
            "runtime": "python3",
            "cwd": repo,
            "inline_cap_bytes": 256,
            "code": "for i in range(80): print(f'row {i}: e2e_spill_needle_99')"
        }),
    );
    let source_id = exec["spill"]["source_id"].as_str().unwrap().to_string();
    let search = call_tool(
        &mut stdin,
        &mut stdout,
        7,
        "code_search",
        json!({"project_root": repo, "source_id": source_id, "query": "e2e_spill_needle_99"}),
    );
    assert!(!search["hits"].as_array().unwrap().is_empty());

    // 3. code_memory write -> read -> rename
    call_tool(
        &mut stdin,
        &mut stdout,
        8,
        "code_memory",
        json!({"project_root": repo, "action": "write", "name": "auth", "content": "JWT design\n"}),
    );
    call_tool(
        &mut stdin,
        &mut stdout,
        9,
        "code_memory",
        json!({"project_root": repo, "action": "write", "name": "overview", "content": "See mem:auth.\n"}),
    );
    let renamed = call_tool(
        &mut stdin,
        &mut stdout,
        10,
        "code_memory",
        json!({"project_root": repo, "action": "rename", "name": "auth", "new_name": "security/auth"}),
    );
    assert_eq!(renamed["rewritten_references"], 1);
    let overview = call_tool(
        &mut stdin,
        &mut stdout,
        11,
        "code_memory",
        json!({"project_root": repo, "action": "read", "name": "overview"}),
    );
    assert!(
        overview["content"]
            .as_str()
            .unwrap()
            .contains("mem:security/auth")
    );

    // 4. code_batch parallel dispatch + code_resume + code_admin
    let batch = call_tool(
        &mut stdin,
        &mut stdout,
        12,
        "code_batch",
        json!({
            "project_root": repo,
            "calls": [
                {"name": "code_outline", "arguments": {"file": py_file}},
                {"name": "code_index", "arguments": {"path": repo}}
            ]
        }),
    );
    assert_eq!(batch["results"].as_array().unwrap().len(), 2);

    let resume = call_tool(
        &mut stdin,
        &mut stdout,
        13,
        "code_resume",
        json!({"project_root": repo}),
    );
    assert!(
        resume["changed_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("calc.py"))
    );

    let stats = call_tool(
        &mut stdin,
        &mut stdout,
        14,
        "code_admin",
        json!({"project_root": repo, "action": "stats"}),
    );
    assert!(stats["event_count"].as_u64().unwrap() >= 1);

    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success());
}

#[test]
fn cli_subcommands_doctor_stats_index_and_hook_exit_zero() {
    let base = std::env::temp_dir().join(format!("codemode-e2e-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    let xdg = base.join("xdg");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(&xdg).unwrap();
    std::fs::write(repo.join("lib.rs"), "pub fn cli_smoke_fn() {}\n").unwrap();

    let doctor = Command::new(bin())
        .args(["doctor", "--root"])
        .arg(&repo)
        .env("XDG_DATA_HOME", &xdg)
        .output()
        .unwrap();
    assert!(doctor.status.success(), "{:?}", doctor);
    let doctor_json: Value = serde_json::from_slice(&doctor.stdout).unwrap();
    assert!(doctor_json["languages_count"].as_u64().unwrap() >= 40);

    let idx = Command::new(bin())
        .arg("index")
        .arg(&repo)
        .env("XDG_DATA_HOME", &xdg)
        .output()
        .unwrap();
    assert!(idx.status.success(), "{:?}", idx);

    let stats = Command::new(bin())
        .args(["stats", "--root"])
        .arg(&repo)
        .env("XDG_DATA_HOME", &xdg)
        .output()
        .unwrap();
    assert!(stats.status.success(), "{:?}", stats);

    let mut hook_child = Command::new(bin())
        .args(["hook", "pre-tool", "--root"])
        .arg(&repo)
        .env("XDG_DATA_HOME", &xdg)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hook_child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"tool_name":"Bash","command":"git status"}"#)
        .unwrap();
    let hook_out = hook_child.wait_with_output().unwrap();
    assert!(hook_out.status.success());
    let hook_json: Value = serde_json::from_slice(&hook_out.stdout).unwrap();
    assert_eq!(hook_json["allow"], true);
    let _ = Path::new(".");
}
