//! Minimal MCP server over stdio: newline-delimited JSON-RPC 2.0.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

pub fn serve<F>(mut handler: F) -> std::io::Result<()>
where
    F: FnMut(&str, Value) -> Result<Value, String>,
{
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("astlens: ignoring malformed JSON: {error}");
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

        let response = match handler(method, params) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(message) => {
                let code = if message.starts_with("method not found") {
                    -32601
                } else {
                    -32603
                };
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        writeln!(stdout, "{response}")?;
        stdout.flush()?;
    }
    Ok(())
}
