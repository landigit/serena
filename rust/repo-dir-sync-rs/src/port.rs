//! The Rust port of `repo_dir_sync.py`.
//!
//! Replace each `unimplemented!` with the port of the source function. Return `Err` where
//! the source throws or raises: the caller gets an error there too. After changing a
//! signature, run `rust-anything refresh` to regenerate the bindings. `rust-anything diff` checks
//! behavior against what the source recorded.

#![forbid(unsafe_code)]

/// Port of `popen` (`repo_dir_sync.py:12`).
pub fn popen(cmd: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &cmd;
    unimplemented!("port `popen` from repo_dir_sync.py")
}

/// Port of `call` (`repo_dir_sync.py:18`).
pub fn call(cmd: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &cmd;
    unimplemented!("port `call` from repo_dir_sync.py")
}

/// Port of `execute` (`repo_dir_sync.py:23`).
///
/// `exception_on_error` defaults to `True` in the source.
pub fn execute(
    cmd: serde_json::Value,
    exception_on_error: bool,
) -> Result<serde_json::Value, String> {
    let _ = (&cmd, &exception_on_error);
    unimplemented!("port `execute` from repo_dir_sync.py")
}

/// Port of `gitLog` (`repo_dir_sync.py:40`).
pub fn git_log(
    path: serde_json::Value,
    arg: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let _ = (&path, &arg);
    unimplemented!("port `gitLog` from repo_dir_sync.py")
}

/// Port of `gitCommit` (`repo_dir_sync.py:48`).
pub fn git_commit(msg: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &msg;
    unimplemented!("port `gitCommit` from repo_dir_sync.py")
}

/// Port of `gitCommitWithMessageFromFile` (`repo_dir_sync.py:54`).
pub fn git_commit_with_message_from_file(
    commit_msg_filename: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let _ = &commit_msg_filename;
    unimplemented!("port `gitCommitWithMessageFromFile` from repo_dir_sync.py")
}
