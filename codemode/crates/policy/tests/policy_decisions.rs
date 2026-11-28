use std::path::Path;

use codemode_core::config::ProjectConfig;
use codemode_policy::{CommandDecision, evaluate_file_read, evaluate_shell_command};

#[test]
fn unwraps_wrappers_pipelines_subshells_and_blocks_bulk_source_reads() {
    let dir = std::env::temp_dir().join(format!("codemode-policy-cmd-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    let big_src = dir.join("src/engine.rs");
    let small_src = dir.join("src/tiny.rs");
    std::fs::write(&big_src, "fn x() {}\n".repeat(1200)).unwrap();
    std::fs::write(&small_src, "fn ok() {}\n").unwrap();

    let cfg = ProjectConfig::default();

    assert!(matches!(
        evaluate_shell_command(&format!("cat {}", big_src.display()), &dir, Some(&cfg)),
        CommandDecision::Deny { .. }
    ));

    assert!(matches!(
        evaluate_shell_command(
            &format!("env FOO=1 bash -c \"cat {}\"", big_src.display()),
            &dir,
            Some(&cfg)
        ),
        CommandDecision::Deny { .. }
    ));

    assert!(matches!(
        evaluate_shell_command(
            &format!("sh -c 'tail -n +1 {} | grep fn'", big_src.display()),
            &dir,
            Some(&cfg)
        ),
        CommandDecision::Deny { .. }
    ));

    assert!(matches!(
        evaluate_shell_command(
            &format!("echo $(sed -n '1,900p' {})", big_src.display()),
            &dir,
            Some(&cfg)
        ),
        CommandDecision::Deny { .. }
    ));

    assert!(matches!(
        evaluate_shell_command(
            &format!("head -n 20 {}", big_src.display()),
            &dir,
            Some(&cfg)
        ),
        CommandDecision::Allow
    ));

    assert!(matches!(
        evaluate_shell_command(&format!("cat {}", small_src.display()), &dir, Some(&cfg)),
        CommandDecision::Allow
    ));

    assert!(matches!(
        evaluate_shell_command("cargo test --offline && git status", &dir, Some(&cfg)),
        CommandDecision::Allow
    ));

    assert!(matches!(
        evaluate_file_read(&big_src, false, Some(&cfg)),
        CommandDecision::Deny { .. }
    ));
    assert!(matches!(
        evaluate_file_read(&big_src, true, Some(&cfg)),
        CommandDecision::Allow
    ));
    let _ = Path::new(".");
}
