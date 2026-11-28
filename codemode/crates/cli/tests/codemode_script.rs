use std::io::{BufRead, BufReader, Write};
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

#[test]
fn python_script_chains_codemode_outline_read_and_search_inside_code_execute() {
    let base = std::env::temp_dir().join(format!("codemode-script-py-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let repo = base.join("repo");
    let xdg = base.join("xdg");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(&xdg).unwrap();

    let py_file = repo.join("service.py");
    std::fs::write(
        &py_file,
        "class TokenService:\n    def verify(self, tok):\n        return tok == 'ok_v1'\n",
    )
    .unwrap();

    let mut child = Command::new(bin())
        .arg("serve")
        .env("XDG_DATA_HOME", &xdg)
        .current_dir(&repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    let _ = rpc(&mut stdin, &mut stdout, 1, "initialize", json!({}));

    let script = format!(
        r#"
out = codemode.outline(file={file:?})
first = out["symbols"][0]["children"][0]["name_path"]
body = codemode.read(file={file:?}, name_path=first)
codemode.index(text=body, label="script:verify")
hits = codemode.search(query="ok_v1", label="script:verify")
print("SYMBOL=" + first)
print("HITS=" + str(hits["count"]))
assert "ok_v1" in body
"#,
        file = py_file.display().to_string()
    );

    let resp = rpc(
        &mut stdin,
        &mut stdout,
        2,
        "tools/call",
        json!({
            "name": "code_execute",
            "arguments": {
                "mode": "codemode",
                "runtime": "python3",
                "cwd": repo,
                "project_root": repo,
                "code": script
            }
        }),
    );
    assert!(resp.get("error").is_none(), "{resp}");
    let text = resp["result"]["content"][0]["text"].as_str().unwrap();
    let outcome: Value = serde_json::from_str(text).unwrap();
    assert_eq!(outcome["exit_code"], 0, "{outcome}");
    let out = outcome["inline_output"].as_str().unwrap();
    assert!(out.contains("SYMBOL=TokenService/verify"), "{out}");
    assert!(out.contains("HITS=1"), "{out}");

    // Also verify recursive code_execute via codemode RPC is blocked cleanly.
    let rec_resp = rpc(
        &mut stdin,
        &mut stdout,
        3,
        "tools/call",
        json!({
            "name": "code_execute",
            "arguments": {
                "mode": "codemode",
                "runtime": "python3",
                "cwd": repo,
                "project_root": repo,
                "code": "try:\n    codemode.call('code_execute', {'code': 'echo hi'})\n    print('UNEXPECTED')\nexcept Exception as e:\n    print('BLOCKED=' + str(e))"
            }
        }),
    );
    let rec_text = rec_resp["result"]["content"][0]["text"].as_str().unwrap();
    let rec_outcome: Value = serde_json::from_str(rec_text).unwrap();
    assert!(
        rec_outcome["inline_output"]
            .as_str()
            .unwrap()
            .contains("BLOCKED="),
        "{rec_outcome}"
    );

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
