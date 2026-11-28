use std::path::Path;

use codemode_core::languages::LanguageId;
use codemode_ts::{
    TsError, best_match, chunk_file, find_symbols, flatten, parse_file, replace_symbol_body,
};

fn fixture(dir: &Path, name: &str, contents: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn parses_existing_and_newly_added_tree_sitter_grammars() {
    let dir = std::env::temp_dir().join(format!("codemode-ts-grammars-{}", std::process::id()));

    let samples: &[(&str, &str, &str)] = &[
        (
            "app.tsx",
            "export function App() {\n  const [mode, setMode] = useState('v');\n  const handleClick = () => setMode('e');\n  return <div onClick={handleClick}>{mode}</div>;\n}\n",
            "App/handleClick",
        ),
        (
            "lib.rs",
            "pub struct Engine;\nimpl Engine {\n  pub fn run(&self) {}\n}\n",
            "Engine[1]/run",
        ),
        (
            "mod.py",
            "class Store:\n    def add(self, x):\n        return x\n",
            "Store/add",
        ),
        (
            "calc.jl",
            "module Calc\nfunction add(a, b)\n  return a + b\nend\nend\n",
            "Calc/add",
        ),
        (
            "build.sh",
            "#!/usr/bin/env bash\nbuild_all() {\n  echo ok\n}\n",
            "build_all",
        ),
        (
            "init.lua",
            "local function greet(name)\n  return 'hi ' .. name\nend\n",
            "greet",
        ),
        (
            "main.zig",
            "const std = @import(\"std\");\npub fn add(a: i32, b: i32) i32 {\n    return a + b;\n}\n",
            "add",
        ),
        (
            "greeter.ex",
            "defmodule Greeter do\n  def hello(name) do\n    name\n  end\nend\n",
            "Greeter/hello",
        ),
        ("cfg.yaml", "server:\n  port: 8080\n", "server/port"),
        ("Cargo.toml", "[package]\nname = \"demo\"\n", "package/name"),
        ("README.md", "# Overview\n\n## Usage\n", "Overview/Usage"),
        (
            "index.html",
            "<html><body><main id=\"app\"></main></body></html>\n",
            "html/body",
        ),
        ("styles.css", ".card { color: red; }\n", ".card"),
        (
            "data.json",
            "{\"service\": {\"port\": 8080}}\n",
            "service/port",
        ),
        (
            "Main.lean",
            "namespace Math\ndef idNat (n : Nat) : Nat := n\nend Math\n",
            "Math/idNat",
        ),
        (
            "App.scala",
            "object App {\n  def run(): Unit = {}\n}\n",
            "App/run",
        ),
        ("App.swift", "struct App {\n  func run() {}\n}\n", "App/run"),
        (
            "Main.hs",
            "add :: Int -> Int -> Int\nadd a b = a + b\n",
            "add",
        ),
        (
            "app.dart",
            "class App {\n  int run() { return 1; }\n}\n",
            "App/run",
        ),
    ];

    for (file_name, src, expected_symbol) in samples {
        let path = fixture(&dir, file_name, src);
        let parsed =
            parse_file(&path, None).unwrap_or_else(|e| panic!("failed to parse {file_name}: {e}"));
        let names: Vec<&str> = flatten(&parsed.symbols)
            .into_iter()
            .map(|s| s.name_path.as_str())
            .collect();
        assert!(
            names.contains(expected_symbol),
            "{file_name}: expected {expected_symbol} in {names:?}"
        );
    }
}

#[test]
fn returns_unsupported_grammar_for_lsp_only_languages() {
    let dir = std::env::temp_dir().join(format!("codemode-ts-lsp-only-{}", std::process::id()));
    let path = fixture(&dir, "main.cr", "def hello\n  \"hi\"\nend\n");
    let err = parse_file(&path, None).unwrap_err();
    assert!(
        matches!(err, TsError::UnsupportedGrammar(LanguageId::Crystal)),
        "unexpected error: {err:?}"
    );
}

#[test]
fn preserves_overload_indices_and_replaces_symbol_body() {
    let dir = std::env::temp_dir().join(format!("codemode-ts-overloads-{}", std::process::id()));
    let java = fixture(
        &dir,
        "Calc.java",
        "class Calc {\n  int add(int a) { return a; }\n  double add(double a) { return a; }\n  void clear() {}\n}\n",
    );
    let parsed = parse_file(&java, None).unwrap();
    let paths: Vec<&str> = flatten(&parsed.symbols)
        .into_iter()
        .map(|s| s.name_path.as_str())
        .collect();
    assert_eq!(
        paths,
        vec!["Calc", "Calc/add[0]", "Calc/add[1]", "Calc/clear"]
    );
    assert_eq!(best_match(&parsed, "Calc/add[1]", false).unwrap().line, 3);
    assert_eq!(find_symbols(&parsed, "Calc/add", false).len(), 2);
    assert!(best_match(&parsed, "Calc/add[2]", false).is_none());

    let outcome = replace_symbol_body(
        &java,
        "Calc/add[1]",
        "double add(double a) { return a * 2.0; }",
        true,
        None,
    )
    .unwrap();
    assert!(!outcome.changed);
    assert!(
        outcome
            .diff
            .contains("-  double add(double a) { return a; }")
    );
    assert!(
        outcome
            .diff
            .contains("+  double add(double a) { return a * 2.0; }")
    );
}

#[test]
fn chunks_long_files_along_symbol_boundaries_with_line_units() {
    let dir = std::env::temp_dir().join(format!("codemode-ts-chunk-{}", std::process::id()));
    let mut src = String::new();
    for idx in 0..8 {
        src.push_str(&format!(
            "fn func_{idx}() -> i32 {{\n    let x = {idx};\n    x + 1\n}}\n\n"
        ));
    }
    let path = fixture(&dir, "many.rs", &src);
    let chunks = chunk_file(&path, &src, 120, None);
    assert!(
        chunks.len() >= 4,
        "expected multiple symbol-aligned chunks, got {}",
        chunks.len()
    );
    assert_eq!(chunks.first().unwrap().start_line, 1);
    let total_lines = src.trim_end().lines().count();
    assert_eq!(chunks.last().unwrap().end_line, total_lines);
    assert!(
        chunks
            .iter()
            .any(|c| c.symbol_path.as_deref() == Some("func_0"))
    );
    for pair in chunks.windows(2) {
        assert!(pair[0].end_line < pair[1].start_line);
    }
}
