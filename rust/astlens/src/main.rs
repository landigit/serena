//! astlens: a stateless structural + LSP MCP server.

mod diff;
mod lsp;
mod mcp;
mod roots;
mod tools;
mod ts;

// The gated name-path unit (rust/name-path/src/port.rs), shared without a PyO3 link.
#[path = "../../name-path/src/port.rs"]
#[allow(dead_code)]
mod name_path;

// The gated diagnostics-group unit (rust/diagnostics-group-rs/src/port.rs), shared without a PyO3 link.
#[path = "../../diagnostics-group-rs/src/port.rs"]
#[allow(dead_code)]
mod diagnostics_group;

use std::sync::Arc;

use serde_json::{Value, json};

fn main() {
    let manager = Arc::new(lsp::LspManager::new());
    let result = mcp::serve(|method, params| match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "shutdown" => Ok(Value::Null),
        "tools/list" => Ok(tools::list()),
        "tools/call" => Ok(tools::call(&params, &manager)),
        other => Err(format!("method not found: {other}")),
    });
    if let Err(error) = result {
        eprintln!("astlens: {error}");
        std::process::exit(1);
    }
}

fn initialize(params: &Value) -> Value {
    let protocol_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "astlens", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "astlens complements native file tools. Use ast_outline before reading a large file, read_symbol for one exact body, find_references/diagnostics when the language server knows better than grep, and edit_symbol to replace or rename one symbol. Every call derives its workspace from the file path."
    })
}
