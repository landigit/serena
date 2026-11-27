//! Behavioral parity tests comparing `elixir-tools` against the Python source
//! in `src/solidlsp/language_servers/elixir_tools/elixir_tools.py` and `src/solidlsp/ls.py`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use elixir_tools::{
    ALWAYS_IGNORED_DIRS, BinaryDownloader, DEFAULT_READY_TIMEOUT_SECS,
    DEFAULT_REQUEST_TIMEOUT_SECS, DEFAULT_WAIT_TIME_FOR_CROSS_FILE_REFERENCING_SECS,
    DOCUMENT_SYMBOLS_CACHE_FINGERPRINT, DownloadOutcome, ELIXIR_IGNORED_DIRS,
    ELIXIR_NOT_INSTALLED_MESSAGE, EXPERT_ALLOWED_HOSTS, EXPERT_OUTDATED_SHA256S, EXPERT_VERSION,
    ElixirSettings, ElixirTools, ElixirToolsError, ElixirVersionProbe, LogLevel, LogSink,
    NullLogSink, OutdatedInstallStatus, PlatformId, RuntimeSetupContext,
    create_base_initialize_params, is_ignored_dirname, normalize_symbol_name, path_to_file_uri,
    remove_outdated_expert_install_with_hashes, runtime_dependency_for_platform, sha256,
    write_downloaded_bytes,
};
use serde_json::Value;

static SEQ: AtomicU64 = AtomicU64::new(1);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "elixir-tools-parity-{prefix}-{}-{unique}-{seq}",
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn run_python_json(script: &str, args: &[&str]) -> Value {
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .args(args)
        .current_dir(repo_root())
        .output()
        .expect("python3 must be available for parity tests");
    assert!(
        output.status.success(),
        "python3 parity helper failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("python3 output must be valid JSON")
}

#[test]
fn constants_and_runtime_dependencies_match_python_source() {
    let py = run_python_json(
        r#"
import ast, json
from pathlib import Path

src = Path("src/solidlsp/language_servers/elixir_tools/elixir_tools.py").read_text(encoding="utf-8")
tree = ast.parse(src)

ns = {}
for node in tree.body:
    if isinstance(node, ast.Assign):
        for t in node.targets:
            if isinstance(t, ast.Name) and t.id in ("EXPERT_VERSION", "EXPERT_ALLOWED_HOSTS", "EXPERT_OUTDATED_SHA256S"):
                ns[t.id] = eval(compile(ast.Expression(node.value), "<ast>", "eval"), ns)

class FakePlatformId:
    LINUX_x64 = "linux-x64"
    LINUX_arm64 = "linux-arm64"
    OSX_x64 = "osx-x64"
    OSX_arm64 = "osx-arm64"
    WIN_x64 = "win-x64"

def extract_runtime_deps(expert_version):
    cls_node = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "ElixirTools")
    fn_node = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "_setup_runtime_dependencies")
    dict_node = next(
        n.value for n in fn_node.body
        if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "runtime_deps" for t in n.targets)
    )
    def make_dep(**kwargs):
        return kwargs
    env = {
        "PlatformId": FakePlatformId,
        "RuntimeDependency": make_dep,
        "EXPERT_VERSION": ns["EXPERT_VERSION"],
        "EXPERT_ALLOWED_HOSTS": ns["EXPERT_ALLOWED_HOSTS"],
        "expert_version": expert_version,
    }
    return eval(compile(ast.Expression(dict_node), "<ast>", "eval"), env)

ls_src = Path("src/solidlsp/ls.py").read_text(encoding="utf-8")
ls_tree = ast.parse(ls_src)
ls_cls = next(n for n in ls_tree.body if isinstance(n, ast.ClassDef) and n.name == "SolidLanguageServer")
always_ignored = None
for n in ls_cls.body:
    if isinstance(n, ast.Assign):
        for t in n.targets:
            if isinstance(t, ast.Name) and t.id == "_ALWAYS_IGNORED_DIRS":
                always_ignored = sorted(eval(compile(ast.Expression(n.value), "<ast>", "eval")))

cls_node = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "ElixirTools")
is_ignored_fn = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "is_ignored_dirname")
elixir_ignored = sorted(ast.literal_eval(is_ignored_fn.body[0].value.values[1].comparators[0]))

wait_fn = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "_get_wait_time_for_cross_file_referencing")
wait_secs = ast.literal_eval(wait_fn.body[0].value)

fp_fn = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "_document_symbols_cache_fingerprint")
fp_val = ast.literal_eval(fp_fn.body[0].value)

init_fn = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "__init__")
req_timeout = next(
    ast.literal_eval(stmt.value.args[0])
    for stmt in init_fn.body
    if isinstance(stmt, ast.Expr)
    and isinstance(stmt.value, ast.Call)
    and isinstance(stmt.value.func, ast.Attribute)
    and stmt.value.func.attr == "set_request_timeout"
)

ready_fn = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "_start_server")
ready_timeout = next(
    ast.literal_eval(stmt.value)
    for stmt in ready_fn.body
    if isinstance(stmt, ast.Assign)
    and any(isinstance(t, ast.Name) and t.id == "ready_timeout" for t in stmt.targets)
)

print(json.dumps({
    "expert_version": ns["EXPERT_VERSION"],
    "allowed_hosts": list(ns["EXPERT_ALLOWED_HOSTS"]),
    "outdated_sha256s": sorted(ns["EXPERT_OUTDATED_SHA256S"]),
    "always_ignored_dirs": always_ignored,
    "elixir_ignored_dirs": elixir_ignored,
    "wait_time_for_cross_file_referencing": wait_secs,
    "document_symbols_cache_fingerprint": fp_val,
    "request_timeout": req_timeout,
    "ready_timeout": ready_timeout,
    "deps_default": extract_runtime_deps(ns["EXPERT_VERSION"]),
    "deps_custom": extract_runtime_deps("v0.2.0-rc.1"),
}))
"#,
        &[],
    );

    assert_eq!(EXPERT_VERSION, py["expert_version"].as_str().unwrap());

    let py_hosts: Vec<&str> = py["allowed_hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(EXPERT_ALLOWED_HOSTS, py_hosts.as_slice());

    let py_outdated: BTreeSet<&str> = py["outdated_sha256s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let rs_outdated: BTreeSet<&str> = EXPERT_OUTDATED_SHA256S.iter().copied().collect();
    assert_eq!(rs_outdated, py_outdated);

    let py_always: BTreeSet<&str> = py["always_ignored_dirs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let rs_always: BTreeSet<&str> = ALWAYS_IGNORED_DIRS.iter().copied().collect();
    assert_eq!(rs_always, py_always);

    let py_elixir_ignored: BTreeSet<&str> = py["elixir_ignored_dirs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let rs_elixir_ignored: BTreeSet<&str> = ELIXIR_IGNORED_DIRS.iter().copied().collect();
    assert_eq!(rs_elixir_ignored, py_elixir_ignored);
    for d in py_always.iter().chain(py_elixir_ignored.iter()) {
        assert!(is_ignored_dirname(d), "{d} should be ignored");
    }
    assert!(!is_ignored_dirname("lib"));

    assert_eq!(
        DEFAULT_WAIT_TIME_FOR_CROSS_FILE_REFERENCING_SECS,
        py["wait_time_for_cross_file_referencing"].as_f64().unwrap()
    );
    assert_eq!(
        DOCUMENT_SYMBOLS_CACHE_FINGERPRINT as u64,
        py["document_symbols_cache_fingerprint"].as_u64().unwrap()
    );
    assert_eq!(
        DEFAULT_REQUEST_TIMEOUT_SECS,
        py["request_timeout"].as_f64().unwrap()
    );
    assert_eq!(
        DEFAULT_READY_TIMEOUT_SECS,
        py["ready_timeout"].as_f64().unwrap()
    );

    let platforms = [
        PlatformId::LinuxX64,
        PlatformId::LinuxArm64,
        PlatformId::OsxX64,
        PlatformId::OsxArm64,
        PlatformId::WinX64,
    ];
    for (label, version, key) in [
        ("default", EXPERT_VERSION, "deps_default"),
        ("custom", "v0.2.0-rc.1", "deps_custom"),
    ] {
        for platform in platforms {
            let py_dep = &py[key][platform.as_str()];
            let rs_dep = runtime_dependency_for_platform(platform, version).unwrap();
            assert_eq!(
                rs_dep.id,
                py_dep["id"].as_str().unwrap(),
                "{label} {platform} id"
            );
            assert_eq!(
                rs_dep.platform_id,
                py_dep["platform_id"].as_str().unwrap(),
                "{label} {platform} platform_id"
            );
            assert_eq!(
                rs_dep.url,
                py_dep["url"].as_str().unwrap(),
                "{label} {platform} url"
            );
            assert_eq!(
                rs_dep.archive_type,
                py_dep["archive_type"].as_str().unwrap(),
                "{label} {platform} archive_type"
            );
            assert_eq!(
                rs_dep.binary_name,
                py_dep["binary_name"].as_str().unwrap(),
                "{label} {platform} binary_name"
            );
            assert_eq!(
                rs_dep.extract_path,
                py_dep["extract_path"].as_str().unwrap(),
                "{label} {platform} extract_path"
            );
            assert_eq!(
                rs_dep.sha256.as_deref(),
                py_dep["sha256"].as_str(),
                "{label} {platform} sha256"
            );
            let py_dep_hosts: Vec<&str> = py_dep["allowed_hosts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            let rs_dep_hosts: Vec<&str> = rs_dep.allowed_hosts.iter().map(String::as_str).collect();
            assert_eq!(rs_dep_hosts, py_dep_hosts);
        }
    }
}

#[test]
fn initialize_params_match_python_exactly() {
    let py_params = run_python_json(
        r#"
import ast, json
from pathlib import Path

src = Path("src/solidlsp/language_servers/elixir_tools/elixir_tools.py").read_text(encoding="utf-8")
tree = ast.parse(src)
cls_node = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "ElixirTools")
fn_node = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "_create_base_initialize_params")
assign_node = next(
    n for n in fn_node.body
    if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "initialize_params" for t in n.targets)
)
val = eval(compile(ast.Expression(assign_node.value), "<ast>", "eval"))
print(json.dumps(val))
"#,
        &[],
    );

    assert_eq!(create_base_initialize_params(), py_params);
}

#[test]
fn normalize_symbol_name_matches_python_across_corpus() {
    let cases = [
        "",
        "   ",
        "User",
        "TestRepo.Models",
        "def new()",
        "def new(attrs \\\\ %{})",
        "def create_user(pid, attrs)",
        "defp helper(x)",
        "defp helper_no_args",
        "def   spaced_fn(a, b)",
        "  def prefixed_space(a)",
        "defp def nested(a, b)",
        "def def double_def(a)",
        "defp defp double_defp(a)",
        "def foo(a, (b, c))",
        "test \"creates a user\"(context)",
        "describe \"User\"(context)",
        "@type t",
        "%User{name: nil}",
        "def with_unicode_ws(a)\u{00a0}",
    ];

    let serialized_cases = serde_json::to_string(&cases).unwrap();
    let py_results = run_python_json(
        r#"
import ast, json, sys
from pathlib import Path

src = Path("src/solidlsp/language_servers/elixir_tools/elixir_tools.py").read_text(encoding="utf-8")
tree = ast.parse(src)
cls_node = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "ElixirTools")
fn_node = next(n for n in cls_node.body if isinstance(n, ast.FunctionDef) and n.name == "_normalize_symbol_name")
ret_expr = next(n.value for n in fn_node.body if isinstance(n, ast.Return))
code = compile(ast.Expression(ret_expr), "<ast>", "eval")

cases = json.loads(sys.argv[1])
out = [eval(code, {"symbol": {"name": c}}) for c in cases]
print(json.dumps(out))
"#,
        &[&serialized_cases],
    );

    for (raw, py_val) in cases.iter().zip(py_results.as_array().unwrap()) {
        assert_eq!(
            normalize_symbol_name(raw),
            py_val.as_str().unwrap(),
            "mismatch for {raw:?}"
        );
    }
}

#[test]
fn sha256_and_outdated_install_cleanup_and_uris_match_python() {
    let tmp = TempDir::new("sha-uri");
    let expert_dir = tmp.path.join("expert");
    fs::create_dir_all(&expert_dir).unwrap();
    let binary_path = expert_dir.join("expert_linux_amd64");
    let exe_path = expert_dir.join("expert");
    let bytes = b"fake-expert-v0.1.0-rc.6-binary\x00\xffpayload";
    fs::write(&binary_path, bytes).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("expert_linux_amd64", &exe_path).unwrap();
    #[cfg(not(unix))]
    fs::copy(&binary_path, &exe_path).unwrap();

    let rs_hash = sha256::calculate_file_sha256(&binary_path).unwrap();
    let py_info = run_python_json(
        r#"
import hashlib, json, sys
from pathlib import Path

p = Path(sys.argv[1])
h = hashlib.sha256(p.read_bytes()).hexdigest()
print(json.dumps({"sha256": h, "uri": p.as_uri()}))
"#,
        &[binary_path.to_str().unwrap()],
    );

    assert_eq!(rs_hash, py_info["sha256"].as_str().unwrap());
    assert_eq!(
        path_to_file_uri(&binary_path).unwrap(),
        py_info["uri"].as_str().unwrap()
    );

    let spaced = tmp.path.join("my dir").join("mix.exs");
    fs::create_dir_all(spaced.parent().unwrap()).unwrap();
    fs::write(&spaced, "").unwrap();
    let py_spaced_uri = run_python_json(
        "import json, sys; from pathlib import Path; print(json.dumps(Path(sys.argv[1]).as_uri()))",
        &[spaced.to_str().unwrap()],
    );
    assert_eq!(
        path_to_file_uri(&spaced).unwrap(),
        py_spaced_uri.as_str().unwrap()
    );

    // Real SHA-256 + symlink canonicalize + dir removal test.
    let status = remove_outdated_expert_install_with_hashes(
        &expert_dir,
        &exe_path,
        &[rs_hash.as_str()],
        &NullLogSink,
    )
    .unwrap();
    assert_eq!(status, OutdatedInstallStatus::Removed { sha256: rs_hash });
    assert!(!expert_dir.exists());
}

#[test]
fn ignore_rules_and_facade_end_to_end_work_offline() {
    let tmp = TempDir::new("e2e");
    let repo = tmp.path.join("repo");
    fs::create_dir_all(repo.join("lib")).unwrap();
    fs::create_dir_all(repo.join("test")).unwrap();
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::create_dir_all(repo.join("ignored_dir")).unwrap();
    fs::create_dir_all(repo.join("_build/dev")).unwrap();

    let mix_exs = repo.join("mix.exs");
    fs::write(&mix_exs, "defmodule Repo.MixProject do\nend\n").unwrap();
    fs::write(repo.join("lib/models.ex"), "defmodule Models do\nend\n").unwrap();
    fs::write(
        repo.join("test/models_test.exs"),
        "defmodule ModelsTest do\nend\n",
    )
    .unwrap();
    fs::write(repo.join("scripts/seed.exs"), "IO.puts(:hi)\n").unwrap();
    fs::write(repo.join("ignored_dir/x.ex"), "defmodule X do\nend\n").unwrap();
    fs::write(repo.join("lib/README.md"), "# docs\n").unwrap();

    struct Probe;
    impl ElixirVersionProbe for Probe {
        fn get_elixir_version(&self) -> Result<Option<String>, ElixirToolsError> {
            Ok(Some("Elixir 1.18.1".to_string()))
        }
    }

    struct LocalMirrorDownloader;
    impl BinaryDownloader for LocalMirrorDownloader {
        fn download(
            &self,
            url: &str,
            dest_path: &Path,
        ) -> Result<DownloadOutcome, ElixirToolsError> {
            write_downloaded_bytes(dest_path, b"#!/bin/sh\nexit 0\n")?;
            Ok(DownloadOutcome {
                final_url: url.to_string(),
            })
        }
    }

    #[derive(Default)]
    struct CaptureLog(RefCell<Vec<(LogLevel, String)>>);
    impl LogSink for CaptureLog {
        fn log(&self, level: LogLevel, message: &str) {
            self.0.borrow_mut().push((level, message.to_string()));
        }
    }

    let resources_dir = tmp.path.join("resources");
    let logger = CaptureLog::default();
    let tools = ElixirTools::with_runtime_setup(
        &repo,
        ["scripts", "ignored_*"],
        RuntimeSetupContext {
            ls_resources_dir: &resources_dir,
            platform: PlatformId::LinuxX64,
            path_env: None,
            settings: ElixirSettings {
                expert_version: Some("v0.1.10-test".to_string()),
            },
            elixir_probe: &Probe,
            downloader: &LocalMirrorDownloader,
            logger: &logger,
        },
    )
    .unwrap();

    assert_eq!(tools.language_id(), "elixir");
    assert_eq!(tools.request_timeout(), Duration::from_secs(180));
    assert_eq!(
        tools.wait_time_for_cross_file_referencing(),
        Duration::from_secs(10)
    );
    assert_eq!(tools.document_symbols_cache_fingerprint(), 1);
    assert!(
        tools
            .process_launch_info()
            .command_string()
            .ends_with("expert --stdio")
    );

    assert!(tools.is_ignored_dirname("_build"));
    assert!(tools.is_ignored_dirname(".expert"));
    assert!(!tools.is_ignored_dirname("lib"));

    assert!(tools.is_ignored_path("mix.exs", true, true));
    assert!(tools.is_ignored_path("mix.exs", false, false));
    assert!(!tools.is_ignored_path("lib/models.ex", true, true));
    assert!(!tools.is_ignored_path("test/models_test.exs", true, true));
    assert!(tools.is_ignored_path("scripts/seed.exs", true, true));
    assert!(tools.is_ignored_path("ignored_dir/x.ex", true, true));
    assert!(tools.is_ignored_path("_build/dev/foo.ex", true, true));
    assert!(tools.is_ignored_path("lib/README.md", true, true));
    assert!(!tools.is_ignored_path("lib/README.md", false, true));
    // Non-existent dotfile / trailing-dot parity with Python Path.suffix:
    assert!(!tools.is_ignored_path(".ex", true, true));
    assert!(tools.is_ignored_path("foo.", true, true));

    assert_eq!(tools.find_mix_exs(&logger), Some(mix_exs));

    // Drive ElixirTools::start_handshake against a mock stdio server.
    let mock_server = tmp.path.join("mock.py");
    fs::write(
        &mock_server,
        r#"import json, sys
def read_msg():
    n = None
    while True:
        l = sys.stdin.buffer.readline()
        if not l: return None
        s = l.rstrip(b"\r\n")
        if not s: break
        if b":" in s:
            k, v = s.split(b":", 1)
            if k.strip().lower() == b"content-length": n = int(v.strip())
    return json.loads(sys.stdin.buffer.read(n).decode())
def write_msg(o):
    b = json.dumps(o).encode()
    sys.stdout.buffer.write(f"Content-Length: {len(b)}\r\n\r\n".encode() + b)
    sys.stdout.buffer.flush()

while True:
    m = read_msg()
    if m is None: break
    if m.get("method") == "initialize":
        assert m["params"] == json.loads(sys.argv[1])
        write_msg({"jsonrpc": "2.0", "id": m["id"], "result": {"capabilities": {"textDocumentSync": 1}}})
    elif m.get("method") == "textDocument/didOpen":
        assert m["params"]["textDocument"]["languageId"] == "elixir"
        write_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "1", "value": {"kind": "begin", "title": "Building repo"}}})
        write_msg({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "1", "value": {"kind": "end"}}})
    elif m.get("method") == "textDocument/didClose":
        break
"#,
    )
    .unwrap();

    let expected_init_json = serde_json::to_string(&tools.create_base_initialize_params()).unwrap();
    let mut child = Command::new("python3")
        .arg(&mock_server)
        .arg(&expected_init_json)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let startup = tools
        .start_handshake(&mut stdin, stdout, Duration::from_secs(5), &logger)
        .unwrap();
    assert!(startup.signaled_ready);
    assert!(startup.opened_mix_exs_uri.is_some());
    drop(stdin);
    assert!(child.wait().unwrap().success());

    // Sanity check the Python Elixir-not-installed message constant.
    assert!(ELIXIR_NOT_INSTALLED_MESSAGE.contains("https://elixir-lang.org/install.html"));
}
