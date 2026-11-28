//! Clean-room read-deny/read-cap policy decisions over shell commands and file reads.

pub mod shell_lexer;

use std::path::Path;

use codemode_core::config::ProjectConfig;
use codemode_core::languages::resolve_language;
use serde::{Deserialize, Serialize};

use shell_lexer::{SimpleCommand, unwrap_commands};

const DEFAULT_READ_CAP_BYTES: usize = 8192;
const MAX_TARGETED_LINES: usize = 100;

/// Policy decision returned by `evaluate_shell_command` and `evaluate_file_read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum CommandDecision {
    Allow,
    Cap { max_bytes: usize, reason: String },
    Deny { reason: String },
}

/// Evaluates a shell command string for bulk-read escapes across wrappers, pipes, and subshells.
pub fn evaluate_shell_command(
    command: &str,
    cwd: &Path,
    config: Option<&ProjectConfig>,
) -> CommandDecision {
    let policy = config.map(|c| &c.policy);
    let deny_bulk = policy.map(|p| p.deny_bulk_reads).unwrap_or(true);
    if !deny_bulk {
        return CommandDecision::Allow;
    }
    let read_cap = policy
        .map(|p| p.read_cap_bytes)
        .unwrap_or(DEFAULT_READ_CAP_BYTES);

    for cmd in unwrap_commands(command) {
        if let CommandDecision::Deny { reason } =
            evaluate_simple_command(&cmd, cwd, config, read_cap)
        {
            return CommandDecision::Deny { reason };
        }
    }
    CommandDecision::Allow
}

/// Evaluates a direct file-read request against the project's `read_cap_bytes` policy.
pub fn evaluate_file_read(
    path: &Path,
    has_bounded_range: bool,
    config: Option<&ProjectConfig>,
) -> CommandDecision {
    let policy = config.map(|c| &c.policy);
    if has_bounded_range || !policy.map(|p| p.deny_bulk_reads).unwrap_or(true) {
        return CommandDecision::Allow;
    }
    let read_cap = policy
        .map(|p| p.read_cap_bytes)
        .unwrap_or(DEFAULT_READ_CAP_BYTES);
    let Ok(meta) = std::fs::metadata(path) else {
        return CommandDecision::Allow;
    };
    if meta.is_file() && meta.len() as usize > read_cap && resolve_language(path, config).is_some()
    {
        return CommandDecision::Deny {
            reason: format!(
                "bulk read of {} ({} bytes > {} byte cap) blocked; use code_outline / code_read or a bounded line range",
                path.display(),
                meta.len(),
                read_cap
            ),
        };
    }
    CommandDecision::Allow
}

fn evaluate_simple_command(
    cmd: &SimpleCommand,
    cwd: &Path,
    config: Option<&ProjectConfig>,
    read_cap: usize,
) -> CommandDecision {
    let prog = cmd.program.to_ascii_lowercase();
    let is_bulk_tool = matches!(
        prog.as_str(),
        "cat" | "less" | "more" | "nl" | "bat" | "batcat" | "head" | "tail" | "sed"
    );
    if !is_bulk_tool {
        return CommandDecision::Allow;
    }

    if is_targeted_slice(&prog, &cmd.args) {
        return CommandDecision::Allow;
    }

    for arg in file_operands(&prog, &cmd.args) {
        let candidate = if Path::new(&arg).is_absolute() {
            Path::new(&arg).to_path_buf()
        } else {
            cwd.join(&arg)
        };
        if let Ok(meta) = std::fs::metadata(&candidate)
            && meta.is_file()
            && meta.len() as usize > read_cap
            && resolve_language(&candidate, config).is_some()
        {
            return CommandDecision::Deny {
                reason: format!(
                    "bulk shell read '{prog}' on {} ({} bytes > {} cap) blocked; use code_outline / code_read instead",
                    candidate.display(),
                    meta.len(),
                    read_cap
                ),
            };
        }
    }

    CommandDecision::Allow
}

fn is_targeted_slice(prog: &str, args: &[String]) -> bool {
    match prog {
        "head" | "tail" => {
            let mut i = 0usize;
            while i < args.len() {
                let a = args[i].as_str();
                if a == "-n" || a == "-c" {
                    if let Some(val) = args.get(i + 1) {
                        if let Some(plus) = val.strip_prefix('+') {
                            let _ = plus;
                            return false;
                        }
                        if let Ok(n) = val.parse::<usize>() {
                            return n <= MAX_TARGETED_LINES;
                        }
                    }
                    i += 2;
                    continue;
                }
                if let Some(rest) = a.strip_prefix("-n")
                    && let Ok(n) = rest.parse::<usize>()
                {
                    return n <= MAX_TARGETED_LINES;
                }
                i += 1;
            }
            false
        }
        "sed" => {
            let has_n = args.iter().any(|a| a == "-n");
            if !has_n {
                return false;
            }
            for a in args {
                if let Some(range) = a.strip_suffix('p')
                    && let Some((s, e)) = range.split_once(',')
                    && let (Ok(start), Ok(end)) =
                        (s.trim().parse::<usize>(), e.trim().parse::<usize>())
                {
                    return end.saturating_sub(start) < MAX_TARGETED_LINES;
                }
            }
            false
        }
        _ => false,
    }
}

fn file_operands(prog: &str, args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut after_dashdash = false;
    while i < args.len() {
        let a = &args[i];
        if after_dashdash {
            out.push(a.clone());
            i += 1;
            continue;
        }
        if a == "--" {
            after_dashdash = true;
            i += 1;
            continue;
        }
        if a.starts_with('-') {
            let takes_value = match prog {
                "head" | "tail" => matches!(a.as_str(), "-n" | "-c"),
                "sed" => matches!(a.as_str(), "-e" | "-f"),
                _ => false,
            };
            if takes_value {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}
