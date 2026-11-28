use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codemode_sandbox::{
    ExecDispatch, ExecRequest, JobRegistry, JobStatus, execute_sandboxed, job_status,
};
use codemode_store::ContextDb;

#[test]
fn strips_sensitive_env_vars_unless_allowlisted() {
    let base = std::env::temp_dir().join(format!("codemode-sandbox-env-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let db = Arc::new(ContextDb::open(&base.join("context.db")).unwrap());
    let jobs = JobRegistry::new();

    // Inject test secrets in extra_env? No, verify process env scrubbing by setting env vars on current process?
    // Safe way without mutating global env concurrently: pass via std::env in a single-key test OR inspect scrub_env + live subprocess.
    // Let's test live subprocess with both a parent-inherited var and allowlist!
    unsafe {
        std::env::set_var("GITHUB_TOKEN", "ghp_secret_123");
        std::env::set_var("OPENAI_API_KEY", "sk_secret_456");
        std::env::set_var("AWS_SECRET_ACCESS_KEY", "aws_secret_789");
        std::env::set_var("CODEMODE_ALLOWED_TOKEN", "ok_allowed");
    }

    let outcome = match execute_sandboxed(
        ExecRequest {
            runtime: "sh".into(),
            code: "printf 'GH=%s|OAI=%s|AWS=%s|ALLOW=%s' \"$GITHUB_TOKEN\" \"$OPENAI_API_KEY\" \"$AWS_SECRET_ACCESS_KEY\" \"$CODEMODE_ALLOWED_TOKEN\"".into(),
            cwd: base.clone(),
            timeout_ms: 5000,
            inline_cap_bytes: 4096,
            allow_env: vec!["CODEMODE_ALLOWED_TOKEN".into()],
            extra_env: BTreeMap::new(),
            background: false,
            label: Some("env_check".into()),
        },
        Arc::clone(&db),
        &jobs,
    )
    .unwrap()
    {
        ExecDispatch::Completed(o) => o,
        ExecDispatch::Background(_) => panic!("expected foreground"),
    };

    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(
        outcome.inline_output.as_deref(),
        Some("GH=|OAI=|AWS=|ALLOW=ok_allowed")
    );
}

#[test]
fn kills_entire_process_group_on_timeout_without_orphans() {
    let base = std::env::temp_dir().join(format!("codemode-sandbox-kill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let db = Arc::new(ContextDb::open(&base.join("context.db")).unwrap());
    let jobs = JobRegistry::new();
    let pid_file = base.join("child.pid");

    let t0 = Instant::now();
    let outcome = match execute_sandboxed(
        ExecRequest {
            runtime: "sh".into(),
            code: format!("sleep 30 & echo $! > {}; wait", pid_file.display()),
            cwd: base.clone(),
            timeout_ms: 150,
            inline_cap_bytes: 4096,
            allow_env: Vec::new(),
            extra_env: BTreeMap::new(),
            background: false,
            label: None,
        },
        db,
        &jobs,
    )
    .unwrap()
    {
        ExecDispatch::Completed(o) => o,
        ExecDispatch::Background(_) => panic!("expected foreground"),
    };

    assert!(outcome.timed_out);
    assert!(t0.elapsed() < Duration::from_secs(2));
    std::thread::sleep(Duration::from_millis(50));
    if let Ok(pid_str) = std::fs::read_to_string(&pid_file)
        && let Ok(pid) = pid_str.trim().parse::<i32>()
    {
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        let is_zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.contains(") Z "))
            .unwrap_or(false);
        assert!(
            !alive || is_zombie,
            "grandchild sleep pid {pid} survived process-group kill"
        );
    }
}

#[test]
fn background_jobs_poll_and_oversized_output_spills_to_context_db() {
    let base = std::env::temp_dir().join(format!("codemode-sandbox-bg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let db = Arc::new(ContextDb::open(&base.join("context.db")).unwrap());
    let jobs = JobRegistry::new();

    // 1. Oversized foreground output spills to ContextDb.
    let big_outcome = match execute_sandboxed(
        ExecRequest {
            runtime: "python3".into(),
            code: "for i in range(300): print(f'row {i}: sandbox spill payload marker_xyz')".into(),
            cwd: base.clone(),
            timeout_ms: 5000,
            inline_cap_bytes: 512,
            allow_env: Vec::new(),
            extra_env: BTreeMap::new(),
            background: false,
            label: Some("cmd:py_big".into()),
        },
        Arc::clone(&db),
        &jobs,
    )
    .unwrap()
    {
        ExecDispatch::Completed(o) => o,
        ExecDispatch::Background(_) => panic!("expected foreground"),
    };
    assert_eq!(big_outcome.exit_code, Some(0));
    assert!(big_outcome.inline_output.is_none());
    let spill = big_outcome.spill.expect("expected spill handle");
    assert!(spill.total_bytes > 512);
    assert!(spill.preview.len() <= 2048);
    let hits = db
        .search_fts("marker_xyz", Some(&spill.source_id), None, 5)
        .unwrap();
    assert!(!hits.is_empty());

    // 2. Background mode returns JobId immediately and indexes upon completion.
    let job_id = match execute_sandboxed(
        ExecRequest {
            runtime: "sh".into(),
            code: "sleep 0.05; echo background_done_token".into(),
            cwd: base.clone(),
            timeout_ms: 5000,
            inline_cap_bytes: 20,
            allow_env: Vec::new(),
            extra_env: BTreeMap::new(),
            background: true,
            label: Some("cmd:bg_job".into()),
        },
        Arc::clone(&db),
        &jobs,
    )
    .unwrap()
    {
        ExecDispatch::Background(id) => id,
        ExecDispatch::Completed(_) => panic!("expected background JobId"),
    };

    let deadline = Instant::now() + Duration::from_secs(5);
    let final_outcome = loop {
        match job_status(&jobs, &job_id) {
            JobStatus::Finished(outcome) => break outcome,
            JobStatus::Running => {
                assert!(Instant::now() < deadline, "background job timed out");
                std::thread::sleep(Duration::from_millis(20));
            }
            JobStatus::NotFound => panic!("missing job_id"),
        }
    };
    assert_eq!(final_outcome.exit_code, Some(0));
    let bg_spill = final_outcome.spill.expect("expected background spill");
    let bg_hits = db
        .search_fts("background_done_token", Some(&bg_spill.source_id), None, 5)
        .unwrap();
    assert!(!bg_hits.is_empty());
}
