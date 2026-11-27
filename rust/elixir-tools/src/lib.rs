//! Rust port of `src/solidlsp/language_servers/elixir_tools/` (`ElixirTools` / Expert).
//!
//! Provides Elixir language-server runtime dependency management, Expert outdated-binary cleanup,
//! ignore rules, symbol name normalization, `mix.exs` discovery, LSP `initialize` parameter
//! generation, and JSON-RPC stdio startup handshake tracking.

#![forbid(unsafe_code)]

pub mod ignore;
pub mod lsp;
pub mod runtime;
pub mod sha256;

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

pub use ignore::{
    ALWAYS_IGNORED_DIRS, ELIXIR_IGNORED_DIRS, IgnoreSpec, SOURCE_EXTENSIONS, is_ignored_dirname,
    is_ignored_path, is_relevant_elixir_filename, normalize_symbol_name,
};
pub use lsp::{
    DEFAULT_READY_TIMEOUT_SECS, DEFAULT_REQUEST_TIMEOUT_SECS,
    DEFAULT_WAIT_TIME_FOR_CROSS_FILE_REFERENCING_SECS, DOCUMENT_SYMBOLS_CACHE_FINGERPRINT,
    ExpertMessageReader, ProcessLaunchInfo, ProgressTracker, StartupResult,
    classify_window_log_message, create_base_initialize_params, find_mix_exs, path_to_file_uri,
    read_lsp_message, run_expert_startup_handshake, write_lsp_message,
};
pub use runtime::{
    BinaryDownloader, DownloadOutcome, ELIXIR_NOT_INSTALLED_MESSAGE, EXPERT_ALLOWED_HOSTS,
    EXPERT_OUTDATED_SHA256S, EXPERT_VERSION, ElixirSettings, ElixirToolsError, ElixirVersionProbe,
    LogLevel, LogSink, NullLogSink, OutdatedInstallStatus, PlatformId, RuntimeDependency,
    SystemElixirVersionProbe, find_executable_in_path, remove_outdated_expert_install,
    remove_outdated_expert_install_with_hashes, runtime_dependency_for_platform,
    setup_runtime_dependencies, validate_download_host, write_downloaded_bytes,
};

/// Dependencies injected when resolving Expert runtime assets via [`ElixirTools::with_runtime_setup`].
pub struct RuntimeSetupContext<'a> {
    pub ls_resources_dir: &'a Path,
    pub platform: PlatformId,
    pub path_env: Option<&'a OsStr>,
    pub settings: ElixirSettings,
    pub elixir_probe: &'a dyn ElixirVersionProbe,
    pub downloader: &'a dyn BinaryDownloader,
    pub logger: &'a dyn LogSink,
}

/// High-level Rust counterpart to `solidlsp.language_servers.elixir_tools.ElixirTools`.
#[derive(Debug, Clone)]
pub struct ElixirTools {
    repository_root: PathBuf,
    expert_executable_path: PathBuf,
    settings: ElixirSettings,
    ignore_spec: IgnoreSpec,
    request_timeout: Duration,
}

impl ElixirTools {
    /// Constructs an [`ElixirTools`] instance with a pre-resolved Expert executable path.
    pub fn new<I, S>(
        repository_root: impl Into<PathBuf>,
        expert_executable_path: impl Into<PathBuf>,
        settings: ElixirSettings,
        ignored_paths: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self {
            repository_root: repository_root.into(),
            expert_executable_path: expert_executable_path.into(),
            settings,
            ignore_spec: IgnoreSpec::from_patterns(ignored_paths),
            request_timeout: Duration::from_secs_f64(DEFAULT_REQUEST_TIMEOUT_SECS),
        }
    }

    /// Resolves runtime dependencies and constructs an [`ElixirTools`] instance.
    pub fn with_runtime_setup<I, S>(
        repository_root: impl Into<PathBuf>,
        ignored_paths: I,
        context: RuntimeSetupContext<'_>,
    ) -> Result<Self, ElixirToolsError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let expert_executable_path = setup_runtime_dependencies(
            &context.settings,
            context.ls_resources_dir,
            context.platform,
            context.path_env,
            context.elixir_probe,
            context.downloader,
            context.logger,
        )?;
        Ok(Self::new(
            repository_root,
            expert_executable_path,
            context.settings,
            ignored_paths,
        ))
    }

    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    pub fn expert_executable_path(&self) -> &Path {
        &self.expert_executable_path
    }

    pub fn settings(&self) -> &ElixirSettings {
        &self.settings
    }

    pub fn language_id(&self) -> &'static str {
        "elixir"
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn set_request_timeout(&mut self, timeout: Duration) {
        self.request_timeout = timeout;
    }

    pub fn wait_time_for_cross_file_referencing(&self) -> Duration {
        Duration::from_secs_f64(DEFAULT_WAIT_TIME_FOR_CROSS_FILE_REFERENCING_SECS)
    }

    pub fn document_symbols_cache_fingerprint(&self) -> u32 {
        DOCUMENT_SYMBOLS_CACHE_FINGERPRINT
    }

    pub fn process_launch_info(&self) -> ProcessLaunchInfo {
        ProcessLaunchInfo::for_expert(&self.expert_executable_path, &self.repository_root)
    }

    pub fn is_ignored_dirname(&self, dirname: &str) -> bool {
        is_ignored_dirname(dirname)
    }

    pub fn is_ignored_path(
        &self,
        relative_path: &str,
        ignore_unsupported_files: bool,
        check_path_for_ignored_dirnames: bool,
    ) -> bool {
        is_ignored_path(
            relative_path,
            &self.repository_root,
            ignore_unsupported_files,
            check_path_for_ignored_dirnames,
            &self.ignore_spec,
        )
    }

    pub fn normalize_symbol_name(&self, raw_name: &str) -> String {
        normalize_symbol_name(raw_name)
    }

    pub fn find_mix_exs(&self, logger: &dyn LogSink) -> Option<PathBuf> {
        find_mix_exs(&self.repository_root, logger)
    }

    pub fn create_base_initialize_params(&self) -> Value {
        create_base_initialize_params()
    }

    pub fn start_handshake<W, R>(
        &self,
        writer: &mut W,
        reader: R,
        ready_timeout: Duration,
        logger: &dyn LogSink,
    ) -> Result<StartupResult, ElixirToolsError>
    where
        W: Write,
        R: Read + Send + 'static,
    {
        run_expert_startup_handshake(writer, reader, &self.repository_root, ready_timeout, logger)
    }
}
