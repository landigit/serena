//! `codemode` single binary: fast argv[1] dispatch for serve, hook, index, doctor, stats, purge, upgrade.

use std::io::Read;
use std::path::{Path, PathBuf};

use codemode_core::config::{find_project_root, load_project_config};
use codemode_core::paths::canonical_or_lexical;
use codemode_hooks::{HookEvent, run_hook_sync};
use codemode_retrieval::index_path_gitignore;
use codemode_store::ContextDb;
use serde_json::{Value, json};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None | Some("serve") => {
            if let Err(err) = codemode_mcp::serve_stdio() {
                eprintln!("codemode: {err}");
                1
            } else {
                0
            }
        }
        Some("hook") => run_hook_cli(&args[1..]),
        Some("index") => run_index_cli(&args[1..]),
        Some("doctor") => run_doctor_cli(&args[1..]),
        Some("stats") => run_stats_cli(&args[1..]),
        Some("purge") => run_purge_cli(&args[1..]),
        Some("upgrade") => run_upgrade_cli(),
        Some("--help" | "-h" | "help") => {
            print_help();
            0
        }
        Some(other) => {
            eprintln!("codemode: unknown subcommand '{other}'");
            print_help();
            2
        }
    };
    if code != 0 {
        std::process::exit(code);
    }
}

fn print_help() {
    eprintln!(
        "codemode {} — structural code intelligence + context discipline\n\n\
         USAGE:\n  \
           codemode serve\n  \
           codemode hook <pre-tool|post-tool|session-start|pre-compact> [--root DIR]\n  \
           codemode index [PATH]\n  \
           codemode doctor [--root DIR]\n  \
           codemode stats [--root DIR]\n  \
           codemode purge [--root DIR]\n  \
           codemode upgrade",
        env!("CARGO_PKG_VERSION")
    );
}

fn extract_root_flag(args: &[String]) -> PathBuf {
    for (idx, a) in args.iter().enumerate() {
        if a == "--root"
            && let Some(val) = args.get(idx + 1)
        {
            return canonical_or_lexical(Path::new(val));
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    canonical_or_lexical(&find_project_root(&cwd))
}

fn run_hook_cli(args: &[String]) -> i32 {
    let Some(kind) = args.first().map(String::as_str) else {
        eprintln!("codemode hook: missing event name");
        return 2;
    };
    let root = extract_root_flag(&args[1..]);
    let cfg = load_project_config(&root).ok();
    let db = ContextDb::open_for_project(&root).ok();

    let mut raw_stdin = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw_stdin);
    let payload: Value = if raw_stdin.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&raw_stdin).unwrap_or_else(|_| json!({}))
    };

    let event = match kind {
        "pre-tool" | "pre_tool" => HookEvent::PreTool {
            tool_name: payload
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("Bash")
                .to_string(),
            command: payload
                .get("command")
                .or_else(|| payload.get("tool_input").and_then(|v| v.get("command")))
                .and_then(Value::as_str)
                .map(str::to_string),
            file_path: payload
                .get("file_path")
                .or_else(|| payload.get("tool_input").and_then(|v| v.get("file_path")))
                .and_then(Value::as_str)
                .map(PathBuf::from),
            has_bounded_range: payload
                .get("has_bounded_range")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        "post-tool" | "post_tool" => HookEvent::PostTool {
            tool_name: payload
                .get("tool_name")
                .and_then(Value::as_str)
                .unwrap_or("Bash")
                .to_string(),
            output: payload
                .get("output")
                .or_else(|| payload.get("tool_response"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            file_path: payload
                .get("file_path")
                .and_then(Value::as_str)
                .map(PathBuf::from),
        },
        "session-start" | "session_start" => HookEvent::SessionStart {
            turn_number: payload
                .get("turn_number")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            prompt: payload
                .get("prompt")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "pre-compact" | "pre_compact" => HookEvent::PreCompact,
        other => {
            eprintln!("codemode hook: unknown event '{other}'");
            return 2;
        }
    };

    match run_hook_sync(&root, db.as_ref(), cfg.as_ref(), event) {
        Ok(out) => {
            println!(
                "{}",
                serde_json::to_string(&out).unwrap_or_else(|_| "{}".into())
            );
            0
        }
        Err(err) => {
            eprintln!("codemode hook: {err}");
            1
        }
    }
}

fn run_index_cli(args: &[String]) -> i32 {
    let target = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let abs = canonical_or_lexical(&target);
    let root = canonical_or_lexical(&find_project_root(&abs));
    let cfg = load_project_config(&root).ok();
    let Ok(db) = ContextDb::open_for_project(&root) else {
        eprintln!("codemode index: cannot open ContextDb");
        return 1;
    };
    match index_path_gitignore(&db, &abs, cfg.as_ref(), 1024) {
        Ok(summary) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".into())
            );
            0
        }
        Err(err) => {
            eprintln!("codemode index: {err}");
            1
        }
    }
}

fn run_doctor_cli(args: &[String]) -> i32 {
    let root = extract_root_flag(args);
    println!(
        "{}",
        serde_json::to_string_pretty(&codemode_mcp::doctor_report(&root))
            .unwrap_or_else(|_| "{}".into())
    );
    0
}

fn run_stats_cli(args: &[String]) -> i32 {
    let root = extract_root_flag(args);
    match ContextDb::open_for_project(&root).and_then(|db| db.stats_report()) {
        Ok(stats) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&stats).unwrap_or_else(|_| "{}".into())
            );
            0
        }
        Err(err) => {
            eprintln!("codemode stats: {err}");
            1
        }
    }
}

fn run_purge_cli(args: &[String]) -> i32 {
    let root = extract_root_flag(args);
    match ContextDb::open_for_project(&root).and_then(|db| db.purge()) {
        Ok(()) => {
            println!("{{\"purged\":true}}");
            0
        }
        Err(err) => {
            eprintln!("codemode purge: {err}");
            1
        }
    }
}

fn run_upgrade_cli() -> i32 {
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "version": env!("CARGO_PKG_VERSION"),
            "install": "cargo install --locked --git https://github.com/landigit/codemode codemode-cli"
        }))
        .unwrap_or_else(|_| "{}".into())
    );
    0
}
