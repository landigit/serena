//! JSON-RPC 2.0 stdio MCP server and unified `code_*` tool surface.

pub mod tools;

use std::io::{BufRead, Write};

use serde_json::{Value, json};

pub use tools::{ToolRuntime, call, doctor_report, list, run};

/// Serves newline-delimited JSON-RPC 2.0 MCP requests over stdin/stdout.
pub fn serve_stdio() -> std::io::Result<()> {
    let rt = ToolRuntime::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(msg) => msg,
            Err(err) => {
                eprintln!("codemode: ignoring malformed JSON: {err}");
                continue;
            }
        };
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        if id.is_none() {
            if method == "exit" {
                break;
            }
            continue;
        }

        let response = match dispatch_method(method, &params, &rt) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(msg) => {
                let code = if msg.starts_with("method not found") {
                    -32601
                } else {
                    -32603
                };
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
            }
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}

fn dispatch_method(method: &str, params: &Value, rt: &ToolRuntime) -> Result<Value, String> {
    match method {
        "initialize" => Ok(initialize_reply(params)),
        "ping" => Ok(json!({})),
        "shutdown" => Ok(Value::Null),
        "tools/list" => Ok(tools::list()),
        "tools/call" => Ok(tools::call(params, rt)),
        other => Err(format!("method not found: {other}")),
    }
}

fn initialize_reply(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "codemode", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "Use code_outline before reading large files, code_read/code_edit for symbol-level operations, code_execute for sandboxed runs with FTS5 spilling, code_search/code_index for bounded retrieval, code_memory for .codemode/memories, and code_resume after compaction."
    })
}
