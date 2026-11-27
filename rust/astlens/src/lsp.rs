//! Lazy, per-worktree LSP multiplexer. Clients are keyed by (language, workspace
//! root); every query re-syncs the file from disk first, so edits made by the
//! host's native tools never leave a stale buffer behind.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::Serialize;
use serde_json::{Value, json};

use crate::roots;
use crate::ts::Lang;

type Pending = Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, String>>>>>;
type DiagnosticsStore = Arc<Mutex<HashMap<String, Vec<Value>>>>;
type FileState = (u128, u64, i64);

pub struct LspManager {
    clients: DashMap<(String, PathBuf), Arc<LspClient>>,
}

impl LspManager {
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
        }
    }

    pub fn client_for(&self, path: &Path) -> Result<Arc<LspClient>, String> {
        let lang = Lang::from_path(path)
            .ok_or_else(|| format!("unsupported file type: {}", path.display()))?;
        let root = roots::workspace_root(path, lang);
        let key = (lang.name().to_string(), root.clone());
        if let Some(client) = self.clients.get(&key) {
            return Ok(Arc::clone(client.value()));
        }
        let command = roots::server_command(lang)?;
        let client = Arc::new(LspClient::spawn(&command, &root, lang)?);
        self.clients.insert(key, Arc::clone(&client));
        Ok(client)
    }
}

pub struct LspClient {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    diagnostics: DiagnosticsStore,
    opened: Mutex<HashMap<PathBuf, FileState>>,
    next_id: AtomicI64,
    settled: AtomicBool,
    ready: Arc<AtomicBool>,
    root: PathBuf,
    language: Lang,
}

#[derive(Debug, Serialize)]
pub struct Location {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub preview: String,
}

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl LspClient {
    pub fn spawn(command: &[String], root: &Path, language: Lang) -> Result<Self, String> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", command.join(" ")))?;
        let stdin = child.stdin.take().ok_or("language server has no stdin")?;
        let stdout = child.stdout.take().ok_or("language server has no stdout")?;

        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics: DiagnosticsStore = Arc::new(Mutex::new(HashMap::new()));
        let writer = Arc::new(Mutex::new(stdin));
        let ready = Arc::new(AtomicBool::new(false));

        let reader_pending = Arc::clone(&pending);
        let reader_diagnostics = Arc::clone(&diagnostics);
        let reader_writer = Arc::clone(&writer);
        let reader_ready = Arc::clone(&ready);
        std::thread::spawn(move || {
            reader_loop(
                stdout,
                reader_pending,
                reader_diagnostics,
                reader_writer,
                reader_ready,
            )
        });

        let client = Self {
            child: Mutex::new(child),
            stdin: writer,
            pending,
            diagnostics,
            opened: Mutex::new(HashMap::new()),
            next_id: AtomicI64::new(1),
            settled: AtomicBool::new(false),
            ready,
            root: root.to_path_buf(),
            language,
        };
        client.initialize()?;
        Ok(client)
    }

    fn initialize(&self) -> Result<(), String> {
        let root_uri = file_uri(&self.root)?;
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "astlens", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root_uri,
            "workspaceFolders": [{
                "uri": root_uri,
                "name": self.root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace")
            }],
            "capabilities": {
                "workspace": {"workspaceEdit": {"documentChanges": true}, "workspaceFolders": true},
                "textDocument": {
                    "synchronization": {"didSave": false, "dynamicRegistration": false},
                    "definition": {"linkSupport": true},
                    "references": {},
                    "implementation": {},
                    "rename": {},
                    "publishDiagnostics": {"relatedInformation": true},
                    "diagnostic": {}
                }
            }
        });
        self.request("initialize", params, Duration::from_secs(60))?;
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "pending map poisoned")?
            .insert(id, sender);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = write_message(
            &mut *self.stdin.lock().map_err(|_| "stdin poisoned")?,
            &message,
        ) {
            self.pending
                .lock()
                .map_err(|_| "pending map poisoned")?
                .remove(&id);
            return Err(format!("write to language server failed: {error}"));
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                self.pending
                    .lock()
                    .map_err(|_| "pending map poisoned")?
                    .remove(&id);
                Err(format!(
                    "language server did not answer {method} in {timeout:?}"
                ))
            }
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        write_message(
            &mut *self.stdin.lock().map_err(|_| "stdin poisoned")?,
            &message,
        )
        .map_err(|error| format!("write to language server failed: {error}"))
    }

    /// Send `didOpen` the first time, `didChange` whenever disk truth moved.
    /// Returns true when the server just received a fresh buffer.
    pub fn ensure_open(&self, path: &Path) -> Result<bool, String> {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let source = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let meta = std::fs::metadata(&path)
            .map_err(|error| format!("cannot stat {}: {error}", path.display()))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let signature = (modified, meta.len());
        let uri = file_uri(&path)?;

        let mut opened = self.opened.lock().map_err(|_| "opened map poisoned")?;
        let fresh = match opened.get_mut(&path) {
            Some((modified, len, _)) if (*modified, *len) == signature => false,
            Some((modified, len, version)) => {
                *modified = signature.0;
                *len = signature.1;
                *version += 1;
                let version = *version;
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": {"uri": uri, "version": version},
                        "contentChanges": [{"text": source}]
                    }),
                )?;
                true
            }
            None => {
                opened.insert(path, (signature.0, signature.1, 1));
                self.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": self.language.lsp_id(),
                            "version": 1,
                            "text": source
                        }
                    }),
                )?;
                true
            }
        };
        drop(opened);
        Ok(fresh)
    }

    /// shortcut: language servers index the project asynchronously after the first
    /// didOpen. One settle per client keeps cross-file rename/references correct.
    /// Upgrade to a server project-load notification if large repos stay incomplete.
    fn settle(&self) {
        if self.settled.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.language == Lang::Rust {
            // rust-analyzer reports quiescence; wait for it instead of a fixed sleep.
            let deadline = Instant::now() + Duration::from_secs(8);
            while !self.ready.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(100));
            }
            if !self.ready.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
            }
        } else {
            std::thread::sleep(Duration::from_millis(2000));
        }
    }

    pub fn references(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        kind: &str,
        include_declaration: bool,
    ) -> Result<Vec<Location>, String> {
        self.ensure_open(path)?;
        self.settle();
        let uri = file_uri(path)?;
        let position = json!({"line": line, "character": character});
        let (method, params) = match kind {
            "definition" => (
                "textDocument/definition",
                json!({"textDocument": {"uri": uri}, "position": position}),
            ),
            "implementation" => (
                "textDocument/implementation",
                json!({"textDocument": {"uri": uri}, "position": position}),
            ),
            _ => (
                "textDocument/references",
                json!({
                    "textDocument": {"uri": uri},
                    "position": position,
                    "context": {"includeDeclaration": include_declaration}
                }),
            ),
        };
        let value = self.request(method, params, Duration::from_secs(30))?;
        let items = match value {
            Value::Array(items) => items,
            Value::Null => Vec::new(),
            other => vec![other],
        };
        Ok(items.iter().filter_map(location_from_value).collect())
    }

    pub fn diagnostics(&self, path: &Path) -> Result<Vec<Diagnostic>, String> {
        self.ensure_open(path)?;
        let uri = file_uri(path)?;
        let deadline = Instant::now() + Duration::from_millis(1500);
        loop {
            let items = self
                .diagnostics
                .lock()
                .map_err(|_| "diagnostics map poisoned")?
                .get(&uri)
                .cloned();
            if let Some(items) = items {
                if !items.is_empty() || Instant::now() >= deadline {
                    return Ok(items.iter().filter_map(diagnostic_from_value).collect());
                }
            } else if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        // Servers with pull diagnostics answer this; others return MethodNotFound.
        match self.request(
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": uri}}),
            Duration::from_secs(10),
        ) {
            Ok(value) => Ok(value
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(diagnostic_from_value).collect())
                .unwrap_or_default()),
            Err(_) => Ok(Vec::new()),
        }
    }

    /// Apply an LSP rename and return `(file, unified diff)` for every file touched.
    pub fn rename(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Vec<(PathBuf, String)>, String> {
        self.ensure_open(path)?;
        self.settle();
        let uri = file_uri(path)?;
        let value = self.request(
            "textDocument/rename",
            json!({
                "textDocument": {"uri": uri},
                "position": {"line": line, "character": character},
                "newName": new_name
            }),
            Duration::from_secs(30),
        )?;
        let edits = workspace_edits(&value)?;
        let mut by_file: HashMap<PathBuf, Vec<(usize, usize, String)>> = HashMap::new();
        for (uri, edit) in edits {
            let file = uri_to_path(&uri)?;
            let text = std::fs::read_to_string(&file)
                .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
            let start = byte_of(&text, edit.start_line, edit.start_character);
            let end = byte_of(&text, edit.end_line, edit.end_character);
            by_file
                .entry(file)
                .or_default()
                .push((start, end, edit.new_text));
        }

        let mut changed = Vec::new();
        for (file, mut file_edits) in by_file {
            let old = std::fs::read_to_string(&file)
                .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
            file_edits.sort_by_key(|(start, _, _)| *start);
            let mut new = old.clone();
            for (start, end, text) in file_edits.into_iter().rev() {
                new.replace_range(start..end, &text);
            }
            std::fs::write(&file, &new)
                .map_err(|error| format!("cannot write {}: {error}", file.display()))?;
            changed.push((file.clone(), crate::diff::unified(&file, &old, &new)));
        }
        Ok(changed)
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        let _ = self.request("shutdown", Value::Null, Duration::from_secs(2));
        let _ = self.notify("exit", Value::Null);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct RawEdit {
    start_line: u32,
    start_character: u32,
    end_line: u32,
    end_character: u32,
    new_text: String,
}

fn workspace_edits(value: &Value) -> Result<Vec<(String, RawEdit)>, String> {
    let mut out = Vec::new();
    if let Some(changes) = value.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            for edit in edits.as_array().into_iter().flatten() {
                if let Some(edit) = raw_edit(edit) {
                    out.push((uri.clone(), edit));
                }
            }
        }
    }
    if let Some(changes) = value.get("documentChanges").and_then(Value::as_array) {
        for change in changes {
            let Some(uri) = change
                .get("textDocument")
                .and_then(|document| document.get("uri"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            for edit in change
                .get("edits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(edit) = raw_edit(edit) {
                    out.push((uri.to_string(), edit));
                }
            }
        }
    }
    if out.is_empty() {
        return Err(
            "language server returned no rename edits (is the cursor on a renameable symbol?)"
                .to_string(),
        );
    }
    Ok(out)
}

fn raw_edit(value: &Value) -> Option<RawEdit> {
    let range = value.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    Some(RawEdit {
        start_line: start.get("line")?.as_u64()? as u32,
        start_character: start.get("character")?.as_u64()? as u32,
        end_line: end.get("line")?.as_u64()? as u32,
        end_character: end.get("character")?.as_u64()? as u32,
        new_text: value.get("newText")?.as_str()?.to_string(),
    })
}

fn location_from_value(value: &Value) -> Option<Location> {
    let uri = value
        .get("uri")
        .or_else(|| value.get("targetUri"))
        .and_then(Value::as_str)?;
    let range = value
        .get("targetSelectionRange")
        .or_else(|| value.get("range"))
        .or_else(|| value.get("targetRange"))?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let file = uri_to_path(uri).ok()?;
    let source = std::fs::read_to_string(&file).unwrap_or_default();
    let line = start.get("line")?.as_u64()? as u32;
    let character = start.get("character")?.as_u64()? as u32;
    let end_line = end.get("line")?.as_u64()? as u32;
    let end_character = end.get("character")?.as_u64()? as u32;
    Some(Location {
        file: file.display().to_string(),
        line: line + 1,
        column: utf16_to_char(&source, line, character) + 1,
        end_line: end_line + 1,
        end_column: utf16_to_char(&source, end_line, end_character) + 1,
        preview: crate::ts::line_preview(&source, line as usize),
    })
}

fn diagnostic_from_value(value: &Value) -> Option<Diagnostic> {
    let range = value.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    let severity = match value.get("severity").and_then(Value::as_u64) {
        Some(1) => "error",
        Some(2) => "warning",
        Some(3) => "info",
        Some(4) => "hint",
        _ => "unknown",
    };
    Some(Diagnostic {
        line: start.get("line")?.as_u64()? as u32 + 1,
        column: start.get("character")?.as_u64()? as u32 + 1,
        end_line: end.get("line")?.as_u64()? as u32 + 1,
        end_column: end.get("character")?.as_u64()? as u32 + 1,
        severity: severity.to_string(),
        message: value.get("message")?.as_str()?.to_string(),
        code: value.get("code").cloned(),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn write_message(writer: &mut ChildStdin, message: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

fn reader_loop(
    stdout: ChildStdout,
    pending: Pending,
    diagnostics: DiagnosticsStore,
    writer: Arc<Mutex<ChildStdin>>,
    ready: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        if length == 0 {
            continue;
        }
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let Ok(message) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        match (id, method) {
            (Some(id), Some(_)) => {
                // A request from the server (progress, registration): answer null.
                let response = json!({"jsonrpc": "2.0", "id": id, "result": Value::Null});
                if let Ok(mut writer) = writer.lock() {
                    let _ = write_message(&mut writer, &response);
                }
            }
            (Some(id), None) => {
                let result = match message.get("error") {
                    Some(error) => Err(format!("language server error: {error}")),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Some(sender) = pending
                    .lock()
                    .ok()
                    .and_then(|mut pending| pending.remove(&id.as_i64().unwrap_or(0)))
                {
                    let _ = sender.send(result);
                }
            }
            (None, Some("experimental/serverStatus")) => {
                if message
                    .get("params")
                    .and_then(|params| params.get("quiescent"))
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    ready.store(true, Ordering::SeqCst);
                }
            }
            (None, Some("$/progress")) => {
                if message
                    .get("params")
                    .and_then(|params| params.get("value"))
                    .and_then(|value| value.get("kind"))
                    .and_then(Value::as_str)
                    == Some("end")
                {
                    ready.store(true, Ordering::SeqCst);
                }
            }
            (None, Some("textDocument/publishDiagnostics")) => {
                if let Some(params) = message.get("params")
                    && let (Some(uri), Some(items)) = (
                        params.get("uri").and_then(Value::as_str),
                        params.get("diagnostics"),
                    )
                    && let Ok(mut diagnostics) = diagnostics.lock()
                {
                    diagnostics.insert(
                        uri.to_string(),
                        items.as_array().cloned().unwrap_or_default(),
                    );
                }
            }
            _ => {}
        }
    }
}

pub fn file_uri(path: &Path) -> Result<String, String> {
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| format!("cannot build a file URI for {}", path.display()))
}

pub fn uri_to_path(uri: &str) -> Result<PathBuf, String> {
    url::Url::parse(uri)
        .map_err(|error| format!("bad URI {uri}: {error}"))?
        .to_file_path()
        .map_err(|_| format!("not a file URI: {uri}"))
}

/// Byte offset -> (0-based line, UTF-16 column) for the LSP.
pub fn position_of(source: &str, byte: usize) -> (u32, u32) {
    let byte = byte.min(source.len());
    let before = &source[..byte];
    let line = before.bytes().filter(|value| *value == b'\n').count() as u32;
    let line_start = before.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let character = source[line_start..byte]
        .chars()
        .map(|character| character.len_utf16() as u32)
        .sum();
    (line, character)
}

/// (0-based line, UTF-16 column) -> byte offset.
pub fn byte_of(source: &str, line: u32, character: u32) -> usize {
    let mut current_line = 0;
    let mut line_start = 0;
    for (index, byte) in source.bytes().enumerate() {
        if current_line == line {
            break;
        }
        if byte == b'\n' {
            current_line += 1;
            line_start = index + 1;
        }
    }
    let mut units = 0;
    for (offset, character_value) in source[line_start..].char_indices() {
        if units >= character || character_value == '\n' {
            return line_start + offset;
        }
        units += character_value.len_utf16() as u32;
    }
    source.len()
}

fn utf16_to_char(source: &str, line: u32, character: u32) -> u32 {
    let text = source.lines().nth(line as usize).unwrap_or("");
    let mut units = 0;
    for (index, value) in text.chars().enumerate() {
        if units >= character {
            return index as u32;
        }
        units += value.len_utf16() as u32;
    }
    text.chars().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_round_trip_with_utf16_and_emoji() {
        let source = "first\n  const value = \"😀\";\n";
        let byte = source.find("value").unwrap();
        let (line, character) = position_of(source, byte);
        assert_eq!(line, 1);
        assert_eq!(byte_of(source, line, character), byte);
    }

    #[test]
    fn file_uris_round_trip() {
        let path = std::path::Path::new("/tmp/astlens test/ä.ts");
        let uri = file_uri(path).unwrap();
        assert!(uri.starts_with("file://"));
        assert_eq!(uri_to_path(&uri).unwrap(), path);
    }
}
