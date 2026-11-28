use std::time::{Duration, Instant};

use codemode_core::config::{PolicyConfig, ProjectConfig};
use codemode_hooks::{HookEvent, run_hook_sync};
use codemode_store::{ContextDb, EventKind, JournalEvent};

#[test]
fn synchronous_hooks_inject_recall_throttle_spool_and_stay_under_10ms() {
    let dir = std::env::temp_dir().join(format!("codemode-hooks-sync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".codemode")).unwrap();
    let db = ContextDb::open(&dir.join("context.db")).unwrap();

    db.record_event(&JournalEvent {
        turn_number: 1,
        kind: EventKind::Prompt,
        summary: "Harden policy lexer".into(),
        detail: None,
        path: None,
        bytes_in: 100,
        bytes_out: 20,
    })
    .unwrap();
    db.record_event(&JournalEvent {
        turn_number: 1,
        kind: EventKind::Decision,
        summary: "Use clean-room recursive unwrapper".into(),
        detail: None,
        path: Some("src/shell_lexer.rs".into()),
        bytes_in: 100,
        bytes_out: 30,
    })
    .unwrap();

    let cfg = ProjectConfig {
        policy: PolicyConfig {
            inline_cap_bytes: 256,
            ..PolicyConfig::default()
        },
        ..ProjectConfig::default()
    };

    let t0 = Instant::now();
    let start_out = run_hook_sync(
        &dir,
        Some(&db),
        Some(&cfg),
        HookEvent::SessionStart {
            turn_number: 2,
            prompt: Some("Continue hardening".into()),
        },
    )
    .unwrap();
    assert!(start_out.allow);
    let ctx = start_out.additional_context.unwrap();
    assert!(ctx.contains("Use clean-room recursive unwrapper"));
    assert!(
        t0.elapsed() < Duration::from_millis(10),
        "SessionStart took {:?}",
        t0.elapsed()
    );

    let compact_out = run_hook_sync(&dir, Some(&db), Some(&cfg), HookEvent::PreCompact).unwrap();
    assert!(
        compact_out
            .additional_context
            .unwrap()
            .contains("Continue hardening")
    );

    let big_output = "noisy compiler warning line\n".repeat(40);
    let post1 = run_hook_sync(
        &dir,
        Some(&db),
        Some(&cfg),
        HookEvent::PostTool {
            tool_name: "Bash".into(),
            output: big_output.clone(),
            file_path: None,
        },
    )
    .unwrap();
    assert!(post1.spilled_source_id.is_some());
    assert!(post1.throttled_output.as_ref().unwrap().len() <= 512);

    let post2 = run_hook_sync(
        &dir,
        Some(&db),
        Some(&cfg),
        HookEvent::PostTool {
            tool_name: "Bash".into(),
            output: big_output,
            file_path: None,
        },
    )
    .unwrap();
    assert!(post2.throttled);

    let src = dir.join("small.rs");
    std::fs::write(&src, "fn f() {}\n").unwrap();
    let t_pre = Instant::now();
    let pre = run_hook_sync(
        &dir,
        Some(&db),
        Some(&cfg),
        HookEvent::PreTool {
            tool_name: "Bash".into(),
            command: Some(format!("cat {}", src.display())),
            file_path: None,
            has_bounded_range: false,
        },
    )
    .unwrap();
    assert!(pre.allow);
    assert!(t_pre.elapsed() < Duration::from_millis(10));
}
