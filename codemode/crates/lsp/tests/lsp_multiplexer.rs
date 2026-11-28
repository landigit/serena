use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codemode_core::config::ProjectConfig;
use codemode_lsp::{LspManager, document_symbols_outline, evict_idle_clients};
use serde_json::json;

fn write_mock_lsp(dir: &Path, name: &str, script: &str) -> String {
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    format!("python3 {}", path.display())
}

#[test]
fn clean_unchanged_file_diagnostics_completes_under_150ms() {
    let root = std::env::temp_dir().join(format!("codemode-lsp-fast-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".codemode")).unwrap();
    std::fs::write(root.join("pyproject.toml"), "[project]\nname='fast'\n").unwrap();
    let file = root.join("clean.py");
    std::fs::write(&file, "x = 1\n").unwrap();

    // Case 1: pull-diagnostics server advertising diagnosticProvider and returning empty items.
    let pull_script = r#"
import json, sys
def read_msg():
    n = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        s = line.decode().strip()
        if not s: break
        if s.lower().startswith("content-length:"): n = int(s.split(":", 1)[1])
    return json.loads(sys.stdin.buffer.read(n))
def send_msg(obj):
    b = json.dumps(obj).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {"diagnosticProvider": {}}}})
    elif method == "textDocument/diagnostic":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"kind": "full", "items": []}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
    let mut cfg = ProjectConfig::default();
    cfg.lsp_overrides.insert(
        "python".into(),
        write_mock_lsp(&root, "mock_pull.py", pull_script),
    );
    let manager = LspManager::new();
    let client = manager.client_for(&file, Some(&cfg)).unwrap();
    let t0 = Instant::now();
    let diags = client.diagnostics(&file).unwrap();
    assert!(diags.is_empty());
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_millis(150),
        "pull clean diagnostics took {elapsed:?}"
    );
    // Repeated query on unchanged file is also fast.
    let t1 = Instant::now();
    let diags2 = client.diagnostics(&file).unwrap();
    assert!(diags2.is_empty());
    assert!(t1.elapsed() < Duration::from_millis(150));

    // Case 2: push-diagnostics server publishing empty diagnostics immediately on didOpen.
    let root2 = root.join("push");
    std::fs::create_dir_all(&root2).unwrap();
    std::fs::write(root2.join("pyproject.toml"), "[project]\nname='push'\n").unwrap();
    let file2 = root2.join("clean.py");
    std::fs::write(&file2, "y = 2\n").unwrap();
    let push_script = r#"
import json, sys
def read_msg():
    n = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        s = line.decode().strip()
        if not s: break
        if s.lower().startswith("content-length:"): n = int(s.split(":", 1)[1])
    return json.loads(sys.stdin.buffer.read(n))
def send_msg(obj):
    b = json.dumps(obj).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "textDocument/didOpen":
        uri = m["params"]["textDocument"]["uri"]
        send_msg({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": []}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
    let mut cfg2 = ProjectConfig::default();
    cfg2.lsp_overrides.insert(
        "python".into(),
        write_mock_lsp(&root2, "mock_push.py", push_script),
    );
    let client2 = manager.client_for(&file2, Some(&cfg2)).unwrap();
    let t2 = Instant::now();
    let diags_push = client2.diagnostics(&file2).unwrap();
    assert!(diags_push.is_empty());
    let elapsed2 = t2.elapsed();
    assert!(
        elapsed2 < Duration::from_millis(150),
        "push clean diagnostics took {elapsed2:?}"
    );
}

#[test]
fn evicts_idle_lsp_clients_and_terminates_children() {
    let root = std::env::temp_dir().join(format!("codemode-lsp-evict-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("pyproject.toml"), "[project]\nname='evict'\n").unwrap();
    let file = root.join("main.py");
    std::fs::write(&file, "x = 1\n").unwrap();

    let script = r#"
import json, sys
def read_msg():
    n = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        s = line.decode().strip()
        if not s: break
        if s.lower().startswith("content-length:"): n = int(s.split(":", 1)[1])
    return json.loads(sys.stdin.buffer.read(n))
def send_msg(obj):
    b = json.dumps(obj).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()
while True:
    m = read_msg()
    if m is None: break
    if m.get("method") == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif m.get("method") == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
    let mut cfg = ProjectConfig::default();
    cfg.lsp_overrides.insert(
        "python".into(),
        write_mock_lsp(&root, "mock_idle.py", script),
    );

    let manager = LspManager::new();
    let client = manager.client_for(&file, Some(&cfg)).unwrap();
    assert_eq!(manager.client_count(), 1);
    assert!(client.is_alive());
    drop(client);

    std::thread::sleep(Duration::from_millis(70));
    let evicted = evict_idle_clients(&manager, Duration::from_millis(50));
    assert_eq!(evicted, 1);
    assert_eq!(manager.client_count(), 0);
}

#[test]
fn translates_hierarchical_and_flat_document_symbols_into_symbol_trees() {
    let source = "module Billing\n  class Invoice\n    def total\n      42\n    end\n    def total(tax)\n      42 + tax\n    end\n  end\nend\n";

    let hierarchical = json!([
        {
            "name": "Billing",
            "kind": 2,
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 3}},
            "selectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 14}},
            "children": [
                {
                    "name": "Invoice",
                    "kind": 5,
                    "range": {"start": {"line": 1, "character": 2}, "end": {"line": 8, "character": 5}},
                    "selectionRange": {"start": {"line": 1, "character": 8}, "end": {"line": 1, "character": 15}},
                    "children": [
                        {
                            "name": "total",
                            "kind": 6,
                            "range": {"start": {"line": 2, "character": 4}, "end": {"line": 4, "character": 7}},
                            "selectionRange": {"start": {"line": 2, "character": 8}, "end": {"line": 2, "character": 13}}
                        },
                        {
                            "name": "total",
                            "kind": 6,
                            "range": {"start": {"line": 5, "character": 4}, "end": {"line": 7, "character": 7}},
                            "selectionRange": {"start": {"line": 5, "character": 8}, "end": {"line": 5, "character": 13}}
                        }
                    ]
                }
            ]
        }
    ]);
    let tree = document_symbols_outline(&hierarchical, source);
    let paths: Vec<&str> = codemode_ts::flatten(&tree)
        .into_iter()
        .map(|s| s.name_path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec![
            "Billing",
            "Billing/Invoice",
            "Billing/Invoice/total[0]",
            "Billing/Invoice/total[1]"
        ]
    );

    let flat = json!([
        {
            "name": "Billing",
            "kind": 2,
            "location": {"uri": "file:///tmp/x.cr", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 3}}}
        },
        {
            "name": "Invoice",
            "kind": 5,
            "containerName": "Billing",
            "location": {"uri": "file:///tmp/x.cr", "range": {"start": {"line": 1, "character": 2}, "end": {"line": 8, "character": 5}}}
        },
        {
            "name": "total",
            "kind": 6,
            "containerName": "Invoice",
            "location": {"uri": "file:///tmp/x.cr", "range": {"start": {"line": 2, "character": 4}, "end": {"line": 4, "character": 7}}}
        }
    ]);
    let flat_tree = document_symbols_outline(&flat, source);
    let flat_paths: Vec<&str> = codemode_ts::flatten(&flat_tree)
        .into_iter()
        .map(|s| s.name_path.as_str())
        .collect();
    assert_eq!(
        flat_paths,
        vec!["Billing", "Billing/Invoice", "Billing/Invoice/total"]
    );
}

#[test]
fn isolates_concurrent_queries_across_two_worktrees_and_respects_language_overrides() {
    let base = std::env::temp_dir().join(format!("codemode-lsp-wt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let wt_a = base.join("wt-a");
    let wt_b = base.join("wt-b");
    std::fs::create_dir_all(&wt_a).unwrap();
    std::fs::create_dir_all(&wt_b).unwrap();
    std::fs::write(wt_a.join("deno.json"), "{}").unwrap();
    std::fs::write(wt_b.join("deno.json"), "{}").unwrap();
    let file_a = wt_a.join("mod.ts");
    let file_b = wt_b.join("mod.ts");
    std::fs::write(&file_a, "export function a() { return 1; }\n").unwrap();
    std::fs::write(&file_b, "export function b() { return 2; }\n").unwrap();

    let script = r#"
import json, os, sys
root_name = os.path.basename(os.getcwd())
def read_msg():
    n = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        s = line.decode().strip()
        if not s: break
        if s.lower().startswith("content-length:"): n = int(s.split(":", 1)[1])
    return json.loads(sys.stdin.buffer.read(n))
def send_msg(obj):
    b = json.dumps(obj).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "textDocument/references":
        uri = m["params"]["textDocument"]["uri"]
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": [{"uri": uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}]})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
    let deno_cmd = write_mock_lsp(&base, "mock_deno.py", script);
    let mut cfg = ProjectConfig {
        languages: vec!["deno".into()],
        ..ProjectConfig::default()
    };
    cfg.lsp_overrides.insert("deno".into(), deno_cmd);

    let manager = Arc::new(LspManager::new());
    let m1 = Arc::clone(&manager);
    let m2 = Arc::clone(&manager);
    let cfg1 = cfg.clone();
    let cfg2 = cfg;
    let fa = file_a.clone();
    let fb = file_b.clone();

    let t1 = std::thread::spawn(move || {
        let client = m1.client_for(&fa, Some(&cfg1)).unwrap();
        client.references(&fa, 0, 16, "references", true).unwrap()
    });
    let t2 = std::thread::spawn(move || {
        let client = m2.client_for(&fb, Some(&cfg2)).unwrap();
        client.references(&fb, 0, 16, "references", true).unwrap()
    });

    let locs_a = t1.join().unwrap();
    let locs_b = t2.join().unwrap();
    assert_eq!(manager.client_count(), 2);
    assert!(locs_a[0].file.contains("wt-a"));
    assert!(locs_b[0].file.contains("wt-b"));
}

#[test]
fn lsp_document_symbol_fallback_grouped_diagnostics_and_ignore_discovery() {
    use codemode_core::config::IgnoreConfig;
    use codemode_core::languages::LanguageId;
    use codemode_lsp::{Diagnostic, discover_project_files, group_diagnostics};

    let root = std::env::temp_dir().join(format!("codemode-lsp-fallback-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("gen")).unwrap();
    std::fs::write(root.join("shard.yml"), "name: demo\n").unwrap();
    std::fs::write(root.join(".gitignore"), "gen/\n").unwrap();
    let cr = root.join("src/main.cr");
    let skipped = root.join("src/skip.cr");
    let gen_cr = root.join("gen/out.cr");
    std::fs::write(&cr, "class Greeter\n  def hi\n    1\n  end\nend\n").unwrap();
    std::fs::write(&skipped, "def s; end\n").unwrap();
    std::fs::write(&gen_cr, "def g; end\n").unwrap();

    let script = r#"
import json, sys
def read_msg():
    n = 0
    while True:
        line = sys.stdin.buffer.readline()
        if not line: return None
        s = line.decode().strip()
        if not s: break
        if s.lower().startswith("content-length:"): n = int(s.split(":", 1)[1])
    return json.loads(sys.stdin.buffer.read(n))
def send_msg(obj):
    b = json.dumps(obj).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "textDocument/documentSymbol":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": [{
            "name": "Greeter",
            "kind": 5,
            "range": {"start": {"line": 0, "character": 0}, "end": {"line": 4, "character": 3}},
            "selectionRange": {"start": {"line": 0, "character": 6}, "end": {"line": 0, "character": 13}},
            "children": [{
                "name": "hi",
                "kind": 6,
                "range": {"start": {"line": 1, "character": 2}, "end": {"line": 3, "character": 5}},
                "selectionRange": {"start": {"line": 1, "character": 6}, "end": {"line": 1, "character": 8}}
            }]
        }]})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
    let mut cfg = ProjectConfig {
        ignore: IgnoreConfig {
            extra_patterns: vec!["skip.cr".into()],
        },
        ..ProjectConfig::default()
    };
    cfg.lsp_overrides.insert(
        "crystal".into(),
        write_mock_lsp(&root, "mock_cr.py", script),
    );

    let manager = LspManager::new();
    let client = manager.client_for(&cr, Some(&cfg)).unwrap();
    let symbols = client.document_symbols(&cr).unwrap();
    assert_eq!(symbols[0].children[0].name_path, "Greeter/hi");

    let grouped = group_diagnostics(
        &cr,
        &[Diagnostic {
            line: 2,
            column: 3,
            end_line: 2,
            end_column: 5,
            severity: "error".into(),
            message: "type error".into(),
            code: None,
            source: Some("crystalline".into()),
        }],
        Some(&symbols),
        Some(&cfg),
    )
    .unwrap();
    let file_key = cr.display().to_string();
    assert_eq!(
        grouped[&file_key]["Error"]["Greeter/hi"][0]["message"],
        "type error"
    );

    let (files, capped) =
        discover_project_files(&root, Some(LanguageId::Crystal), None, 20, Some(&cfg));
    assert!(!capped);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0], std::fs::canonicalize(&cr).unwrap());
}
