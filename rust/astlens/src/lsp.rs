//! Lazy, per-worktree LSP multiplexer. Clients are keyed by (language, workspace
//! root); every query re-syncs the file from disk first, so edits made by the
//! host's native tools never leave a stale buffer behind.

use std::collections::{HashMap, HashSet};
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
            if client.is_alive() {
                return Ok(Arc::clone(client.value()));
            }
            drop(client);
            self.clients.remove(&key);
        }
        let command = roots::server_command(lang)?;
        let client = Arc::new(LspClient::spawn(&command, &root, lang)?);
        self.clients.insert(key, Arc::clone(&client));
        Ok(client)
    }
}

#[derive(Default)]
struct ProgressState {
    active_tokens: HashSet<String>,
    saw_progress: bool,
    server_busy: bool,
    ready: bool,
}

impl ProgressState {
    fn on_create(&mut self, token: Option<&Value>) {
        self.saw_progress = true;
        if let Some(key) = token_key(token) {
            self.active_tokens.insert(key);
        }
        self.ready = false;
    }

    fn on_progress(&mut self, token: Option<&Value>, kind: Option<&str>) {
        self.saw_progress = true;
        match kind {
            Some("begin") => {
                if let Some(key) = token_key(token) {
                    self.active_tokens.insert(key);
                }
                self.ready = false;
            }
            Some("end") => {
                if let Some(key) = token_key(token) {
                    self.active_tokens.remove(&key);
                }
                if self.active_tokens.is_empty() && !self.server_busy {
                    self.ready = true;
                }
            }
            _ => {}
        }
    }

    fn on_ready_signal(&mut self, is_ready: bool) {
        self.saw_progress = true;
        self.server_busy = !is_ready;
        self.ready = is_ready && self.active_tokens.is_empty();
    }

    fn is_done(&self) -> bool {
        self.ready && !self.server_busy && self.active_tokens.is_empty()
    }
}

fn token_key(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
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
    supports_pull_diagnostics: AtomicBool,
    progress: Arc<Mutex<ProgressState>>,
    alive: Arc<AtomicBool>,
    root: PathBuf,
    language: Lang,
}

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize)]
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
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        let alive = Arc::new(AtomicBool::new(true));

        let reader_pending = Arc::clone(&pending);
        let reader_diagnostics = Arc::clone(&diagnostics);
        let reader_writer = Arc::clone(&writer);
        let reader_progress = Arc::clone(&progress);
        let reader_alive = Arc::clone(&alive);
        std::thread::spawn(move || {
            reader_loop(
                stdout,
                reader_pending,
                reader_diagnostics,
                reader_writer,
                reader_progress,
                reader_alive,
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
            supports_pull_diagnostics: AtomicBool::new(true),
            progress,
            alive,
            root: root.to_path_buf(),
            language,
        };
        client.initialize()?;
        Ok(client)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
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
                "window": {"workDoneProgress": true},
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
        if !self.is_alive() {
            return Err("language server exited".to_string());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "pending map poisoned")?
            .insert(id, sender);
        if !self.is_alive() {
            self.pending
                .lock()
                .map_err(|_| "pending map poisoned")?
                .remove(&id);
            return Err("language server exited".to_string());
        }
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
                // Drop diagnostics from the previous file version before sending didChange.
                if let Ok(mut diagnostics) = self.diagnostics.lock() {
                    diagnostics.remove(&uri);
                }
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
                if let Ok(mut diagnostics) = self.diagnostics.lock() {
                    diagnostics.remove(&uri);
                }
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

    /// Wait for initial workspace indexing using server progress/status notifications
    /// when reported, falling back to a bounded wait for servers without progress.
    fn settle(&self) {
        if self.settled.swap(true, Ordering::SeqCst) {
            return;
        }
        let started = Instant::now();
        let max_wait = Duration::from_secs(8);
        let fallback_wait = if matches!(self.language, Lang::Rust | Lang::Lean) {
            Duration::from_secs(8)
        } else {
            Duration::from_millis(1500)
        };
        while self.is_alive() {
            let (done, saw_progress) = self
                .progress
                .lock()
                .map(|state| (state.is_done(), state.saw_progress))
                .unwrap_or((true, false));
            if done {
                return;
            }
            let limit = if saw_progress {
                max_wait
            } else {
                fallback_wait
            };
            if started.elapsed() >= limit {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
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

    fn pull_diagnostics(&self, uri: &str) -> Vec<Diagnostic> {
        if !self.supports_pull_diagnostics.load(Ordering::SeqCst) {
            return Vec::new();
        }
        match self.request(
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": uri}}),
            Duration::from_secs(10),
        ) {
            Ok(value) => value
                .get("items")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(diagnostic_from_value).collect())
                .unwrap_or_default(),
            Err(error) => {
                let lower = error.to_ascii_lowercase();
                if lower.contains("-32601")
                    || lower.contains("methodnotfound")
                    || lower.contains("method not found")
                    || lower.contains("unhandled method")
                    || lower.contains("not supported")
                {
                    self.supports_pull_diagnostics
                        .store(false, Ordering::SeqCst);
                }
                Vec::new()
            }
        }
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

        Ok(self.pull_diagnostics(&uri))
    }

    /// Collect diagnostics across a batch of workspace files using one shared wait.
    pub fn project_diagnostics(
        &self,
        files: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<Diagnostic>)>, String> {
        let mut opened_uris = Vec::with_capacity(files.len());
        for path in files {
            let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            match self.ensure_open(&canon) {
                Ok(_) => {}
                Err(error)
                    if error.starts_with("cannot read ") || error.starts_with("cannot stat ") =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            }
            let uri = file_uri(&canon)?;
            opened_uris.push((canon, uri));
        }
        if opened_uris.is_empty() {
            return Ok(Vec::new());
        }
        self.settle();

        let deadline = Instant::now() + Duration::from_millis(2000);
        while self.is_alive() && Instant::now() < deadline {
            let done = {
                let map = self
                    .diagnostics
                    .lock()
                    .map_err(|_| "diagnostics map poisoned")?;
                opened_uris.iter().all(|(_, uri)| map.contains_key(uri))
            };
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let snapshot = self
            .diagnostics
            .lock()
            .map_err(|_| "diagnostics map poisoned")?
            .clone();
        let mut out = Vec::with_capacity(opened_uris.len());
        for (path, uri) in opened_uris {
            let items = match snapshot.get(&uri) {
                Some(raw) => raw.iter().filter_map(diagnostic_from_value).collect(),
                None => self.pull_diagnostics(&uri),
            };
            out.push((path, items));
        }
        Ok(out)
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
            // Force a fresh didChange on the next query even if rename preserved length within the same mtime tick.
            if let Ok(mut opened) = self.opened.lock() {
                opened.remove(&file);
            }
            if let Ok(uri) = file_uri(&file)
                && let Ok(mut diagnostics) = self.diagnostics.lock()
            {
                diagnostics.remove(&uri);
            }
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
    progress: Arc<Mutex<ProgressState>>,
    alive: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut length = 0usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => {
                    alive.store(false, Ordering::SeqCst);
                    if let Ok(mut pending) = pending.lock() {
                        for (_, sender) in pending.drain() {
                            let _ = sender.send(Err("language server exited".to_string()));
                        }
                    }
                    return;
                }
                Ok(_) => {}
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        if length == 0 {
            continue;
        }
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).is_err() {
            alive.store(false, Ordering::SeqCst);
            if let Ok(mut pending) = pending.lock() {
                for (_, sender) in pending.drain() {
                    let _ = sender.send(Err("language server exited".to_string()));
                }
            }
            return;
        }
        let Ok(message) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str);
        match (id, method) {
            (Some(id), Some(method)) => {
                if method == "window/workDoneProgress/create"
                    && let Ok(mut state) = progress.lock()
                {
                    let token = message.get("params").and_then(|params| params.get("token"));
                    state.on_create(token);
                }
                let result = if method == "workspace/configuration" {
                    let count = message
                        .get("params")
                        .and_then(|params| params.get("items"))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; count])
                } else {
                    Value::Null
                };
                let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
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
                if let Some(quiescent) = message
                    .get("params")
                    .and_then(|params| params.get("quiescent"))
                    .and_then(Value::as_bool)
                    && let Ok(mut state) = progress.lock()
                {
                    state.on_ready_signal(quiescent);
                }
            }
            (None, Some("$/lean/fileProgress")) => {
                if let Some(processing) = message
                    .get("params")
                    .and_then(|params| params.get("processing"))
                    .and_then(Value::as_array)
                    && let Ok(mut state) = progress.lock()
                {
                    state.on_ready_signal(processing.is_empty());
                }
            }
            (None, Some("$/progress")) => {
                if let Some(params) = message.get("params")
                    && let Ok(mut state) = progress.lock()
                {
                    let token = params.get("token");
                    let kind = params
                        .get("value")
                        .and_then(|value| value.get("kind"))
                        .and_then(Value::as_str);
                    state.on_progress(token, kind);
                }
            }
            (None, Some("language/status")) => {
                if let Some(kind) = message
                    .get("params")
                    .and_then(|params| params.get("type"))
                    .and_then(Value::as_str)
                    && let Ok(mut state) = progress.lock()
                {
                    match kind {
                        "ServiceReady" | "Started" => state.on_ready_signal(true),
                        "Starting" => state.on_ready_signal(false),
                        _ => {}
                    }
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

    fn spawn_mock_lsp(name: &str, script: &str, root: &Path) -> LspClient {
        let script_path = root.join(name);
        std::fs::write(&script_path, script).unwrap();
        let cmd = vec!["python3".to_string(), script_path.display().to_string()];
        LspClient::spawn(&cmd, root, Lang::Python).unwrap()
    }

    #[test]
    fn diagnostics_drops_stale_cache_after_did_change() {
        let root = std::env::temp_dir().join(format!("astlens-lsp-diag-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("main.py");
        std::fs::write(&file, "x = bad()\n").unwrap();
        let script = r#"
import json, sys, time
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
        send_msg({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
            "uri": uri,
            "diagnostics": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "severity": 1, "message": "old error"}]
        }})
    elif method == "textDocument/didChange":
        uri = m["params"]["textDocument"]["uri"]
        time.sleep(0.15)
        send_msg({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": []}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let client = spawn_mock_lsp("mock_diag.py", script, &root);
        let first = client.diagnostics(&file).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].message, "old error");

        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&file, "x = 1\n").unwrap();
        let second = client.diagnostics(&file).unwrap();
        assert!(second.is_empty(), "got stale diagnostics: {second:?}");
    }

    #[test]
    fn reader_handles_lowercase_content_length_and_workspace_configuration() {
        let root = std::env::temp_dir().join(format!("astlens-lsp-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
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
    sys.stdout.buffer.write(f"content-length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": 99, "method": "workspace/configuration", "params": {"items": [{"section": "a"}, {"section": "b"}]}})
        resp = read_msg()
        if resp.get("result") != [None, None]:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32602, "message": f"bad config reply: {resp}"}})
        else:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let _client = spawn_mock_lsp("mock_cfg.py", script, &root);
    }

    #[test]
    fn dead_lsp_fails_pending_requests_immediately() {
        let root = std::env::temp_dir().join(format!("astlens-lsp-eof-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
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
    elif m.get("method") == "textDocument/references":
        sys.exit(1)
"#;
        let client = spawn_mock_lsp("mock_eof.py", script, &root);
        let file = root.join("main.py");
        std::fs::write(&file, "x = 1\n").unwrap();
        let started = Instant::now();
        let err = client
            .request(
                "textDocument/references",
                json!({}),
                Duration::from_secs(10),
            )
            .unwrap_err();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "hung for {:?}",
            started.elapsed()
        );
        assert!(err.contains("exited") || err.contains("closed"), "{err}");
    }

    #[test]
    fn settle_waits_for_all_progress_tokens_then_returns_promptly() {
        let root = std::env::temp_dir().join(format!("astlens-lsp-prog-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("main.py");
        std::fs::write(&file, "x = 1\n").unwrap();
        let script = r#"
import json, sys, threading, time
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
        done_event = threading.Event()
        caps = m["params"]["capabilities"]
        ok = caps.get("window", {}).get("workDoneProgress") is True
        if not ok:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32602, "message": "missing workDoneProgress"}})
        else:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "initialized":
        send_msg({"jsonrpc": "2.0", "id": 101, "method": "window/workDoneProgress/create", "params": {"token": "t1"}})
        send_msg({"jsonrpc": "2.0", "id": 102, "method": "window/workDoneProgress/create", "params": {"token": "t2"}})
    elif method == "textDocument/didOpen":
        def finish():
            time.sleep(0.1)
            send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t1", "value": {"kind": "end"}}})
            time.sleep(0.25)
            done_event.set()
            send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t2", "value": {"kind": "end"}}})
        threading.Thread(target=finish, daemon=True).start()
    elif method == "textDocument/references":
        if not done_event.is_set():
            send_msg({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32000, "message": "queried before t2 finished"}})
        else:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "result": []})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let client = spawn_mock_lsp("mock_prog.py", script, &root);
        let started = Instant::now();
        let locs = client.references(&file, 0, 0, "references", true).unwrap();
        let elapsed = started.elapsed();
        assert!(locs.is_empty());
        assert!(
            elapsed >= Duration::from_millis(300) && elapsed < Duration::from_millis(1200),
            "unexpected settle duration: {elapsed:?}"
        );
    }

    #[test]
    fn project_diagnostics_collects_batch_with_shared_wait() {
        let root = std::env::temp_dir().join(format!("astlens-lsp-proj-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a.py");
        let b = root.join("b.py");
        std::fs::write(&a, "x = bad()\n").unwrap();
        std::fs::write(&b, "y = 1\n").unwrap();
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
    elif method == "initialized":
        send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "init", "value": {"kind": "end"}}})
    elif method == "textDocument/didOpen":
        uri = m["params"]["textDocument"]["uri"]
        diags = []
        if uri.endswith("a.py"):
            diags = [{"range": {"start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 7}}, "severity": 1, "message": "bad ref"}]
        send_msg({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": diags}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let client = spawn_mock_lsp("mock_proj.py", script, &root);
        let started = Instant::now();
        let results = client.project_diagnostics(&[a.clone(), b.clone()]).unwrap();
        assert!(started.elapsed() < Duration::from_millis(1200));
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1.len(), 1);
        assert_eq!(results[0].1[0].message, "bad ref");
        assert!(results[1].1.is_empty());
    }

    #[test]
    fn settle_waits_for_server_status_even_after_progress_token_ends() {
        let root =
            std::env::temp_dir().join(format!("astlens-lsp-quiescent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("main.py");
        std::fs::write(&file, "x = 1\n").unwrap();
        let script = r#"
import json, sys, threading, time
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
quiescent_event = threading.Event()
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "initialized":
        send_msg({"jsonrpc": "2.0", "method": "experimental/serverStatus", "params": {"quiescent": False}})
        send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t1", "value": {"kind": "begin"}}})
    elif method == "textDocument/didOpen":
        def finish():
            time.sleep(0.08)
            send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "t1", "value": {"kind": "end"}}})
            time.sleep(0.25)
            quiescent_event.set()
            send_msg({"jsonrpc": "2.0", "method": "experimental/serverStatus", "params": {"quiescent": True}})
        threading.Thread(target=finish, daemon=True).start()
    elif method == "textDocument/references":
        if not quiescent_event.is_set():
            send_msg({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32000, "message": "queried before quiescent"}})
        else:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "result": []})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let client = spawn_mock_lsp("mock_quiescent.py", script, &root);
        let started = Instant::now();
        let locs = client.references(&file, 0, 0, "references", true).unwrap();
        assert!(locs.is_empty());
        assert!(
            started.elapsed() >= Duration::from_millis(300),
            "returned too early: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn project_diagnostics_falls_back_to_pull_and_caches_unsupported() {
        let root = std::env::temp_dir().join(format!("astlens-lsp-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a.py");
        let b = root.join("b.py");
        std::fs::write(&a, "a = 1\n").unwrap();
        std::fs::write(&b, "b = 2\n").unwrap();

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
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "initialized":
        send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "init", "value": {"kind": "end"}}})
    elif method == "textDocument/didOpen":
        uri = m["params"]["textDocument"]["uri"]
        if uri.endswith("a.py"):
            send_msg({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": uri, "diagnostics": []}})
    elif method == "textDocument/diagnostic":
        uri = m["params"]["textDocument"]["uri"]
        items = [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "severity": 2, "message": "pull warn"}] if uri.endswith("b.py") else []
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"kind": "full", "items": items}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let client = spawn_mock_lsp("mock_pull.py", pull_script, &root);
        let results = client.project_diagnostics(&[a.clone(), b.clone()]).unwrap();
        assert!(results[0].1.is_empty());
        assert_eq!(results[1].1.len(), 1);
        assert_eq!(results[1].1[0].message, "pull warn");
        drop(client);

        let unsupported_script = r#"
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
pull_calls = 0
while True:
    m = read_msg()
    if m is None: break
    method = m.get("method")
    if method == "initialize":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {}}})
    elif method == "initialized":
        send_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "init", "value": {"kind": "end"}}})
    elif method == "textDocument/diagnostic":
        pull_calls += 1
        if pull_calls > 1:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"items": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "severity": 1, "message": "should not pull twice"}]}})
        else:
            send_msg({"jsonrpc": "2.0", "id": m["id"], "error": {"code": -32000, "message": "method not found"}})
    elif method == "shutdown":
        send_msg({"jsonrpc": "2.0", "id": m["id"], "result": None})
        break
"#;
        let client2 = spawn_mock_lsp("mock_no_pull.py", unsupported_script, &root);
        let res2 = client2.project_diagnostics(&[a, b]).unwrap();
        assert!(res2[0].1.is_empty());
        assert!(res2[1].1.is_empty());
    }
}
