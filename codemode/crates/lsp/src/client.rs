//! Lazy per-(language, workspace_root) LSP multiplexer with disk-truth sync,
//! progress-driven settle, fast clean-file diagnostics, and idle child eviction.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use codemode_core::config::ProjectConfig;
use codemode_core::languages::{
    LanguageId, resolve_language, server_command_for, workspace_root_for,
};
use codemode_core::paths::canonical_or_lexical;
use codemode_ts::{Symbol, line_preview, unified_diff};
use dashmap::DashMap;
use serde::Serialize;
use serde_json::{Value, json};

use crate::LspError;
use crate::document_symbol::document_symbols_outline;

type Pending = Arc<Mutex<HashMap<i64, mpsc::Sender<Result<Value, LspError>>>>>;
type DiagnosticsStore = Arc<Mutex<HashMap<String, Vec<Value>>>>;
type FileState = (u128, u64, i64);

/// Status snapshot for an active language server child.
#[derive(Debug, Clone, Serialize)]
pub struct LspClientStatus {
    pub language: String,
    pub root: String,
    pub alive: bool,
    pub idle_ms: u128,
}

/// Multiplexes lazy per-worktree LSP child processes keyed by `(LanguageId, workspace_root)`.
pub struct LspManager {
    clients: DashMap<(LanguageId, PathBuf), Arc<LspClient>>,
}

impl Default for LspManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LspManager {
    /// Creates an empty multiplexer with no spawned LSP children.
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
        }
    }

    /// Returns the number of currently tracked LSP clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Returns status metadata for all tracked LSP clients.
    pub fn status(&self) -> Vec<LspClientStatus> {
        self.clients
            .iter()
            .map(|entry| {
                let ((lang, root), client) = entry.pair();
                LspClientStatus {
                    language: lang.key().to_string(),
                    root: root.display().to_string(),
                    alive: client.is_alive(),
                    idle_ms: client.idle_duration().as_millis(),
                }
            })
            .collect()
    }

    /// Returns a live LSP client for `path`, spawning one for `(lang, root)` if needed.
    pub fn client_for(
        &self,
        path: &Path,
        config: Option<&ProjectConfig>,
    ) -> Result<Arc<LspClient>, LspError> {
        let lang = resolve_language(path, config)
            .ok_or_else(|| LspError::UnsupportedFile(path.to_path_buf()))?;
        let root = canonical_or_lexical(&workspace_root_for(path, lang));
        let key = (lang, root.clone());
        if let Some(client) = self.clients.get(&key) {
            if client.is_alive() {
                client.touch();
                return Ok(Arc::clone(client.value()));
            }
            drop(client);
            self.clients.remove(&key);
        }
        let command = server_command_for(lang, config).map_err(LspError::ServerUnavailable)?;
        let client = Arc::new(LspClient::spawn(&command, &root, lang)?);
        self.clients.insert(key, Arc::clone(&client));
        Ok(client)
    }

    /// Evicts and terminates clients idle for at least `max_idle` (or no longer alive).
    pub fn evict_idle(&self, max_idle: Duration) -> usize {
        let keys_to_drop: Vec<(LanguageId, PathBuf)> = self
            .clients
            .iter()
            .filter_map(|entry| {
                let client = entry.value();
                if !client.is_alive() || client.idle_duration() >= max_idle {
                    Some(entry.key().clone())
                } else {
                    None
                }
            })
            .collect();
        let mut evicted = 0usize;
        for key in keys_to_drop {
            if self.clients.remove(&key).is_some() {
                evicted += 1;
            }
        }
        evicted
    }
}

/// Terminates and removes clients from `manager` that have been idle for `max_idle`.
pub fn evict_idle_clients(manager: &LspManager, max_idle: Duration) -> usize {
    manager.evict_idle(max_idle)
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

/// Active JSON-RPC connection to a spawned language server child process.
pub struct LspClient {
    child: Mutex<Child>,
    stdin: Arc<Mutex<ChildStdin>>,
    pending: Pending,
    diagnostics: DiagnosticsStore,
    opened: Mutex<HashMap<PathBuf, FileState>>,
    next_id: AtomicI64,
    settled: AtomicBool,
    supports_pull_diagnostics: AtomicBool,
    advertised_pull_diagnostics: AtomicBool,
    progress: Arc<Mutex<ProgressState>>,
    alive: Arc<AtomicBool>,
    last_used: Mutex<Instant>,
    root: PathBuf,
    language: LanguageId,
}

/// Cross-file source location returned by `references`, `definition`, or `implementation`.
#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub preview: String,
}

/// Normalized compiler or linter diagnostic entry.
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
    /// Spawns and initializes a language server child in `root`.
    pub fn spawn(command: &[String], root: &Path, language: LanguageId) -> Result<Self, LspError> {
        let mut child = Command::new(&command[0])
            .args(&command[1..])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| {
                LspError::Protocol(format!("cannot start {}: {error}", command.join(" ")))
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::Protocol("language server has no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::Protocol("language server has no stdout".into()))?;

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
            );
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
            advertised_pull_diagnostics: AtomicBool::new(false),
            progress,
            alive,
            last_used: Mutex::new(Instant::now()),
            root: root.to_path_buf(),
            language,
        };
        client.initialize()?;
        Ok(client)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn touch(&self) {
        if let Ok(mut guard) = self.last_used.lock() {
            *guard = Instant::now();
        }
    }

    pub fn idle_duration(&self) -> Duration {
        self.last_used
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or_default()
    }

    fn initialize(&self) -> Result<(), LspError> {
        let root_uri = file_uri(&self.root)?;
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "codemode", "version": env!("CARGO_PKG_VERSION")},
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
                    "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
                    "definition": {"linkSupport": true},
                    "references": {},
                    "implementation": {},
                    "rename": {},
                    "publishDiagnostics": {"relatedInformation": true},
                    "diagnostic": {}
                }
            }
        });
        let init_result = self.request("initialize", params, Duration::from_secs(60))?;
        let has_pull = init_result
            .get("capabilities")
            .and_then(|c| c.get("diagnosticProvider"))
            .is_some_and(|v| !v.is_null() && v != &Value::Bool(false));
        self.advertised_pull_diagnostics
            .store(has_pull, Ordering::SeqCst);
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    /// Sends a JSON-RPC request and waits up to `timeout` for its reply.
    pub fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, LspError> {
        self.touch();
        if !self.is_alive() {
            return Err(LspError::ServerExited);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| LspError::Protocol("pending map poisoned".into()))?
            .insert(id, sender);
        if !self.is_alive() {
            self.pending
                .lock()
                .map_err(|_| LspError::Protocol("pending map poisoned".into()))?
                .remove(&id);
            return Err(LspError::ServerExited);
        }
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(error) = write_message(
            &mut *self
                .stdin
                .lock()
                .map_err(|_| LspError::Protocol("stdin poisoned".into()))?,
            &message,
        ) {
            self.pending
                .lock()
                .map_err(|_| LspError::Protocol("pending map poisoned".into()))?
                .remove(&id);
            return Err(LspError::Protocol(format!(
                "write to language server failed: {error}"
            )));
        }
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(_) => {
                self.pending
                    .lock()
                    .map_err(|_| LspError::Protocol("pending map poisoned".into()))?
                    .remove(&id);
                Err(LspError::Timeout {
                    method: method.to_string(),
                    timeout,
                })
            }
        }
    }

    /// Sends a JSON-RPC notification.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.touch();
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        write_message(
            &mut *self
                .stdin
                .lock()
                .map_err(|_| LspError::Protocol("stdin poisoned".into()))?,
            &message,
        )
        .map_err(|error| LspError::Protocol(format!("write to language server failed: {error}")))
    }

    /// Sends `didOpen` initially or `didChange` when disk `(mtime, len)` diverges.
    pub fn ensure_open(&self, path: &Path) -> Result<bool, LspError> {
        self.touch();
        let path = canonical_or_lexical(path);
        let source = std::fs::read_to_string(&path).map_err(|error| LspError::Io {
            path: path.clone(),
            source: error,
        })?;
        let meta = std::fs::metadata(&path).map_err(|error| LspError::Io {
            path: path.clone(),
            source: error,
        })?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let signature = (modified, meta.len());
        let uri = file_uri(&path)?;

        let mut opened = self
            .opened
            .lock()
            .map_err(|_| LspError::Protocol("opened map poisoned".into()))?;
        let fresh = match opened.get_mut(&path) {
            Some((m, len, _)) if (*m, *len) == signature => false,
            Some((m, len, version)) => {
                *m = signature.0;
                *len = signature.1;
                *version += 1;
                let version = *version;
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
                let language_id = self.language.lsp_id_for_path(&path);
                opened.insert(path, (signature.0, signature.1, 1));
                if let Ok(mut diagnostics) = self.diagnostics.lock() {
                    diagnostics.remove(&uri);
                }
                self.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": language_id,
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

    fn settle(&self) {
        if self.settled.swap(true, Ordering::SeqCst) {
            return;
        }
        let started = Instant::now();
        let initial_grace = Duration::from_millis(300);
        let max_wait = Duration::from_secs(8);
        let fallback_wait = Duration::from_millis(1200);
        loop {
            let elapsed = started.elapsed();
            let (saw_progress, is_done) = self
                .progress
                .lock()
                .map(|state| (state.saw_progress, state.is_done()))
                .unwrap_or((false, false));
            if is_done && elapsed >= Duration::from_millis(50) {
                return;
            }
            if !saw_progress && elapsed >= fallback_wait {
                return;
            }
            if elapsed >= max_wait {
                return;
            }
            if saw_progress && elapsed < initial_grace {
                std::thread::sleep(Duration::from_millis(25));
                continue;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Queries `textDocument/documentSymbol` and converts to `codemode_ts::Symbol` trees.
    pub fn document_symbols(&self, path: &Path) -> Result<Vec<Symbol>, LspError> {
        self.ensure_open(path)?;
        let canon = canonical_or_lexical(path);
        let source = std::fs::read_to_string(&canon).map_err(|source| LspError::Io {
            path: canon.clone(),
            source,
        })?;
        let uri = file_uri(&canon)?;
        let value = self.request(
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
            Duration::from_secs(30),
        )?;
        Ok(document_symbols_outline(&value, &source))
    }

    /// Queries `references`, `definition`, or `implementation` via LSP.
    pub fn references(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        kind: &str,
        include_declaration: bool,
    ) -> Result<Vec<Location>, LspError> {
        self.ensure_open(path)?;
        self.settle();
        let canon = canonical_or_lexical(path);
        let uri = file_uri(&canon)?;
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

    fn pull_diagnostics_for_uri(&self, uri: &str) -> Option<Vec<Diagnostic>> {
        if !self.supports_pull_diagnostics.load(Ordering::SeqCst) {
            return None;
        }
        match self.request(
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": uri}}),
            Duration::from_secs(10),
        ) {
            Ok(value) => {
                let items = value
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| items.iter().filter_map(diagnostic_from_value).collect())
                    .unwrap_or_default();
                Some(items)
            }
            Err(_) => {
                self.supports_pull_diagnostics
                    .store(false, Ordering::SeqCst);
                None
            }
        }
    }

    /// Returns diagnostics for one file without sleeping 1.5s when clean status is already confirmed.
    pub fn diagnostics(&self, path: &Path) -> Result<Vec<Diagnostic>, LspError> {
        let fresh = self.ensure_open(path)?;
        let canon = canonical_or_lexical(path);
        let uri = file_uri(&canon)?;

        // Fast path 1: server advertised pull diagnostics and no push entry is cached yet.
        if self.advertised_pull_diagnostics.load(Ordering::SeqCst) {
            if let Some(cached) = self.cached_diagnostics(&uri) {
                return Ok(cached);
            }
            if let Some(pulled) = self.pull_diagnostics_for_uri(&uri) {
                return Ok(pulled);
            }
        }

        // Fast path 2: unchanged file already confirmed (including empty []).
        if !fresh && let Some(cached) = self.cached_diagnostics(&uri) {
            return Ok(cached);
        }

        // Wait briefly for push `publishDiagnostics` after didOpen/didChange.
        // As soon as `publishDiagnostics` delivers an array (even empty `[]`), return immediately.
        let deadline = Instant::now() + Duration::from_millis(1200);
        loop {
            if let Some(items) = self.cached_diagnostics(&uri) {
                return Ok(items);
            }
            if Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        Ok(self.pull_diagnostics_for_uri(&uri).unwrap_or_default())
    }

    fn cached_diagnostics(&self, uri: &str) -> Option<Vec<Diagnostic>> {
        self.diagnostics
            .lock()
            .ok()
            .and_then(|map| map.get(uri).cloned())
            .map(|items| items.iter().filter_map(diagnostic_from_value).collect())
    }

    /// Queries diagnostics across multiple files with one shared settle wait.
    pub fn project_diagnostics(
        &self,
        paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<Diagnostic>)>, LspError> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut uris = Vec::with_capacity(paths.len());
        let mut any_fresh = false;
        for path in paths {
            let fresh = self.ensure_open(path)?;
            any_fresh |= fresh;
            uris.push(file_uri(&canonical_or_lexical(path))?);
        }

        if any_fresh && !self.advertised_pull_diagnostics.load(Ordering::SeqCst) {
            let wait = if !self.settled.load(Ordering::SeqCst) {
                self.settle();
                Duration::from_millis(150)
            } else {
                Duration::from_millis(300)
            };
            let deadline = Instant::now() + wait;
            loop {
                let all_published = self
                    .diagnostics
                    .lock()
                    .map(|store| uris.iter().all(|uri| store.contains_key(uri)))
                    .unwrap_or(false);
                if all_published || Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        let mut results = Vec::with_capacity(paths.len());
        for (path, uri) in paths.iter().zip(uris.iter()) {
            let items = if let Some(cached) = self.cached_diagnostics(uri) {
                cached
            } else {
                self.pull_diagnostics_for_uri(uri).unwrap_or_default()
            };
            results.push((path.clone(), items));
        }
        Ok(results)
    }

    /// Applies an LSP rename and returns `(file, unified_diff)` for each modified file.
    pub fn rename(
        &self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Vec<(PathBuf, String)>, LspError> {
        self.ensure_open(path)?;
        self.settle();
        let canon = canonical_or_lexical(path);
        let uri = file_uri(&canon)?;
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
            let text = std::fs::read_to_string(&file).map_err(|source| LspError::Io {
                path: file.clone(),
                source,
            })?;
            let start = byte_of(&text, edit.start_line, edit.start_character);
            let end = byte_of(&text, edit.end_line, edit.end_character);
            by_file
                .entry(file)
                .or_default()
                .push((start, end, edit.new_text));
        }

        let mut changed = Vec::new();
        for (file, mut file_edits) in by_file {
            let old = std::fs::read_to_string(&file).map_err(|source| LspError::Io {
                path: file.clone(),
                source,
            })?;
            file_edits.sort_by_key(|(start, _, _)| *start);
            let mut new = old.clone();
            for (start, end, text) in file_edits.into_iter().rev() {
                new.replace_range(start..end, &text);
            }
            std::fs::write(&file, &new).map_err(|source| LspError::Io {
                path: file.clone(),
                source,
            })?;
            if let Ok(mut opened) = self.opened.lock() {
                opened.remove(&file);
            }
            if let Ok(uri) = file_uri(&file)
                && let Ok(mut diagnostics) = self.diagnostics.lock()
            {
                diagnostics.remove(&uri);
            }
            changed.push((file.clone(), unified_diff(&file, &old, &new)));
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

fn workspace_edits(value: &Value) -> Result<Vec<(String, RawEdit)>, LspError> {
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
        return Err(LspError::Protocol(
            "language server returned no rename edits (is the cursor on a renameable symbol?)"
                .to_string(),
        ));
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
        preview: line_preview(&source, line as usize),
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
                            let _ = sender.send(Err(LspError::ServerExited));
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
                    let _ = sender.send(Err(LspError::ServerExited));
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
                let result = match method {
                    "workspace/configuration" => {
                        let count = message
                            .get("params")
                            .and_then(|params| params.get("items"))
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; count])
                    }
                    "window/workDoneProgress/create" => {
                        let token = message.get("params").and_then(|params| params.get("token"));
                        if let Ok(mut state) = progress.lock() {
                            state.on_create(token);
                        }
                        Value::Null
                    }
                    _ => Value::Null,
                };
                let response = json!({"jsonrpc": "2.0", "id": id, "result": result});
                if let Ok(mut writer) = writer.lock() {
                    let _ = write_message(&mut writer, &response);
                }
            }
            (Some(id), None) => {
                let result = match message.get("error") {
                    Some(error) => Err(LspError::Protocol(format!(
                        "language server error: {error}"
                    ))),
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
                if let Some(params) = message.get("params") {
                    let token = params.get("token");
                    let kind = params
                        .get("value")
                        .and_then(|value| value.get("kind"))
                        .and_then(Value::as_str);
                    if let Ok(mut state) = progress.lock() {
                        state.on_progress(token, kind);
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

/// Converts a filesystem path into a `file://` URI.
pub fn file_uri(path: &Path) -> Result<String, LspError> {
    url::Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| LspError::Protocol(format!("cannot build a file URI for {}", path.display())))
}

/// Converts a `file://` URI into a filesystem path.
pub fn uri_to_path(uri: &str) -> Result<PathBuf, LspError> {
    url::Url::parse(uri)
        .map_err(|error| LspError::Protocol(format!("bad URI {uri}: {error}")))?
        .to_file_path()
        .map_err(|_| LspError::Protocol(format!("not a file URI: {uri}")))
}

/// Converts a byte offset into `(0-based line, UTF-16 character offset)` for LSP requests.
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

/// Converts `(0-based line, UTF-16 character offset)` into a UTF-8 byte offset.
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
