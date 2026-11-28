//! Scoped per-invocation Unix socket RPC bridge for `mode = "codemode"` scripts.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use serde_json::{Value, json};

static SOCK_SEQ: AtomicU64 = AtomicU64::new(1);

/// Callback invoked by sandboxed `codemode` scripts to execute `code_*` tools locally.
pub type ToolRpcHandler = Arc<dyn Fn(&str, Value) -> Result<String, String> + Send + Sync>;

/// Per-invocation Unix domain socket bridge serving `codemode.*` script calls.
pub struct CodemodeBridge {
    sock_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl CodemodeBridge {
    /// Starts a scoped Unix domain socket listener for one `code_execute` run.
    pub fn start(handler: ToolRpcHandler) -> std::io::Result<Self> {
        let seq = SOCK_SEQ.fetch_add(1, Ordering::Relaxed);
        let sock_path =
            std::env::temp_dir().join(format!("codemode-rpc-{}-{seq}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path)?;
        let _ = std::fs::set_permissions(&sock_path, std::fs::Permissions::from_mode(0o600));
        listener.set_nonblocking(true)?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&shutdown);
        let handle = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        handle_connection(stream, &handler);
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            sock_path,
            shutdown,
            handle: Some(handle),
        })
    }

    /// Returns the filesystem path to the active Unix socket.
    pub fn socket_path(&self) -> &Path {
        &self.sock_path
    }
}

impl Drop for CodemodeBridge {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

fn handle_connection(stream: UnixStream, handler: &ToolRpcHandler) {
    let Ok(writer_stream) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut writer = writer_stream;
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            let reply = match serde_json::from_str::<Value>(trimmed) {
                Ok(req) => {
                    let tool = req.get("tool").and_then(Value::as_str).unwrap_or("");
                    let args = req.get("arguments").cloned().unwrap_or_else(|| json!({}));
                    if matches!(tool, "code_execute" | "code_batch") {
                        json!({"ok": false, "error": format!("recursive {tool} is disallowed inside codemode scripts")})
                    } else {
                        match handler(tool, args) {
                            Ok(text) => {
                                let parsed: Value = match serde_json::from_str(&text) {
                                    Ok(v) => v,
                                    Err(_) => Value::String(text),
                                };
                                json!({"ok": true, "result": parsed})
                            }
                            Err(err) => json!({"ok": false, "error": err}),
                        }
                    }
                }
                Err(err) => json!({"ok": false, "error": format!("invalid RPC JSON: {err}")}),
            };
            let _ = writeln!(writer, "{reply}");
            let _ = writer.flush();
        }
        line.clear();
    }
}

/// Wraps a user script with the typed `codemode` runtime helper for Python or Node.
pub fn wrap_script_with_codemode(
    runtime: &str,
    code: &str,
    sock_path: &Path,
) -> Result<(String, String), String> {
    let norm = runtime.trim().to_ascii_lowercase();
    match norm.as_str() {
        "" | "auto" | "python" | "python3" | "py" => {
            let prelude = python_prelude(sock_path);
            Ok(("python3".into(), format!("{prelude}\n{code}")))
        }
        "node" | "js" | "javascript" => {
            let prelude = node_prelude(sock_path);
            Ok(("node".into(), format!("{prelude}\n{code}")))
        }
        other => Err(format!(
            "mode='codemode' requires python3 or node runtime, got '{other}'"
        )),
    }
}

fn python_prelude(sock_path: &Path) -> String {
    format!(
        r#"import json as _cm_json, os as _cm_os, socket as _cm_socket
class _CodeModeBridge:
    def __init__(self, sock):
        self._sock = sock
    def call(self, tool, arguments=None):
        s = _cm_socket.socket(_cm_socket.AF_UNIX, _cm_socket.SOCK_STREAM)
        s.connect(self._sock)
        payload = _cm_json.dumps({{"tool": tool, "arguments": arguments or {{}}}}) + "\n"
        s.sendall(payload.encode("utf-8"))
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
        s.close()
        resp = _cm_json.loads(buf.decode("utf-8"))
        if not resp.get("ok"):
            raise RuntimeError(resp.get("error", "codemode rpc error"))
        return resp.get("result")
    def outline(self, **kw): return self.call("code_outline", kw)
    def read(self, **kw): return self.call("code_read", kw)
    def references(self, **kw): return self.call("code_references", kw)
    def diagnostics(self, **kw): return self.call("code_diagnostics", kw)
    def edit(self, **kw): return self.call("code_edit", kw)
    def index(self, **kw): return self.call("code_index", kw)
    def search(self, **kw): return self.call("code_search", kw)
    def resume(self, **kw): return self.call("code_resume", kw)
    def memory(self, **kw): return self.call("code_memory", kw)
codemode = _CodeModeBridge(_cm_os.environ.get("CODEMODE_RPC_SOCK", {sock:?}))
"#,
        sock = sock_path.display().to_string()
    )
}

fn node_prelude(sock_path: &Path) -> String {
    format!(
        r#"const _cmCp = require('child_process');
const _cmSock = process.env.CODEMODE_RPC_SOCK || {sock:?};
function _cmCall(tool, args) {{
  const script = "const net=require('net');const c=net.createConnection(process.argv[1],()=>{{c.write(process.argv[2]+'\\n');}});let d='';c.on('data',b=>{{d+=b.toString();if(d.endsWith('\\n')){{process.stdout.write(d);c.end();}}}});";
  const r = _cmCp.spawnSync(process.execPath, ['-e', script, _cmSock, JSON.stringify({{tool, arguments: args || {{}}}})], {{encoding: 'utf8'}});
  if (r.status !== 0) throw new Error(r.stderr || 'codemode rpc failed');
  const resp = JSON.parse(r.stdout.trim());
  if (!resp.ok) throw new Error(resp.error || 'codemode rpc error');
  return resp.result;
}}
const codemode = {{
  call: _cmCall,
  outline: (a) => _cmCall('code_outline', a),
  read: (a) => _cmCall('code_read', a),
  references: (a) => _cmCall('code_references', a),
  diagnostics: (a) => _cmCall('code_diagnostics', a),
  edit: (a) => _cmCall('code_edit', a),
  index: (a) => _cmCall('code_index', a),
  search: (a) => _cmCall('code_search', a),
  resume: (a) => _cmCall('code_resume', a),
  memory: (a) => _cmCall('code_memory', a),
}};
"#,
        sock = sock_path.display().to_string()
    )
}
