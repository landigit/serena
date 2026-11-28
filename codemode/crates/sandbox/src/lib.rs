//! Clean-room scrubbed sandbox execution with process-group kill, background jobs, and FTS5 spill.

pub mod jobs;
pub mod runtimes;

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codemode_store::{ContextDb, EventKind, JournalEvent, SpillHandle, StoredChunkInput};
use serde::{Deserialize, Serialize};

pub use jobs::{JobId, JobRegistry, JobStatus, job_status};
pub use runtimes::{build_scrubbed_env, detect_runtimes, resolve_runtime_command};

const MAX_CAPTURE_BYTES: usize = 8 * 1024 * 1024;
const DEFAULT_CHUNK_LINES: usize = 100;

/// Errors raised while validating or launching a sandboxed command.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("{0}")]
    InvalidRequest(String),
    #[error("failed to spawn sandboxed process: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(#[from] codemode_store::StoreError),
}

/// Parameters controlling a sandboxed execution.
#[derive(Debug, Clone)]
pub struct ExecRequest {
    pub runtime: String,
    pub code: String,
    pub cwd: PathBuf,
    pub timeout_ms: u64,
    pub inline_cap_bytes: usize,
    pub allow_env: Vec<String>,
    pub extra_env: BTreeMap<String, String>,
    pub background: bool,
    pub label: Option<String>,
}

/// Final outcome of a completed sandbox run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecOutcome {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inline_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spill: Option<SpillHandle>,
    pub total_bytes: usize,
    pub duration_ms: u64,
}

/// Immediate return value from `execute_sandboxed`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExecDispatch {
    Completed(ExecOutcome),
    Background(JobId),
}

/// Executes `req` in a scrubbed, process-group-isolated sandbox (foreground or background).
pub fn execute_sandboxed(
    req: ExecRequest,
    db: Arc<ContextDb>,
    jobs: &JobRegistry,
) -> Result<ExecDispatch, SandboxError> {
    // Validate runtime upfront even for background mode.
    let _ =
        resolve_runtime_command(&req.runtime, &req.code).map_err(SandboxError::InvalidRequest)?;

    if req.background {
        let job_id = jobs.allocate();
        let id_clone = job_id.clone();
        let jobs_ref = jobs.clone();
        std::thread::spawn(move || {
            let outcome = match run_foreground(req, &db) {
                Ok(out) => out,
                Err(err) => ExecOutcome {
                    exit_code: None,
                    timed_out: false,
                    inline_output: Some(format!("sandbox error: {err}")),
                    spill: None,
                    total_bytes: 0,
                    duration_ms: 0,
                },
            };
            jobs_ref.finish(&id_clone, outcome);
        });
        return Ok(ExecDispatch::Background(job_id));
    }

    let outcome = run_foreground(req, &db)?;
    Ok(ExecDispatch::Completed(outcome))
}

fn run_foreground(req: ExecRequest, db: &ContextDb) -> Result<ExecOutcome, SandboxError> {
    let argv =
        resolve_runtime_command(&req.runtime, &req.code).map_err(SandboxError::InvalidRequest)?;
    let env_vars = build_scrubbed_env(&req.allow_env, &req.extra_env);

    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .current_dir(&req.cwd)
        .env_clear()
        .envs(env_vars)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // SAFETY: `setsid` is async-signal-safe and isolates the child + descendants in a fresh PGID.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let started = Instant::now();
    let mut child = cmd.spawn()?;
    let pid = child.id() as i32;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_handle = std::thread::spawn(move || read_capped(stdout));
    let err_handle = std::thread::spawn(move || read_capped(stderr));

    let timeout = Duration::from_millis(req.timeout_ms.max(10));
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            // SAFETY: `pid` is the session/process-group leader created via `setsid`.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stdout_bytes = out_handle.join().unwrap_or_default();
    let stderr_bytes = err_handle.join().unwrap_or_default();
    let stdout_str = String::from_utf8_lossy(&stdout_bytes);
    let stderr_str = String::from_utf8_lossy(&stderr_bytes);

    let mut combined = String::new();
    if !stdout_str.is_empty() {
        combined.push_str(&stdout_str);
    }
    if !stderr_str.is_empty() {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str("[stderr]\n");
        combined.push_str(&stderr_str);
    }
    if timed_out {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push_str(&format!("[timed out after {} ms]", req.timeout_ms));
    }

    let total_bytes = combined.len();
    let exit_code = status.and_then(|s| s.code());
    let duration_ms = started.elapsed().as_millis() as u64;
    let label = req
        .label
        .unwrap_or_else(|| format!("cmd:{}", req.runtime.trim()));

    let (inline_output, spill, bytes_out) = if total_bytes <= req.inline_cap_bytes {
        let out_len = total_bytes as u64;
        (Some(combined), None, out_len)
    } else {
        let chunks = chunk_lines(&combined);
        let handle = db.store_spilled(&label, "exec", None, &combined, chunks)?;
        let out_len = handle.preview.len() as u64;
        (None, Some(handle), out_len)
    };

    let _ = db.record_event(&JournalEvent {
        turn_number: 0,
        kind: if timed_out || exit_code.is_some_and(|c| c != 0) {
            EventKind::ToolErr
        } else {
            EventKind::Cmd
        },
        summary: format!("{label} (exit={exit_code:?}, timed_out={timed_out})"),
        detail: None,
        path: None,
        bytes_in: total_bytes as u64,
        bytes_out,
    });

    Ok(ExecOutcome {
        exit_code,
        timed_out,
        inline_output,
        spill,
        total_bytes,
        duration_ms,
    })
}

fn read_capped(stream: Option<impl Read>) -> Vec<u8> {
    let Some(mut reader) = stream else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let remaining = MAX_CAPTURE_BYTES.saturating_sub(out.len());
                if remaining > 0 {
                    let take = n.min(remaining);
                    out.extend_from_slice(&buf[..take]);
                }
            }
        }
    }
    out
}

fn chunk_lines(text: &str) -> Vec<StoredChunkInput> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    lines
        .chunks(DEFAULT_CHUNK_LINES)
        .enumerate()
        .map(|(idx, group)| {
            let start_line = idx * DEFAULT_CHUNK_LINES + 1;
            let end_line = start_line + group.len() - 1;
            StoredChunkInput {
                start_line,
                end_line,
                symbol_path: None,
                kind: Some("exec".to_string()),
                content: group.join("\n"),
            }
        })
        .collect()
}
