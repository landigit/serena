//! Rust port of `src/serena/olb/olb_jvm_backend.py`.
//!
//! Provides [`OraiosJvmLanguageBackend`] and supporting helpers for discovering
//! the `olb` CLI, adapting JetBrains IDE facade metadata, building launch/stop
//! commands, and shutting down an active JVM backend with bounded timeouts.

#![forbid(unsafe_code)]

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Backend identifier matching `BuiltinLanguageBackend.OLB_JVM.value` in
/// `src/serena/language_backend.py`.
pub const OLB_JVM_BACKEND_KEY: &str = "olb-jvm";

/// Language backend argument passed to `olb start` / `olb stop`.
pub const OLB_LANGUAGE_BACKEND: &str = "jvm";

/// Actionable installation error matching `OraiosJVMLanguageBackend._check_olb_installed`.
pub const OLB_NOT_FOUND_MESSAGE: &str = "The Oraios Language Backend CLI (olb) was not found on the PATH. Install it via `uv tool install olb-cli` and then install the JVM language backend via `olb install jvm`.";

/// Errors produced by `olb` discovery and command construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OlbError {
    BinaryNotFound,
}

impl fmt::Display for OlbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinaryNotFound => f.write_str(OLB_NOT_FOUND_MESSAGE),
        }
    }
}

impl std::error::Error for OlbError {}

/// Facade metadata adapted for the Oraios Language Backend (`_create_ide_facade`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdeFacadeSpec {
    pub name: String,
    pub description: String,
}

/// Outcome of [`OraiosJvmLanguageBackend::shutdown_active_project`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShutdownOutcome {
    /// `olb stop` exited with status code 0.
    Stopped,
    /// `olb stop` exited with a non-zero status (e.g. no instance running).
    NotStopped { code: Option<i32>, output: String },
    /// Process spawning, waiting, or timeout failed (non-fatal warning in Python).
    Failed { reason: String },
}

/// Port of `OraiosJVMLanguageBackend` (`src/serena/olb/olb_jvm_backend.py`).
#[derive(Debug, Clone)]
pub struct OraiosJvmLanguageBackend {
    key: String,
    olb_binary: OsString,
}

impl Default for OraiosJvmLanguageBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl OraiosJvmLanguageBackend {
    /// Creates a backend using the default key (`"olb-jvm"`) and `olb` executable name.
    pub fn new() -> Self {
        Self {
            key: OLB_JVM_BACKEND_KEY.to_string(),
            olb_binary: OsString::from("olb"),
        }
    }

    /// Creates a backend targeting an explicit `olb` executable path (useful in tests).
    pub fn with_binary<S: Into<OsString>>(olb_binary: S) -> Self {
        Self {
            key: OLB_JVM_BACKEND_KEY.to_string(),
            olb_binary: olb_binary.into(),
        }
    }

    /// Returns the backend key (`"olb-jvm"`).
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Returns `true`, matching `OraiosJVMLanguageBackend.is_jetbrains()`.
    pub fn is_jetbrains(&self) -> bool {
        true
    }

    /// Adapts a JetBrains IDE facade description into the OLB `"ide"` facade.
    pub fn adapt_ide_facade(description: &str) -> IdeFacadeSpec {
        IdeFacadeSpec {
            name: "ide".to_string(),
            description: description.replace("JetBrains", "Oraios"),
        }
    }

    /// Checks whether `olb` is available on the current `PATH`.
    pub fn check_olb_installed() -> Result<PathBuf, OlbError> {
        find_olb_in_path(std::env::var_os("PATH").as_deref())
    }

    /// Returns the launch command (`["olb", "start", "jvm"]`).
    pub fn launch_command(&self) -> Vec<OsString> {
        vec![
            self.olb_binary.clone(),
            OsString::from("start"),
            OsString::from(OLB_LANGUAGE_BACKEND),
        ]
    }

    /// Returns the stop command (`["olb", "stop", "--lb", "jvm", <project_root>]`).
    pub fn stop_command(&self, project_root: &Path) -> Vec<OsString> {
        vec![
            self.olb_binary.clone(),
            OsString::from("stop"),
            OsString::from("--lb"),
            OsString::from(OLB_LANGUAGE_BACKEND),
            project_root.as_os_str().to_os_string(),
        ]
    }

    /// Stops the active IDE instance for `project_root` within `ceil(timeout_secs)` seconds.
    ///
    /// Mirrors `OraiosJVMLanguageBackend.shutdown_active_project`: failures and non-zero exit
    /// codes are captured into [`ShutdownOutcome`] rather than panicking or propagating fatal
    /// errors.
    pub fn shutdown_active_project(
        &self,
        project_name: &str,
        project_root: &Path,
        timeout_secs: f64,
    ) -> ShutdownOutcome {
        let Some(timeout) = ceil_timeout_duration(timeout_secs) else {
            return ShutdownOutcome::Failed {
                reason: format!(
                    "Failed to stop the IDE instance for project {project_name}: invalid timeout {timeout_secs}"
                ),
            };
        };

        let mut cmd = Command::new(&self.olb_binary);
        cmd.arg("stop")
            .arg("--lb")
            .arg(OLB_LANGUAGE_BACKEND)
            .arg(project_root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(err) => {
                return ShutdownOutcome::Failed {
                    reason: format!(
                        "Failed to stop the IDE instance for project {project_name}: {err}"
                    ),
                };
            }
        };

        let stdout_handle = child.stdout.take().map(spawn_pipe_reader);
        let stderr_handle = child.stderr.take().map(spawn_pipe_reader);

        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => {
                    let now = Instant::now();
                    if now >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break Err(format!(
                            "Failed to stop the IDE instance for project {project_name}: timed out after {}s",
                            timeout.as_secs()
                        ));
                    }
                    let remaining = deadline.saturating_duration_since(now);
                    thread::sleep(remaining.min(Duration::from_millis(10)));
                }
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(format!(
                        "Failed to stop the IDE instance for project {project_name}: {err}"
                    ));
                }
            }
        };

        match status {
            Err(reason) => ShutdownOutcome::Failed { reason },
            Ok(exit) if exit.success() => ShutdownOutcome::Stopped,
            Ok(exit) => {
                let stdout = join_pipe_reader(stdout_handle);
                let stderr = join_pipe_reader(stderr_handle);
                let stderr_text = String::from_utf8_lossy(&stderr);
                let stdout_text = String::from_utf8_lossy(&stdout);
                let selected = if stderr_text.is_empty() {
                    stdout_text.trim()
                } else {
                    stderr_text.trim()
                };
                ShutdownOutcome::NotStopped {
                    code: exit.code(),
                    output: selected.to_string(),
                }
            }
        }
    }
}

/// Searches `path_env` (such as `$PATH`) for an executable `olb` binary.
pub fn find_olb_in_path(path_env: Option<&OsStr>) -> Result<PathBuf, OlbError> {
    let Some(path_env) = path_env else {
        return Err(OlbError::BinaryNotFound);
    };

    for dir in std::env::split_paths(path_env) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for candidate in candidate_binary_paths(&dir) {
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
    }

    Err(OlbError::BinaryNotFound)
}

fn candidate_binary_paths(dir: &Path) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        vec![
            dir.join("olb.exe"),
            dir.join("olb.cmd"),
            dir.join("olb.bat"),
            dir.join("olb"),
        ]
    }
    #[cfg(not(windows))]
    {
        vec![dir.join("olb")]
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn ceil_timeout_duration(timeout_secs: f64) -> Option<Duration> {
    if timeout_secs.is_nan() || timeout_secs < 0.0 {
        return None;
    }
    if timeout_secs == 0.0 {
        return Some(Duration::ZERO);
    }
    if !timeout_secs.is_finite() {
        return None;
    }
    let ceiled = timeout_secs.ceil();
    if ceiled > u64::MAX as f64 {
        return Some(Duration::from_secs(u64::MAX));
    }
    Some(Duration::from_secs(ceiled as u64))
}

fn spawn_pipe_reader<R: Read + Send + 'static>(mut reader: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = reader.read_to_end(&mut buf);
        buf
    })
}

fn join_pipe_reader(handle: Option<thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    handle.and_then(|h| h.join().ok()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "serena-olb-test-{prefix}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[cfg(unix)]
    fn write_executable_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    #[test]
    fn metadata_and_facade_adaptation_match_python() {
        let backend = OraiosJvmLanguageBackend::new();
        assert_eq!(backend.key(), "olb-jvm");
        assert!(backend.is_jetbrains());

        let adapted = OraiosJvmLanguageBackend::adapt_ide_facade(
            "JetBrains IDE integration via JetBrains plugin",
        );
        assert_eq!(adapted.name, "ide");
        assert_eq!(
            adapted.description,
            "Oraios IDE integration via Oraios plugin"
        );
    }

    #[test]
    fn launch_and_stop_commands_include_expected_arguments() {
        let backend = OraiosJvmLanguageBackend::new();
        assert_eq!(
            backend.launch_command(),
            vec![
                OsString::from("olb"),
                OsString::from("start"),
                OsString::from("jvm")
            ]
        );
        let root = Path::new("/tmp/my project");
        assert_eq!(
            backend.stop_command(root),
            vec![
                OsString::from("olb"),
                OsString::from("stop"),
                OsString::from("--lb"),
                OsString::from("jvm"),
                OsString::from("/tmp/my project"),
            ]
        );
    }

    #[test]
    fn find_olb_in_path_handles_missing_non_executable_and_directory_entries() {
        assert_eq!(
            find_olb_in_path(None).unwrap_err(),
            OlbError::BinaryNotFound
        );
        assert_eq!(
            find_olb_in_path(Some(OsStr::new(""))).unwrap_err(),
            OlbError::BinaryNotFound
        );
        assert_eq!(OlbError::BinaryNotFound.to_string(), OLB_NOT_FOUND_MESSAGE);

        #[cfg(unix)]
        {
            let tmp = TempDir::new("path");
            let first = tmp.path.join("first");
            let second = tmp.path.join("second");
            std::fs::create_dir_all(first.join("olb")).unwrap();
            std::fs::create_dir_all(&second).unwrap();

            let non_exec = first.join("non_exec_dir");
            std::fs::create_dir_all(&non_exec).unwrap();
            std::fs::write(non_exec.join("olb"), "#!/bin/sh\n").unwrap();

            let joined_missing = std::env::join_paths([&first, &non_exec]).unwrap();
            assert_eq!(
                find_olb_in_path(Some(joined_missing.as_os_str())).unwrap_err(),
                OlbError::BinaryNotFound
            );

            let real_bin = write_executable_script(&second, "olb", "#!/bin/sh\nexit 0\n");
            let joined_found = std::env::join_paths([&first, &non_exec, &second]).unwrap();
            assert_eq!(
                find_olb_in_path(Some(joined_found.as_os_str())).unwrap(),
                real_bin
            );
        }
    }

    #[test]
    fn ceil_timeout_handles_fractional_zero_and_invalid_values() {
        assert_eq!(ceil_timeout_duration(0.0), Some(Duration::ZERO));
        assert_eq!(ceil_timeout_duration(0.2), Some(Duration::from_secs(1)));
        assert_eq!(ceil_timeout_duration(2.0), Some(Duration::from_secs(2)));
        assert_eq!(ceil_timeout_duration(2.1), Some(Duration::from_secs(3)));
        assert_eq!(ceil_timeout_duration(-1.0), None);
        assert_eq!(ceil_timeout_duration(f64::NAN), None);
        assert_eq!(ceil_timeout_duration(f64::INFINITY), None);
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_active_project_handles_success_nonzero_timeout_and_missing_binary() {
        let tmp = TempDir::new("shutdown");
        let project_root = tmp.path.join("proj");
        std::fs::create_dir_all(&project_root).unwrap();

        let args_log = tmp.path.join("args.txt");
        let ok_bin = write_executable_script(
            &tmp.path,
            "olb-ok",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nexit 0\n",
                args_log.display()
            ),
        );
        let ok_backend = OraiosJvmLanguageBackend::with_binary(&ok_bin);
        assert_eq!(
            ok_backend.shutdown_active_project("demo", &project_root, 1.2),
            ShutdownOutcome::Stopped
        );
        let recorded = std::fs::read_to_string(&args_log).unwrap();
        let lines: Vec<_> = recorded.lines().collect();
        assert_eq!(
            lines,
            vec!["stop", "--lb", "jvm", project_root.to_str().unwrap()]
        );

        let stderr_bin = write_executable_script(
            &tmp.path,
            "olb-stderr",
            "#!/bin/sh\necho 'stdout ignored' \necho '  not running  ' >&2\nexit 1\n",
        );
        assert_eq!(
            OraiosJvmLanguageBackend::with_binary(&stderr_bin).shutdown_active_project(
                "demo",
                &project_root,
                2.0
            ),
            ShutdownOutcome::NotStopped {
                code: Some(1),
                output: "not running".to_string(),
            }
        );

        let stdout_bin = write_executable_script(
            &tmp.path,
            "olb-stdout",
            "#!/bin/sh\necho '  stopped already  '\nexit 2\n",
        );
        assert_eq!(
            OraiosJvmLanguageBackend::with_binary(&stdout_bin).shutdown_active_project(
                "demo",
                &project_root,
                2.0
            ),
            ShutdownOutcome::NotStopped {
                code: Some(2),
                output: "stopped already".to_string(),
            }
        );

        let slow_bin = write_executable_script(&tmp.path, "olb-slow", "#!/bin/sh\nsleep 5\n");
        let slow_outcome = OraiosJvmLanguageBackend::with_binary(&slow_bin)
            .shutdown_active_project("demo", &project_root, 0.2);
        match slow_outcome {
            ShutdownOutcome::Failed { reason } => {
                assert!(
                    reason.contains("timed out after 1s"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected Failed on timeout, got {other:?}"),
        }

        let missing_outcome =
            OraiosJvmLanguageBackend::with_binary(tmp.path.join("does-not-exist"))
                .shutdown_active_project("demo", &project_root, 1.0);
        assert!(matches!(missing_outcome, ShutdownOutcome::Failed { .. }));

        let neg_timeout = OraiosJvmLanguageBackend::with_binary(&ok_bin).shutdown_active_project(
            "demo",
            &project_root,
            -0.5,
        );
        assert!(matches!(neg_timeout, ShutdownOutcome::Failed { .. }));

        let nan_timeout = OraiosJvmLanguageBackend::with_binary(&ok_bin).shutdown_active_project(
            "demo",
            &project_root,
            f64::NAN,
        );
        assert!(matches!(nan_timeout, ShutdownOutcome::Failed { .. }));

        let zero_timeout = OraiosJvmLanguageBackend::with_binary(&slow_bin)
            .shutdown_active_project("demo", &project_root, 0.0);
        assert!(matches!(zero_timeout, ShutdownOutcome::Failed { .. }));
    }
}
