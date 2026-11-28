//! Runtime dependency discovery, outdated install cleanup, and Expert setup.
//!
//! Mirrors `ElixirTools._get_elixir_version`, `ElixirTools._remove_outdated_expert_install`,
//! `ElixirTools._setup_runtime_dependencies`, and the host/SHA-256 verification rules from
//! `FileUtils` in `src/solidlsp/ls_utils.py`.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use url::Url;

use crate::sha256::calculate_file_sha256;

/// Default pinned Expert language server release tag.
pub const EXPERT_VERSION: &str = "v0.1.10";

/// Allowed download hosts for Expert release artifacts.
pub const EXPERT_ALLOWED_HOSTS: &[&str] = &[
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
];

/// SHA-256 checksums of previously pinned Expert binaries (`v0.1.0-rc.6`) known to crash
/// on `documentSymbol` and `definition` requests.
pub const EXPERT_OUTDATED_SHA256S: &[&str] = &[
    "643a492ff972246668b0ca356a84c3d0a0f5feeae0ab5dc1b9a126876ed460e4", // v0.1.0-rc.6 linux_amd64
    "d8b830bdaa8991d7ebf255dacbb3674f3ea335c87d0bfba4b7f907ded4a8f014", // v0.1.0-rc.6 linux_arm64
    "964f316f1633090b33aab392b6b85fb778c5fb3c0db862671424458da34b1d4d", // v0.1.0-rc.6 darwin_amd64
    "5fb5be151baedd635d99835cf3f9986afc9af6ae7b07bd001a1962f4298e45da", // v0.1.0-rc.6 darwin_arm64
    "babee77d2653679021600b99c68d984d4463290cb221e0fc0d1093b3afdeb3b0", // v0.1.0-rc.6 windows_amd64
];

/// Error message raised when Elixir is missing from `PATH`.
pub const ELIXIR_NOT_INSTALLED_MESSAGE: &str = "Elixir is not installed. Please install Elixir from https://elixir-lang.org/install.html and make sure it is added to your PATH.";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Logging severity aligned with Python `logging` levels used in `ElixirTools`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// Sink for diagnostic and lifecycle messages emitted during setup and LSP startup.
pub trait LogSink {
    fn log(&self, level: LogLevel, message: &str);
}

/// No-op log sink.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullLogSink;

impl LogSink for NullLogSink {
    fn log(&self, _level: LogLevel, _message: &str) {}
}

/// Supported and known `SolidLSP` platform identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformId {
    WinX86,
    WinX64,
    WinArm64,
    Osx,
    OsxX64,
    OsxArm64,
    LinuxX86,
    LinuxX64,
    LinuxArm64,
    LinuxMuslX64,
    LinuxMuslArm64,
    FreebsdX64,
    FreebsdArm64,
}

impl PlatformId {
    /// Returns the `PlatformId` string representation matching `solidlsp.ls_utils.PlatformId.value`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WinX86 => "win-x86",
            Self::WinX64 => "win-x64",
            Self::WinArm64 => "win-arm64",
            Self::Osx => "osx",
            Self::OsxX64 => "osx-x64",
            Self::OsxArm64 => "osx-arm64",
            Self::LinuxX86 => "linux-x86",
            Self::LinuxX64 => "linux-x64",
            Self::LinuxArm64 => "linux-arm64",
            Self::LinuxMuslX64 => "linux-musl-x64",
            Self::LinuxMuslArm64 => "linux-musl-arm64",
            Self::FreebsdX64 => "freebsd-x64",
            Self::FreebsdArm64 => "freebsd-arm64",
        }
    }

    pub const fn is_windows(self) -> bool {
        matches!(self, Self::WinX86 | Self::WinX64 | Self::WinArm64)
    }

    /// Detects the current host platform.
    pub fn current() -> Result<Self, ElixirToolsError> {
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let is_musl = cfg!(target_env = "musl");

        match (os, arch, is_musl) {
            ("windows", "x86", _) => Ok(Self::WinX86),
            ("windows", "x86_64", _) => Ok(Self::WinX64),
            ("windows", "aarch64", _) => Ok(Self::WinArm64),
            ("macos", "x86_64", _) => Ok(Self::OsxX64),
            ("macos", "aarch64", _) => Ok(Self::OsxArm64),
            ("linux", "x86", _) => Ok(Self::LinuxX86),
            ("linux", "x86_64", false) => Ok(Self::LinuxX64),
            ("linux", "aarch64", false) => Ok(Self::LinuxArm64),
            ("linux", "x86_64", true) => Ok(Self::LinuxMuslX64),
            ("linux", "aarch64", true) => Ok(Self::LinuxMuslArm64),
            ("freebsd", "x86_64", _) => Ok(Self::FreebsdX64),
            ("freebsd", "aarch64", _) => Ok(Self::FreebsdArm64),
            _ => Err(ElixirToolsError::UnknownPlatform {
                os: os.to_string(),
                arch: arch.to_string(),
            }),
        }
    }
}

impl fmt::Display for PlatformId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime dependency descriptor matching `solidlsp.language_servers.common.RuntimeDependency`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDependency {
    pub id: String,
    pub platform_id: String,
    pub url: String,
    pub archive_type: &'static str,
    pub binary_name: String,
    pub extract_path: String,
    pub sha256: Option<String>,
    pub allowed_hosts: Vec<String>,
}

/// Elixir-specific LS settings (`ls_specific_settings["elixir"]`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ElixirSettings {
    /// Override the pinned Expert version downloaded by Serena (`expert_version`).
    pub expert_version: Option<String>,
}

impl ElixirSettings {
    pub fn resolved_expert_version(&self) -> &str {
        self.expert_version.as_deref().unwrap_or(EXPERT_VERSION)
    }
}

/// Outcome of [`remove_outdated_expert_install`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutdatedInstallStatus {
    NotPresent,
    Removed { sha256: String },
    Kept { sha256: String },
}

/// Errors produced by runtime dependency setup and LSP startup.
#[derive(Debug)]
pub enum ElixirToolsError {
    ElixirNotInstalled,
    UnsupportedPlatform(PlatformId),
    UnknownPlatform {
        os: String,
        arch: String,
    },
    DisallowedHost {
        host: String,
        allowed_hosts: Vec<String>,
    },
    InvalidUrl(String),
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    DownloadFailed(String),
    MissingExecutable(PathBuf),
    MissingCapability(String),
    LspProtocol(String),
    Io(io::Error),
}

impl fmt::Display for ElixirToolsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ElixirNotInstalled => f.write_str(ELIXIR_NOT_INSTALLED_MESSAGE),
            Self::UnsupportedPlatform(platform) => {
                write!(f, "Platform {platform} is not supported for Expert")
            }
            Self::UnknownPlatform { os, arch } => {
                write!(f, "Unknown platform: os={os}, arch={arch}")
            }
            Self::DisallowedHost {
                host,
                allowed_hosts,
            } => write!(
                f,
                "Refusing to download from host '{host}'; allowed hosts: {allowed_hosts:?}"
            ),
            Self::InvalidUrl(msg) => write!(f, "Invalid download URL: {msg}"),
            Self::ChecksumMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "Checksum verification failed for '{}': expected {expected}, got {actual}",
                path.display()
            ),
            Self::DownloadFailed(msg) => write!(f, "Error downloading file: {msg}"),
            Self::MissingExecutable(path) => {
                write!(f, "Expert executable not found at {}", path.display())
            }
            Self::MissingCapability(cap) => {
                write!(f, "Missing {cap} in server capabilities")
            }
            Self::LspProtocol(msg) => write!(f, "LSP protocol error: {msg}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
        }
    }
}

impl std::error::Error for ElixirToolsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ElixirToolsError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Returns the Expert binary [`RuntimeDependency`] for `platform` and `expert_version`.
pub fn runtime_dependency_for_platform(
    platform: PlatformId,
    expert_version: &str,
) -> Result<RuntimeDependency, ElixirToolsError> {
    let default_sha = |hash: &str| {
        if expert_version == EXPERT_VERSION {
            Some(hash.to_string())
        } else {
            None
        }
    };
    let allowed_hosts: Vec<String> = EXPERT_ALLOWED_HOSTS
        .iter()
        .map(|host| (*host).to_string())
        .collect();

    let (id, binary_name, extract_path, sha256) = match platform {
        PlatformId::LinuxX64 => (
            "expert_linux_amd64",
            "expert_linux_amd64",
            "expert",
            default_sha("af29d5270503263139f10fccb597430feb0c510da48386821b176fbfdfe33f8d"),
        ),
        PlatformId::LinuxArm64 => (
            "expert_linux_arm64",
            "expert_linux_arm64",
            "expert",
            default_sha("eaac1a779dc8319099576edcd0c43dde3a383abf2e75ccd6989cdabf3900cd9b"),
        ),
        PlatformId::OsxX64 => (
            "expert_darwin_amd64",
            "expert_darwin_amd64",
            "expert",
            default_sha("5cb885b7e83e73c9c8fc16c27f398391311629113a0a6e26134e5ae4b7005035"),
        ),
        PlatformId::OsxArm64 => (
            "expert_darwin_arm64",
            "expert_darwin_arm64",
            "expert",
            default_sha("992a7c6c0ad062667d6518ca378c62fe3a465d3f57350d5123028741195f1e1a"),
        ),
        PlatformId::WinX64 => (
            "expert_windows_amd64",
            "expert_windows_amd64.exe",
            "expert.exe",
            default_sha("163cb83a75316d77068fd1d00eb43e113dd2bb2d417af48d3e5374f6763a5880"),
        ),
        other => return Err(ElixirToolsError::UnsupportedPlatform(other)),
    };

    Ok(RuntimeDependency {
        id: id.to_string(),
        platform_id: platform.as_str().to_string(),
        url: format!(
            "https://github.com/elixir-lang/expert/releases/download/{expert_version}/{binary_name}"
        ),
        archive_type: "binary",
        binary_name: binary_name.to_string(),
        extract_path: extract_path.to_string(),
        sha256,
        allowed_hosts,
    })
}

/// Validates that `raw_url` targets one of `allowed_hosts` (case-insensitive).
pub fn validate_download_host(
    raw_url: &str,
    allowed_hosts: &[String],
) -> Result<(), ElixirToolsError> {
    if allowed_hosts.is_empty() {
        return Ok(());
    }

    let parsed =
        Url::parse(raw_url).map_err(|err| ElixirToolsError::InvalidUrl(err.to_string()))?;
    let host = parsed.host_str();
    let normalized_allowed: Vec<String> = {
        let mut items: Vec<String> = allowed_hosts
            .iter()
            .map(|h| h.to_ascii_lowercase())
            .collect();
        items.sort();
        items.dedup();
        items
    };

    let Some(host_str) = host else {
        return Err(ElixirToolsError::DisallowedHost {
            host: "<unknown>".to_string(),
            allowed_hosts: normalized_allowed,
        });
    };

    let host_lower = host_str.to_ascii_lowercase();
    if !normalized_allowed
        .iter()
        .any(|allowed| allowed == &host_lower)
    {
        return Err(ElixirToolsError::DisallowedHost {
            host: host_str.to_string(),
            allowed_hosts: normalized_allowed,
        });
    }

    Ok(())
}

/// Probe for discovering the installed Elixir version (`elixir --version`).
pub trait ElixirVersionProbe {
    fn get_elixir_version(&self) -> Result<Option<String>, ElixirToolsError>;
}

/// Runs `elixir --version` (or `cmd.exe /d /c elixir --version` on Windows) on the host.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemElixirVersionProbe;

impl SystemElixirVersionProbe {
    pub fn command_args(is_windows: bool) -> Vec<&'static str> {
        if is_windows {
            vec!["cmd.exe", "/d", "/c", "elixir", "--version"]
        } else {
            vec!["elixir", "--version"]
        }
    }
}

impl ElixirVersionProbe for SystemElixirVersionProbe {
    fn get_elixir_version(&self) -> Result<Option<String>, ElixirToolsError> {
        let args = Self::command_args(cfg!(windows));
        let output = match Command::new(args[0]).args(&args[1..]).output() {
            Ok(out) => out,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(ElixirToolsError::Io(err)),
        };

        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_string(),
            ))
        } else {
            Ok(None)
        }
    }
}

/// Output produced by [`BinaryDownloader::download`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    /// Final resolved URL after any HTTP redirects, validated against `allowed_hosts`.
    pub final_url: String,
}

/// Abstraction for downloading an Expert binary into a staging file path.
pub trait BinaryDownloader {
    fn download(&self, url: &str, dest_path: &Path) -> Result<DownloadOutcome, ElixirToolsError>;
}

/// Searches `path_env` (like `$PATH`) for an executable binary with name `binary_name`.
pub fn find_executable_in_path(binary_name: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    find_executable_in_path_with_pathext(
        binary_name,
        path_env,
        std::env::var_os("PATHEXT").as_deref(),
    )
}

pub fn find_executable_in_path_with_pathext(
    binary_name: &str,
    path_env: Option<&OsStr>,
    pathext_env: Option<&OsStr>,
) -> Option<PathBuf> {
    let path_env = path_env?;
    for dir in std::env::split_paths(path_env) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for candidate in candidate_binary_paths(&dir, binary_name, pathext_env) {
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn candidate_binary_paths(
    dir: &Path,
    binary_name: &str,
    pathext_env: Option<&OsStr>,
) -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        windows_candidate_binary_paths(dir, binary_name, pathext_env)
    }
    #[cfg(not(windows))]
    {
        let _ = pathext_env;
        vec![dir.join(binary_name)]
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn windows_candidate_binary_paths(
    dir: &Path,
    binary_name: &str,
    pathext_env: Option<&OsStr>,
) -> Vec<PathBuf> {
    let raw = pathext_env
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(".COM;.EXE;.BAT;.CMD");

    let mut paths = Vec::new();
    for ext in raw.split(';') {
        let trimmed = ext.trim();
        if trimmed.is_empty() {
            continue;
        }
        let suffix = trimmed.trim_start_matches('.');
        if suffix.is_empty() {
            continue;
        }
        paths.push(dir.join(format!("{binary_name}.{}", suffix.to_ascii_lowercase())));
    }
    paths
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
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

/// Removes a previously downloaded Expert installation if its SHA-256 matches
/// [`EXPERT_OUTDATED_SHA256S`].
///
/// Mirrors `ElixirTools._remove_outdated_expert_install`.
pub fn remove_outdated_expert_install(
    expert_dir: &Path,
    executable_path: &Path,
    logger: &dyn LogSink,
) -> Result<OutdatedInstallStatus, ElixirToolsError> {
    remove_outdated_expert_install_with_hashes(
        expert_dir,
        executable_path,
        EXPERT_OUTDATED_SHA256S,
        logger,
    )
}

/// Removes a previously downloaded Expert installation if its SHA-256 matches any entry in
/// `outdated_sha256s`.
pub fn remove_outdated_expert_install_with_hashes(
    expert_dir: &Path,
    executable_path: &Path,
    outdated_sha256s: &[&str],
    logger: &dyn LogSink,
) -> Result<OutdatedInstallStatus, ElixirToolsError> {
    if !executable_path.exists() {
        return Ok(OutdatedInstallStatus::NotPresent);
    }

    let real_path = fs::canonicalize(executable_path)?;
    let installed_sha256 = calculate_file_sha256(&real_path)?;
    if outdated_sha256s
        .iter()
        .any(|hash| hash.eq_ignore_ascii_case(&installed_sha256))
    {
        logger.log(
            LogLevel::Warning,
            &format!(
                "Found outdated Expert binary at {} (sha256={installed_sha256}); removing {} so that Expert {EXPERT_VERSION} is downloaded",
                executable_path.display(),
                expert_dir.display(),
            ),
        );
        fs::remove_dir_all(expert_dir)?;
        return Ok(OutdatedInstallStatus::Removed {
            sha256: installed_sha256,
        });
    }

    Ok(OutdatedInstallStatus::Kept {
        sha256: installed_sha256,
    })
}

/// Ensures the Expert runtime dependency is available and returns the path to the executable.
///
/// Mirrors `ElixirTools._setup_runtime_dependencies`.
pub fn setup_runtime_dependencies(
    settings: &ElixirSettings,
    ls_resources_dir: &Path,
    platform: PlatformId,
    path_env: Option<&OsStr>,
    elixir_probe: &dyn ElixirVersionProbe,
    downloader: &dyn BinaryDownloader,
    logger: &dyn LogSink,
) -> Result<PathBuf, ElixirToolsError> {
    let expert_version = settings.resolved_expert_version();

    let Some(elixir_version) = elixir_probe.get_elixir_version()? else {
        return Err(ElixirToolsError::ElixirNotInstalled);
    };
    if elixir_version.is_empty() {
        return Err(ElixirToolsError::ElixirNotInstalled);
    }
    logger.log(LogLevel::Info, &format!("Found Elixir: {elixir_version}"));

    if let Some(expert_in_path) = find_executable_in_path("expert", path_env) {
        logger.log(
            LogLevel::Info,
            &format!("Found Expert in PATH: {}", expert_in_path.display()),
        );
        return Ok(expert_in_path);
    }

    let dependency = runtime_dependency_for_platform(platform, expert_version)?;
    let expert_dir = ls_resources_dir.join("expert");
    let executable_name = if platform.is_windows() {
        "expert.exe"
    } else {
        "expert"
    };
    let executable_path = expert_dir.join(executable_name);
    let binary_path = expert_dir.join(&dependency.binary_name);

    if expert_version == EXPERT_VERSION {
        remove_outdated_expert_install(&expert_dir, &executable_path, logger)?;
    }

    if !executable_path.exists() {
        logger.log(
            LogLevel::Info,
            &format!("Downloading Expert binary from {}", dependency.url),
        );
        download_file_verified(&dependency, &binary_path, downloader)?;

        #[cfg(unix)]
        if !platform.is_windows() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o755))?;
        }

        if binary_path != executable_path && !platform.is_windows() {
            #[cfg(unix)]
            {
                if executable_path.symlink_metadata().is_ok() {
                    fs::remove_file(&executable_path)?;
                }
                let target_name = binary_path
                    .file_name()
                    .map(OsStr::to_os_string)
                    .unwrap_or_else(|| OsString::from(&dependency.binary_name));
                std::os::unix::fs::symlink(target_name, &executable_path)?;
            }
        }

        if binary_path != executable_path && platform.is_windows() {
            fs::copy(&binary_path, &executable_path)?;
        }
    }

    if !executable_path.exists() {
        return Err(ElixirToolsError::MissingExecutable(executable_path));
    }

    logger.log(
        LogLevel::Info,
        &format!("Expert binary ready at: {}", executable_path.display()),
    );
    Ok(executable_path)
}

struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed && self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn download_file_verified(
    dependency: &RuntimeDependency,
    target_path: &Path,
    downloader: &dyn BinaryDownloader,
) -> Result<(), ElixirToolsError> {
    validate_download_host(&dependency.url, &dependency.allowed_hosts)?;

    let target_dir = target_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(target_dir)?;

    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("expert");
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = target_dir.join(format!(
        ".{file_name}.{}.{seq}.download",
        std::process::id()
    ));
    let mut guard = TempFileGuard::new(temp_path.clone());

    let outcome = downloader.download(&dependency.url, &temp_path)?;
    validate_download_host(&outcome.final_url, &dependency.allowed_hosts)?;

    if let Some(expected_sha256) = &dependency.sha256 {
        let actual_sha256 = calculate_file_sha256(&temp_path)?;
        if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
            return Err(ElixirToolsError::ChecksumMismatch {
                path: temp_path,
                expected: expected_sha256.clone(),
                actual: actual_sha256,
            });
        }
    }

    fs::rename(&temp_path, target_path)?;
    guard.disarm();
    Ok(())
}

/// Helper for tests or local file mirrors: writes raw bytes via `BinaryDownloader`.
pub fn write_downloaded_bytes(dest_path: &Path, bytes: &[u8]) -> Result<(), ElixirToolsError> {
    let mut file = File::create(dest_path)?;
    file.write_all(bytes)?;
    file.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sha256::sha256_hex;
    use std::cell::RefCell;
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
            let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "elixir-tools-test-{prefix}-{}-{unique}-{seq}",
                std::process::id()
            ));
            fs::create_dir_all(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[derive(Default)]
    struct RecordingLogger {
        entries: RefCell<Vec<(LogLevel, String)>>,
    }

    impl LogSink for RecordingLogger {
        fn log(&self, level: LogLevel, message: &str) {
            self.entries.borrow_mut().push((level, message.to_string()));
        }
    }

    struct MockElixirProbe(Result<Option<String>, ElixirToolsError>);

    impl ElixirVersionProbe for MockElixirProbe {
        fn get_elixir_version(&self) -> Result<Option<String>, ElixirToolsError> {
            match &self.0 {
                Ok(val) => Ok(val.clone()),
                Err(ElixirToolsError::ElixirNotInstalled) => {
                    Err(ElixirToolsError::ElixirNotInstalled)
                }
                Err(other) => Err(ElixirToolsError::DownloadFailed(other.to_string())),
            }
        }
    }

    struct MockDownloader {
        payload: Vec<u8>,
        final_url_override: Option<String>,
        calls: RefCell<Vec<String>>,
    }

    impl BinaryDownloader for MockDownloader {
        fn download(
            &self,
            url: &str,
            dest_path: &Path,
        ) -> Result<DownloadOutcome, ElixirToolsError> {
            self.calls.borrow_mut().push(url.to_string());
            write_downloaded_bytes(dest_path, &self.payload)?;
            Ok(DownloadOutcome {
                final_url: self
                    .final_url_override
                    .clone()
                    .unwrap_or_else(|| url.to_string()),
            })
        }
    }

    #[test]
    fn runtime_dependency_handles_supported_and_unsupported_platforms() {
        let dep = runtime_dependency_for_platform(PlatformId::LinuxX64, EXPERT_VERSION).unwrap();
        assert_eq!(dep.id, "expert_linux_amd64");
        assert_eq!(
            dep.url,
            "https://github.com/elixir-lang/expert/releases/download/v0.1.10/expert_linux_amd64"
        );
        assert_eq!(
            dep.sha256.as_deref(),
            Some("af29d5270503263139f10fccb597430feb0c510da48386821b176fbfdfe33f8d")
        );

        let custom = runtime_dependency_for_platform(PlatformId::WinX64, "v0.2.0").unwrap();
        assert_eq!(custom.binary_name, "expert_windows_amd64.exe");
        assert_eq!(custom.extract_path, "expert.exe");
        assert_eq!(custom.sha256, None);

        let unsupported = runtime_dependency_for_platform(PlatformId::LinuxMuslX64, EXPERT_VERSION);
        assert!(matches!(
            unsupported,
            Err(ElixirToolsError::UnsupportedPlatform(
                PlatformId::LinuxMuslX64
            ))
        ));
    }

    #[test]
    fn host_validation_accepts_allowed_and_rejects_others() {
        let allowed: Vec<String> = EXPERT_ALLOWED_HOSTS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert!(validate_download_host("https://github.com/elixir-lang/expert", &allowed).is_ok());
        assert!(
            validate_download_host("https://OBJECTS.GITHUBUSERCONTENT.COM/asset", &allowed).is_ok()
        );
        assert!(matches!(
            validate_download_host("https://evil.example.com/expert", &allowed),
            Err(ElixirToolsError::DisallowedHost { .. })
        ));
        assert!(matches!(
            validate_download_host("not-a-url", &allowed),
            Err(ElixirToolsError::InvalidUrl(_))
        ));
    }

    #[test]
    fn remove_outdated_install_handles_missing_kept_and_outdated_symlink() {
        let tmp = TempDir::new("outdated");
        let expert_dir = tmp.path.join("expert");
        let exe_path = expert_dir.join("expert");
        let logger = RecordingLogger::default();

        assert_eq!(
            remove_outdated_expert_install(&expert_dir, &exe_path, &logger).unwrap(),
            OutdatedInstallStatus::NotPresent
        );

        fs::create_dir_all(&expert_dir).unwrap();
        let bin_path = expert_dir.join("expert_linux_amd64");
        fs::write(&bin_path, b"current binary").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("expert_linux_amd64", &exe_path).unwrap();
        #[cfg(not(unix))]
        fs::copy(&bin_path, &exe_path).unwrap();

        let status = remove_outdated_expert_install(&expert_dir, &exe_path, &logger).unwrap();
        assert!(matches!(status, OutdatedInstallStatus::Kept { .. }));
        assert!(exe_path.exists());

        // Broken symlink -> exists() is false -> NotPresent
        #[cfg(unix)]
        {
            fs::remove_file(&bin_path).unwrap();
            assert_eq!(
                remove_outdated_expert_install(&expert_dir, &exe_path, &logger).unwrap(),
                OutdatedInstallStatus::NotPresent
            );
        }
    }

    #[test]
    fn setup_runtime_dependencies_requires_elixir_and_prefers_path() {
        let tmp = TempDir::new("setup-path");
        let resources_dir = tmp.path.join("resources");
        let logger = RecordingLogger::default();
        let downloader = MockDownloader {
            payload: b"fake".to_vec(),
            final_url_override: None,
            calls: RefCell::new(Vec::new()),
        };

        let missing_elixir = MockElixirProbe(Ok(None));
        let err = setup_runtime_dependencies(
            &ElixirSettings::default(),
            &resources_dir,
            PlatformId::LinuxX64,
            None,
            &missing_elixir,
            &downloader,
            &logger,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), ELIXIR_NOT_INSTALLED_MESSAGE);

        let empty_elixir = MockElixirProbe(Ok(Some(String::new())));
        let err = setup_runtime_dependencies(
            &ElixirSettings::default(),
            &resources_dir,
            PlatformId::LinuxX64,
            None,
            &empty_elixir,
            &downloader,
            &logger,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), ELIXIR_NOT_INSTALLED_MESSAGE);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let bin_dir = tmp.path.join("bin");
            fs::create_dir_all(&bin_dir).unwrap();
            let expert_bin = bin_dir.join("expert");
            fs::write(&expert_bin, "#!/bin/sh\nexit 0\n").unwrap();
            fs::set_permissions(&expert_bin, fs::Permissions::from_mode(0o755)).unwrap();

            let probe = MockElixirProbe(Ok(Some("Elixir 1.17.2".to_string())));
            let found = setup_runtime_dependencies(
                &ElixirSettings::default(),
                &resources_dir,
                PlatformId::LinuxX64,
                Some(bin_dir.as_os_str()),
                &probe,
                &downloader,
                &logger,
            )
            .unwrap();
            assert_eq!(found, expert_bin);
            assert!(downloader.calls.borrow().is_empty());
        }
    }

    #[test]
    fn setup_runtime_dependencies_downloads_verifies_checksum_and_symlinks() {
        let tmp = TempDir::new("download");
        let resources_dir = tmp.path.join("resources");
        let logger = RecordingLogger::default();
        let probe = MockElixirProbe(Ok(Some("Elixir 1.17.2".to_string())));

        // Mismatched checksum on default version fails and leaves no leftover temp files.
        let bad_downloader = MockDownloader {
            payload: b"corrupted payload".to_vec(),
            final_url_override: None,
            calls: RefCell::new(Vec::new()),
        };
        let err = setup_runtime_dependencies(
            &ElixirSettings::default(),
            &resources_dir,
            PlatformId::LinuxX64,
            None,
            &probe,
            &bad_downloader,
            &logger,
        )
        .unwrap_err();
        assert!(matches!(err, ElixirToolsError::ChecksumMismatch { .. }));
        let expert_dir = resources_dir.join("expert");
        let entries: Vec<_> = fs::read_dir(&expert_dir).unwrap().collect();
        assert!(entries.is_empty(), "temp file should be cleaned up");

        // Disallowed redirect host fails and cleans up temp file.
        let bad_redirect = MockDownloader {
            payload: b"any".to_vec(),
            final_url_override: Some("https://evil.example.org/expert".to_string()),
            calls: RefCell::new(Vec::new()),
        };
        let err = setup_runtime_dependencies(
            &ElixirSettings {
                expert_version: Some("v0.1.11-custom".to_string()),
            },
            &resources_dir,
            PlatformId::LinuxX64,
            None,
            &probe,
            &bad_redirect,
            &logger,
        )
        .unwrap_err();
        assert!(matches!(err, ElixirToolsError::DisallowedHost { .. }));

        // Custom version succeeds without pinned sha256 and sets executable bit + symlink on Unix.
        let custom_downloader = MockDownloader {
            payload: b"#!/bin/sh\necho expert\n".to_vec(),
            final_url_override: Some(
                "https://release-assets.githubusercontent.com/expert_linux_amd64".to_string(),
            ),
            calls: RefCell::new(Vec::new()),
        };
        let exe_path = setup_runtime_dependencies(
            &ElixirSettings {
                expert_version: Some("v0.1.11-custom".to_string()),
            },
            &resources_dir,
            PlatformId::LinuxX64,
            None,
            &probe,
            &custom_downloader,
            &logger,
        )
        .unwrap();
        assert_eq!(exe_path, expert_dir.join("expert"));
        assert!(exe_path.exists());
        assert_eq!(
            sha256_hex(&fs::read(&exe_path).unwrap()),
            sha256_hex(b"#!/bin/sh\necho expert\n")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let link_target = fs::read_link(&exe_path).unwrap();
            assert_eq!(link_target, PathBuf::from("expert_linux_amd64"));
            let mode = fs::metadata(&exe_path).unwrap().permissions().mode();
            assert_ne!(mode & 0o111, 0);
        }
    }

    #[test]
    fn windows_pathext_candidates_follow_pathext_or_defaults() {
        let dir = Path::new("C:\\tools");
        assert_eq!(
            windows_candidate_binary_paths(dir, "expert", None),
            vec![
                dir.join("expert.com"),
                dir.join("expert.exe"),
                dir.join("expert.bat"),
                dir.join("expert.cmd"),
            ]
        );
        assert_eq!(
            windows_candidate_binary_paths(dir, "expert", Some(OsStr::new(".EXE;.BAT"))),
            vec![dir.join("expert.exe"), dir.join("expert.bat")]
        );
    }

    #[test]
    fn setup_reuses_existing_install_and_replaces_broken_symlink() {
        let tmp = TempDir::new("reuse-and-broken-link");
        let resources_dir = tmp.path.join("resources");
        let expert_dir = resources_dir.join("expert");
        fs::create_dir_all(&expert_dir).unwrap();
        let logger = RecordingLogger::default();
        let probe = MockElixirProbe(Ok(Some("Elixir 1.18.0".to_string())));

        #[cfg(unix)]
        {
            let exe_path = expert_dir.join("expert");
            std::os::unix::fs::symlink("missing_target", &exe_path).unwrap();

            let downloader = MockDownloader {
                payload: b"#!/bin/sh\nexit 0\n".to_vec(),
                final_url_override: None,
                calls: RefCell::new(Vec::new()),
            };

            let resolved = setup_runtime_dependencies(
                &ElixirSettings {
                    expert_version: Some("v0.1.11-test".to_string()),
                },
                &resources_dir,
                PlatformId::LinuxX64,
                None,
                &probe,
                &downloader,
                &logger,
            )
            .unwrap();
            assert_eq!(resolved, exe_path);
            assert_eq!(downloader.calls.borrow().len(), 1);

            // Second call reuses the existing binary without downloading again.
            let resolved_again = setup_runtime_dependencies(
                &ElixirSettings {
                    expert_version: Some("v0.1.11-test".to_string()),
                },
                &resources_dir,
                PlatformId::LinuxX64,
                None,
                &probe,
                &downloader,
                &logger,
            )
            .unwrap();
            assert_eq!(resolved_again, exe_path);
            assert_eq!(downloader.calls.borrow().len(), 1);
        }
    }
}
