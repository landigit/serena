//! Synchronous (<10 ms) hook handlers for PreTool, PostTool, SessionStart, and PreCompact.

pub mod spool;

use std::path::{Path, PathBuf};

use codemode_core::config::ProjectConfig;
use codemode_policy::{CommandDecision, evaluate_file_read, evaluate_shell_command};
use codemode_retrieval::build_resume_summary;
use codemode_store::{ContextDb, EventKind, JournalEvent};
use serde::{Deserialize, Serialize};

use spool::spool_post_tool;

/// Errors raised during synchronous hook evaluation.
#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("store error: {0}")]
    Store(#[from] codemode_store::StoreError),
    #[error("retrieval error: {0}")]
    Retrieval(#[from] codemode_retrieval::RetrievalError),
}

/// Supported host lifecycle hook events.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum HookEvent {
    PreTool {
        tool_name: String,
        command: Option<String>,
        file_path: Option<PathBuf>,
        has_bounded_range: bool,
    },
    PostTool {
        tool_name: String,
        output: String,
        file_path: Option<PathBuf>,
    },
    SessionStart {
        turn_number: u64,
        prompt: Option<String>,
    },
    PreCompact,
}

/// Structured hook response returned to host plugins (`<10 ms`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookOutput {
    pub allow: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spilled_source_id: Option<String>,
    pub throttled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throttled_output: Option<String>,
}

/// Executes a hook event synchronously without initializing Tokio or spawning LSPs.
pub fn run_hook_sync(
    project_root: &Path,
    db: Option<&ContextDb>,
    config: Option<&ProjectConfig>,
    event: HookEvent,
) -> Result<HookOutput, HookError> {
    match event {
        HookEvent::PreTool {
            tool_name: _,
            command,
            file_path,
            has_bounded_range,
        } => {
            if let Some(cmd) = command {
                match evaluate_shell_command(&cmd, project_root, config) {
                    CommandDecision::Allow => {}
                    CommandDecision::Cap { max_bytes, reason } => {
                        return Ok(HookOutput {
                            allow: true,
                            reason: Some(format!("{reason} (cap={max_bytes})")),
                            additional_context: None,
                            spilled_source_id: None,
                            throttled: false,
                            throttled_output: None,
                        });
                    }
                    CommandDecision::Deny { reason } => {
                        return Ok(HookOutput {
                            allow: false,
                            reason: Some(reason),
                            additional_context: None,
                            spilled_source_id: None,
                            throttled: false,
                            throttled_output: None,
                        });
                    }
                }
            }
            if let Some(path) = file_path {
                let resolved = if path.is_absolute() {
                    path
                } else {
                    project_root.join(path)
                };
                if let CommandDecision::Deny { reason } =
                    evaluate_file_read(&resolved, has_bounded_range, config)
                {
                    return Ok(HookOutput {
                        allow: false,
                        reason: Some(reason),
                        additional_context: None,
                        spilled_source_id: None,
                        throttled: false,
                        throttled_output: None,
                    });
                }
            }
            Ok(HookOutput {
                allow: true,
                reason: None,
                additional_context: None,
                spilled_source_id: None,
                throttled: false,
                throttled_output: None,
            })
        }
        HookEvent::PostTool {
            tool_name,
            output,
            file_path: _,
        } => {
            let cap = config.map(|c| c.policy.inline_cap_bytes).unwrap_or(6144);
            let spooled = spool_post_tool(&tool_name, &output, cap, db)?;
            Ok(HookOutput {
                allow: true,
                reason: None,
                additional_context: None,
                spilled_source_id: spooled.spilled_source_id,
                throttled: spooled.throttled,
                throttled_output: Some(spooled.output),
            })
        }
        HookEvent::SessionStart {
            turn_number,
            prompt,
        } => {
            if let (Some(db), Some(text)) = (db, prompt.as_deref())
                && !text.trim().is_empty()
            {
                let _ = db.record_event(&JournalEvent {
                    turn_number,
                    kind: EventKind::Prompt,
                    summary: text.trim().to_string(),
                    detail: None,
                    path: None,
                    bytes_in: text.len() as u64,
                    bytes_out: 0,
                });
            }
            let recall = render_recall(db)?;
            Ok(HookOutput {
                allow: true,
                reason: None,
                additional_context: recall,
                spilled_source_id: None,
                throttled: false,
                throttled_output: None,
            })
        }
        HookEvent::PreCompact => {
            let recall = render_recall(db)?;
            Ok(HookOutput {
                allow: true,
                reason: None,
                additional_context: recall,
                spilled_source_id: None,
                throttled: false,
                throttled_output: None,
            })
        }
    }
}

fn render_recall(db: Option<&ContextDb>) -> Result<Option<String>, HookError> {
    let Some(db) = db else {
        return Ok(None);
    };
    let summary = build_resume_summary(db)?;
    if summary.last_task.is_none()
        && summary.changed_files.is_empty()
        && summary.open_errors.is_empty()
        && summary.decisions.is_empty()
    {
        return Ok(None);
    }
    Ok(Some(
        serde_json::to_string(&summary).unwrap_or_else(|_| "{}".to_string()),
    ))
}
