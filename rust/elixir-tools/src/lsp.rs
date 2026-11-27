//! Expert LSP initialization, `mix.exs` discovery, progress readiness, and JSON-RPC stdio handshake.
//!
//! Mirrors `ElixirTools.__init__`, `_find_mix_exs`, `_create_base_initialize_params`, and
//! `_start_server` in `src/solidlsp/language_servers/elixir_tools/elixir_tools.py`.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use url::Url;

use crate::runtime::{ElixirToolsError, LogLevel, LogSink};

/// Default cross-file referencing settle time (`_get_wait_time_for_cross_file_referencing`).
pub const DEFAULT_WAIT_TIME_FOR_CROSS_FILE_REFERENCING_SECS: f64 = 10.0;

/// Default Expert LSP request timeout (`set_request_timeout(180.0)`).
pub const DEFAULT_REQUEST_TIMEOUT_SECS: f64 = 180.0;

/// Default Expert compilation/indexing readiness timeout (`ready_timeout = 300.0`).
pub const DEFAULT_READY_TIMEOUT_SECS: f64 = 300.0;

/// Cache fingerprint for `_normalize_symbol_name` (`_document_symbols_cache_fingerprint`).
pub const DOCUMENT_SYMBOLS_CACHE_FINGERPRINT: u32 = 1;

/// Launch metadata matching `ProcessLaunchInfo(cmd=f"{expert_executable_path} --stdio", cwd=repository_root_path)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessLaunchInfo {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

impl ProcessLaunchInfo {
    pub fn for_expert(executable: impl Into<PathBuf>, repository_root: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            args: vec!["--stdio".to_string()],
            cwd: repository_root.into(),
        }
    }

    /// Returns the shell-style command string matching Python's `f"{expert_executable_path} --stdio"`.
    pub fn command_string(&self) -> String {
        if self.args.is_empty() {
            self.executable.display().to_string()
        } else {
            format!("{} {}", self.executable.display(), self.args.join(" "))
        }
    }
}

/// Converts a filesystem path to a `file://` URI matching `pathlib.Path.as_uri()`.
pub fn path_to_file_uri(path: &Path) -> Result<String, ElixirToolsError> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    Url::from_file_path(&abs).map(|u| u.into()).map_err(|()| {
        ElixirToolsError::InvalidUrl(format!(
            "cannot convert path to file URI: {}",
            abs.display()
        ))
    })
}

/// Finds `mix.exs` in `repository_root` or an immediate subdirectory (monorepo layout).
///
/// Mirrors `ElixirTools._find_mix_exs`. Subdirectories are scanned in sorted order for
/// deterministic resolution when multiple matches exist.
pub fn find_mix_exs(repository_root: &Path, logger: &dyn LogSink) -> Option<PathBuf> {
    let root_mix_exs = repository_root.join("mix.exs");
    if root_mix_exs.exists() {
        return Some(root_mix_exs);
    }

    let read_dir = match fs::read_dir(repository_root) {
        Ok(entries) => entries,
        Err(err) => {
            logger.log(
                LogLevel::Warning,
                &format!("Error scanning for mix.exs in subdirectories: {err}"),
            );
            return None;
        }
    };

    let mut subdirs: Vec<PathBuf> = read_dir
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    subdirs.sort();

    for subdir_path in subdirs {
        let subdir_mix_exs = subdir_path.join("mix.exs");
        if subdir_mix_exs.exists() {
            if let Some(entry_name) = subdir_path.file_name().and_then(|n| n.to_str()) {
                logger.log(
                    LogLevel::Info,
                    &format!("Found mix.exs in subdirectory: {entry_name}/mix.exs"),
                );
            }
            return Some(subdir_mix_exs);
        }
    }

    None
}

/// Returns the LSP `initialize` params for the Expert language server.
///
/// Mirrors `ElixirTools._create_base_initialize_params`.
pub fn create_base_initialize_params() -> Value {
    let symbol_kinds: Vec<u32> = (1..27).collect();
    json!({
        "locale": "en",
        "initializationOptions": {
            "mix_env": "dev",
            "mix_target": "host",
            "experimental": {
                "completions": {
                    "enable": false
                }
            },
            "extensions": {
                "credo": {
                    "enable": true,
                    "cli_options": []
                }
            }
        },
        "capabilities": {
            "textDocument": {
                "synchronization": {
                    "didSave": true,
                    "dynamicRegistration": true
                },
                "completion": {
                    "dynamicRegistration": true,
                    "completionItem": {
                        "snippetSupport": true,
                        "documentationFormat": ["markdown", "plaintext"]
                    }
                },
                "definition": {
                    "dynamicRegistration": true
                },
                "references": {
                    "dynamicRegistration": true
                },
                "documentSymbol": {
                    "dynamicRegistration": true,
                    "hierarchicalDocumentSymbolSupport": true,
                    "symbolKind": {
                        "valueSet": symbol_kinds
                    }
                },
                "hover": {
                    "dynamicRegistration": true,
                    "contentFormat": ["markdown", "plaintext"]
                },
                "formatting": {
                    "dynamicRegistration": true
                },
                "codeAction": {
                    "dynamicRegistration": true,
                    "codeActionLiteralSupport": {
                        "codeActionKind": {
                            "valueSet": [
                                "quickfix",
                                "refactor",
                                "refactor.extract",
                                "refactor.inline",
                                "refactor.rewrite",
                                "source",
                                "source.organizeImports"
                            ]
                        }
                    }
                }
            },
            "workspace": {
                "workspaceFolders": true,
                "didChangeConfiguration": {
                    "dynamicRegistration": true
                },
                "executeCommand": {
                    "dynamicRegistration": true
                }
            },
            "window": {
                "showMessage": {
                    "messageActionItem": {
                        "additionalPropertiesSupport": true
                    }
                },
                "showDocument": {
                    "support": true
                },
                "workDoneProgress": true
            }
        }
    })
}

/// Classifies a `window/logMessage` notification into `(LogLevel, formatted_message)`.
///
/// Mirrors `window_log_message` inside `ElixirTools._start_server`:
/// type 1 -> Error, type 2 -> Warning, other (3/4/missing) -> Debug.
pub fn classify_window_log_message(msg: &Value) -> (LogLevel, String) {
    let message_type = msg.get("type").and_then(Value::as_i64).unwrap_or(4);
    let message_text = msg.get("message").and_then(Value::as_str).unwrap_or("");

    let level = match message_type {
        1 => LogLevel::Error,
        2 => LogLevel::Warning,
        _ => LogLevel::Debug,
    };
    (level, format!("Expert: {message_text}"))
}

/// Tracks Expert `$/progress` notifications to determine project build completion.
///
/// Mirrors `check_server_ready` inside `ElixirTools._start_server`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ProgressTracker {
    building_project: bool,
    server_ready: bool,
}

impl ProgressTracker {
    pub const fn new() -> Self {
        Self {
            building_project: false,
            server_ready: false,
        }
    }

    pub const fn is_building_project(&self) -> bool {
        self.building_project
    }

    pub const fn is_ready(&self) -> bool {
        self.server_ready
    }

    /// Processes a `$/progress` notification's `params` payload.
    ///
    /// Returns `true` if this notification transitioned `server_ready` from `false` to `true`.
    pub fn handle_progress(&mut self, params: &Value, logger: &dyn LogSink) -> bool {
        let value = params.get("value").unwrap_or(&Value::Null);
        let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");
        let title = value.get("title").and_then(Value::as_str).unwrap_or("");

        if kind == "begin" {
            if title.starts_with("Building ") && !title.starts_with("Building engine") {
                self.building_project = true;
            }
            false
        } else if kind == "end" {
            if self.building_project {
                logger.log(
                    LogLevel::Debug,
                    "Expert project build completed - server is ready",
                );
                self.building_project = false;
                let transitioned = !self.server_ready;
                self.server_ready = true;
                return transitioned;
            }
            false
        } else {
            false
        }
    }

    /// Marks the server ready after `ready_timeout` expires.
    pub fn force_ready(&mut self) {
        self.server_ready = true;
    }
}

/// Active background reader over the Expert stdout stream.
#[derive(Debug)]
pub struct ExpertMessageReader {
    rx: Receiver<Result<Option<Value>, ElixirToolsError>>,
    _handle: JoinHandle<()>,
}

impl ExpertMessageReader {
    /// Receives the next JSON-RPC message from Expert, waiting up to `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Option<Value>, ElixirToolsError> {
        match self.rx.recv_timeout(timeout) {
            Ok(item) => item,
            Err(RecvTimeoutError::Timeout) => Err(ElixirToolsError::LspProtocol(
                "timed out waiting for Expert LSP message".to_string(),
            )),
            Err(RecvTimeoutError::Disconnected) => Ok(None),
        }
    }
}

/// Result of running the Expert LSP startup handshake.
#[derive(Debug)]
pub struct StartupResult {
    pub initialize_response: Value,
    pub opened_mix_exs_uri: Option<String>,
    pub signaled_ready: bool,
    pub diagnostics_received: Vec<Value>,
    pub reader: ExpertMessageReader,
}

/// Writes one JSON-RPC message framed with `Content-Length`.
pub fn write_lsp_message<W: Write>(writer: &mut W, payload: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(payload)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

/// Reads one JSON-RPC message framed with `Content-Length`. Returns `Ok(None)` on clean EOF.
pub fn read_lsp_message<R: BufRead>(reader: &mut R) -> Result<Option<Value>, ElixirToolsError> {
    let mut content_length: Option<usize> = None;
    let mut saw_any_header = false;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            if !saw_any_header {
                return Ok(None);
            }
            return Err(ElixirToolsError::LspProtocol(
                "unexpected EOF while reading LSP headers".to_string(),
            ));
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if saw_any_header {
                break;
            }
            continue;
        }
        saw_any_header = true;

        if let Some((name, value)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let parsed = value.trim().parse::<usize>().map_err(|_| {
                ElixirToolsError::LspProtocol(format!("invalid Content-Length: {value}"))
            })?;
            content_length = Some(parsed);
        }
    }

    let length = content_length.ok_or_else(|| {
        ElixirToolsError::LspProtocol("missing Content-Length header".to_string())
    })?;
    let mut buf = vec![0u8; length];
    reader.read_exact(&mut buf)?;
    let msg = serde_json::from_slice(&buf)
        .map_err(|err| ElixirToolsError::LspProtocol(format!("invalid JSON payload: {err}")))?;
    Ok(Some(msg))
}

fn spawn_message_reader<R: Read + Send + 'static>(
    reader: R,
) -> (
    Receiver<Result<Option<Value>, ElixirToolsError>>,
    JoinHandle<()>,
) {
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let mut buf_reader = BufReader::new(reader);
        loop {
            let item = read_lsp_message(&mut buf_reader);
            let done = matches!(item, Ok(None) | Err(_));
            if tx.send(item).is_err() || done {
                break;
            }
        }
    });
    (rx, handle)
}

fn handle_incoming_message<W: Write>(
    msg: &Value,
    writer: &mut W,
    tracker: &mut ProgressTracker,
    diagnostics: &mut Vec<Value>,
    logger: &dyn LogSink,
) -> Result<(), ElixirToolsError> {
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id");

    match (method, id) {
        (Some("client/registerCapability"), Some(req_id)) => {
            let params = msg.get("params").unwrap_or(&Value::Null);
            logger.log(
                LogLevel::Debug,
                &format!("LSP: client/registerCapability: {params}"),
            );
            let reply = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": null
            });
            write_lsp_message(writer, &reply)?;
        }
        (Some("window/workDoneProgress/create"), Some(req_id)) => {
            let reply = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": null
            });
            write_lsp_message(writer, &reply)?;
        }
        (Some(_), Some(req_id)) => {
            let reply = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": null
            });
            write_lsp_message(writer, &reply)?;
        }
        (Some("window/logMessage"), None) => {
            let params = msg.get("params").unwrap_or(&Value::Null);
            let (level, formatted) = classify_window_log_message(params);
            logger.log(level, &formatted);
        }
        (Some("$/progress"), None) => {
            let params = msg.get("params").unwrap_or(&Value::Null);
            tracker.handle_progress(params, logger);
        }
        (Some("textDocument/publishDiagnostics"), None) => {
            if let Some(params) = msg.get("params") {
                diagnostics.push(params.clone());
            }
        }
        _ => {}
    }

    Ok(())
}

/// Runs the Expert LSP startup handshake over arbitrary stdio streams.
///
/// 1. Sends `initialize` using [`create_base_initialize_params`] and validates `capabilities.textDocumentSync`.
/// 2. Sends `initialized`.
/// 3. If `mix.exs` is found via [`find_mix_exs`], opens it with `textDocument/didOpen` to trigger Expert's build pipeline.
/// 4. Pumps LSP requests/notifications up to `ready_timeout` until `$/progress` signals project build completion (or proceeds after timeout).
/// 5. If `mix.exs` was opened, closes it with `textDocument/didClose`.
pub fn run_expert_startup_handshake<W, R>(
    writer: &mut W,
    reader: R,
    repository_root: &Path,
    ready_timeout: Duration,
    logger: &dyn LogSink,
) -> Result<StartupResult, ElixirToolsError>
where
    W: Write,
    R: Read + Send + 'static,
{
    let (rx, reader_handle) = spawn_message_reader(reader);
    let mut tracker = ProgressTracker::new();
    let mut diagnostics = Vec::new();

    logger.log(LogLevel::Debug, "Sending initialize request to Expert");
    let init_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": create_base_initialize_params()
    });
    write_lsp_message(writer, &init_request)?;

    let init_response = loop {
        let msg = match rx.recv() {
            Ok(Ok(Some(msg))) => msg,
            Ok(Ok(None)) => {
                return Err(ElixirToolsError::LspProtocol(
                    "Expert closed stdout before answering initialize".to_string(),
                ));
            }
            Ok(Err(err)) => return Err(err),
            Err(_) => {
                return Err(ElixirToolsError::LspProtocol(
                    "LSP reader terminated unexpectedly".to_string(),
                ));
            }
        };

        if msg.get("id").and_then(Value::as_i64) == Some(1) {
            if let Some(err_obj) = msg.get("error") {
                return Err(ElixirToolsError::LspProtocol(format!(
                    "initialize failed: {err_obj}"
                )));
            }
            break msg.get("result").cloned().ok_or_else(|| {
                ElixirToolsError::LspProtocol("initialize missing result".to_string())
            })?;
        }

        handle_incoming_message(&msg, writer, &mut tracker, &mut diagnostics, logger)?;
    };

    let has_text_document_sync = init_response
        .get("capabilities")
        .and_then(|caps| caps.get("textDocumentSync"))
        .is_some();
    if !has_text_document_sync {
        return Err(ElixirToolsError::MissingCapability(
            "textDocumentSync".to_string(),
        ));
    }

    let initialized_notif = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    write_lsp_message(writer, &initialized_notif)?;

    let mix_exs_path = find_mix_exs(repository_root, logger);
    let mix_exs_uri = if let Some(path) = mix_exs_path {
        let uri = path_to_file_uri(&path)?;
        let content = fs::read_to_string(&path)?;
        let did_open = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": uri,
                    "languageId": "elixir",
                    "version": 1,
                    "text": content
                }
            }
        });
        write_lsp_message(writer, &did_open)?;
        logger.log(
            LogLevel::Debug,
            "Opened mix.exs to trigger Expert's build pipeline",
        );
        Some(uri)
    } else {
        None
    };

    logger.log(
        LogLevel::Debug,
        &format!(
            "Waiting up to {}s for Expert to compile and index...",
            ready_timeout.as_secs_f64()
        ),
    );

    let signaled_ready = if tracker.is_ready() {
        true
    } else {
        let deadline = Instant::now() + ready_timeout;
        loop {
            let now = Instant::now();
            if now >= deadline {
                break false;
            }
            let remaining = deadline.saturating_duration_since(now);
            match rx.recv_timeout(remaining) {
                Ok(Ok(Some(msg))) => {
                    handle_incoming_message(&msg, writer, &mut tracker, &mut diagnostics, logger)?;
                    if tracker.is_ready() {
                        break true;
                    }
                }
                Ok(Ok(None)) => break tracker.is_ready(),
                Ok(Err(err)) => return Err(err),
                Err(RecvTimeoutError::Timeout) => break false,
                Err(RecvTimeoutError::Disconnected) => break tracker.is_ready(),
            }
        }
    };

    if signaled_ready {
        logger.log(LogLevel::Debug, "Expert is ready for requests");
    } else {
        logger.log(
            LogLevel::Warning,
            &format!(
                "Expert did not signal readiness within {}s. Proceeding with requests anyway.",
                ready_timeout.as_secs_f64()
            ),
        );
        tracker.force_ready();
    }

    if let Some(uri) = &mix_exs_uri {
        let did_close = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didClose",
            "params": {
                "textDocument": {
                    "uri": uri
                }
            }
        });
        write_lsp_message(writer, &did_close)?;
        logger.log(LogLevel::Debug, "Closed mix.exs after Expert is ready");
    }

    Ok(StartupResult {
        initialize_response: init_response,
        opened_mix_exs_uri: mix_exs_uri,
        signaled_ready,
        diagnostics_received: diagnostics,
        reader: ExpertMessageReader {
            rx,
            _handle: reader_handle,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(1);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "elixir-tools-lsp-test-{prefix}-{}-{unique}-{seq}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Default)]
    struct RecordingLogger {
        entries: RefCell<Vec<(LogLevel, String)>>,
    }

    impl LogSink for RecordingLogger {
        fn log(&self, level: LogLevel, message: &str) {
            self.entries.borrow_mut().push((level, message.to_string()));
        }
    }

    #[test]
    fn find_mix_exs_checks_root_then_immediate_subdirs() {
        let tmp = TempDir::new("mix");
        let logger = RecordingLogger::default();
        assert_eq!(find_mix_exs(&tmp.path, &logger), None);

        let sub = tmp.path.join("server");
        fs::create_dir_all(&sub).unwrap();
        let sub_mix = sub.join("mix.exs");
        fs::write(&sub_mix, "defmodule Server.MixProject do\nend\n").unwrap();
        assert_eq!(find_mix_exs(&tmp.path, &logger), Some(sub_mix));

        let root_mix = tmp.path.join("mix.exs");
        fs::write(&root_mix, "defmodule Root.MixProject do\nend\n").unwrap();
        assert_eq!(find_mix_exs(&tmp.path, &logger), Some(root_mix));
    }

    #[test]
    fn progress_tracker_ignores_building_engine_and_orphan_end() {
        let logger = RecordingLogger::default();
        let mut tracker = ProgressTracker::new();

        // Orphan end must not mark ready.
        assert!(!tracker.handle_progress(&json!({"value": {"kind": "end"}}), &logger));
        assert!(!tracker.is_ready());

        // "Building engine" must not set building_project or mark ready.
        tracker.handle_progress(
            &json!({"value": {"kind": "begin", "title": "Building engine"}}),
            &logger,
        );
        assert!(!tracker.is_building_project());
        tracker.handle_progress(&json!({"value": {"kind": "end"}}), &logger);
        assert!(!tracker.is_ready());

        // "Building my_app" followed by end marks ready.
        tracker.handle_progress(
            &json!({"value": {"kind": "begin", "title": "Building my_app"}}),
            &logger,
        );
        assert!(tracker.is_building_project());
        assert!(tracker.handle_progress(&json!({"value": {"kind": "end"}}), &logger));
        assert!(tracker.is_ready());
        assert!(!tracker.is_building_project());
    }

    #[test]
    fn window_log_message_maps_types_correctly() {
        assert_eq!(
            classify_window_log_message(&json!({"type": 1, "message": "boom"})),
            (LogLevel::Error, "Expert: boom".to_string())
        );
        assert_eq!(
            classify_window_log_message(&json!({"type": 2, "message": "warn"})),
            (LogLevel::Warning, "Expert: warn".to_string())
        );
        assert_eq!(
            classify_window_log_message(&json!({"type": 3, "message": "info"})),
            (LogLevel::Debug, "Expert: info".to_string())
        );
        assert_eq!(
            classify_window_log_message(&json!({})),
            (LogLevel::Debug, "Expert: ".to_string())
        );
    }

    #[test]
    fn startup_handshake_drives_mock_stdio_expert_server() {
        let tmp = TempDir::new("handshake");
        let mix_exs = tmp.path.join("mix.exs");
        fs::write(&mix_exs, "defmodule Demo.MixProject do\nend\n").unwrap();

        let trace_path = tmp.path.join("server_trace.json");
        let mock_server_script = tmp.path.join("mock_expert.py");
        fs::write(
            &mock_server_script,
            r#"import json, sys

def read_msg():
    content_length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        stripped = line.rstrip(b"\r\n")
        if not stripped:
            break
        if b":" in stripped:
            k, v = stripped.split(b":", 1)
            if k.strip().lower() == b"content-length":
                content_length = int(v.strip())
    body = sys.stdin.buffer.read(content_length)
    return json.loads(body.decode("utf-8"))

def write_msg(obj):
    raw = json.dumps(obj).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(raw)}\r\n\r\n".encode("ascii") + raw)
    sys.stdout.buffer.flush()

trace = []
while True:
    msg = read_msg()
    if msg is None:
        break
    trace.append(msg)
    method = msg.get("method")
    if method == "initialize":
        write_msg({"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 2, "message": "warming up"}})
        write_msg({"jsonrpc": "2.0", "id": 10, "method": "client/registerCapability", "params": {"registrations": []}})
        reg_reply = read_msg()
        trace.append(reg_reply)
        write_msg({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {"textDocumentSync": 1}}})
    elif method == "textDocument/didOpen":
        write_msg({"jsonrpc": "2.0", "id": 11, "method": "window/workDoneProgress/create", "params": {"token": "build"}})
        prog_reply = read_msg()
        trace.append(prog_reply)
        write_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "engine", "value": {"kind": "begin", "title": "Building engine"}}})
        write_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "engine", "value": {"kind": "end"}}})
        write_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "build", "value": {"kind": "begin", "title": "Building demo"}}})
        write_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "build", "value": {"kind": "end"}}})
    elif method == "textDocument/didClose":
        break

with open(sys.argv[1], "w", encoding="utf-8") as f:
    json.dump(trace, f)
"#,
        )
        .unwrap();

        let mut child = Command::new("python3")
            .arg(&mock_server_script)
            .arg(&trace_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let logger = RecordingLogger::default();

        let result = run_expert_startup_handshake(
            &mut stdin,
            stdout,
            &tmp.path,
            Duration::from_secs(5),
            &logger,
        )
        .unwrap();

        drop(stdin);
        let status = child.wait().unwrap();
        assert!(status.success());

        assert!(result.signaled_ready);
        let expected_uri = path_to_file_uri(&mix_exs).unwrap();
        assert_eq!(
            result.opened_mix_exs_uri.as_deref(),
            Some(expected_uri.as_str())
        );

        let trace: Vec<Value> =
            serde_json::from_str(&fs::read_to_string(&trace_path).unwrap()).unwrap();
        let methods: Vec<&str> = trace
            .iter()
            .filter_map(|m| m.get("method").and_then(Value::as_str))
            .collect();
        assert_eq!(
            methods,
            vec![
                "initialize",
                "initialized",
                "textDocument/didOpen",
                "textDocument/didClose"
            ]
        );
        assert!(
            logger
                .entries
                .borrow()
                .iter()
                .any(|(lvl, msg)| { *lvl == LogLevel::Warning && msg == "Expert: warming up" })
        );
    }

    #[test]
    fn startup_handshake_handles_timeout_and_missing_capability() {
        let tmp = TempDir::new("timeout-and-cap");
        let logger = RecordingLogger::default();

        // Missing textDocumentSync capability returns MissingCapability.
        let mut child = Command::new("python3")
            .arg("-c")
            .arg(
                "import json, sys; \
                 line=sys.stdin.buffer.readline(); \
                 n=int(line.split(b':')[1]); sys.stdin.buffer.readline(); sys.stdin.buffer.read(n); \
                 raw=json.dumps({'jsonrpc':'2.0','id':1,'result':{'capabilities':{}}}).encode(); \
                 sys.stdout.buffer.write(f'Content-Length: {len(raw)}\\r\\n\\r\\n'.encode()+raw); sys.stdout.buffer.flush()",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let err = run_expert_startup_handshake(
            &mut stdin,
            stdout,
            &tmp.path,
            Duration::from_secs(1),
            &logger,
        )
        .unwrap_err();
        assert!(
            matches!(err, ElixirToolsError::MissingCapability(ref c) if c == "textDocumentSync")
        );
        drop(stdin);
        let _ = child.wait();

        // Timeout proceeds with warning when Expert never sends $/progress end.
        let mut slow_child = Command::new("python3")
            .arg("-c")
            .arg(
                "import json, sys, time; \
                 line=sys.stdin.buffer.readline(); \
                 n=int(line.split(b':')[1]); sys.stdin.buffer.readline(); sys.stdin.buffer.read(n); \
                 raw=json.dumps({'jsonrpc':'2.0','id':1,'result':{'capabilities':{'textDocumentSync':1}}}).encode(); \
                 sys.stdout.buffer.write(f'Content-Length: {len(raw)}\\r\\n\\r\\n'.encode()+raw); sys.stdout.buffer.flush(); \
                 time.sleep(1.5)",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut slow_stdin = slow_child.stdin.take().unwrap();
        let slow_stdout = slow_child.stdout.take().unwrap();
        let slow_started = Instant::now();
        let slow_res = run_expert_startup_handshake(
            &mut slow_stdin,
            slow_stdout,
            &tmp.path,
            Duration::from_millis(100),
            &logger,
        )
        .unwrap();
        assert!(slow_started.elapsed() < Duration::from_millis(800));
        assert!(!slow_res.signaled_ready);
        assert_eq!(slow_res.opened_mix_exs_uri, None);
        drop(slow_stdin);
        let _ = slow_child.wait();
    }

    #[test]
    fn lsp_framing_handles_case_insensitive_headers_extra_headers_and_errors() {
        let mut cursor = io::Cursor::new(
            b"content-type: application/vscode-jsonrpc; charset=utf-8\r\ncontent-length: 13\r\n\r\n{\"ok\":true}\n ",
        );
        let msg = read_lsp_message(&mut cursor).unwrap().unwrap();
        assert_eq!(msg, json!({"ok": true}));

        let mut eof_cursor = io::Cursor::new(b"");
        assert_eq!(read_lsp_message(&mut eof_cursor).unwrap(), None);

        let mut missing_len = io::Cursor::new(b"Content-Type: text/plain\r\n\r\n");
        assert!(matches!(
            read_lsp_message(&mut missing_len),
            Err(ElixirToolsError::LspProtocol(_))
        ));

        let mut bad_json = io::Cursor::new(b"Content-Length: 3\r\n\r\n{{{");
        assert!(matches!(
            read_lsp_message(&mut bad_json),
            Err(ElixirToolsError::LspProtocol(_))
        ));
    }
}
