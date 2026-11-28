//! Uniform `arborium-*` tree-sitter grammar lookup for `LanguageId` values.

use std::path::Path;

use codemode_core::languages::LanguageId;

/// Returns the tree-sitter `Language` for `(lang, path)`, or `None` for LSP-only languages.
pub fn grammar_for(lang: LanguageId, path: &Path) -> Option<tree_sitter::Language> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let language: tree_sitter::Language = match lang {
        LanguageId::TypeScript | LanguageId::Deno => match ext.as_str() {
            "tsx" => arborium_tsx::language().into(),
            "js" | "jsx" | "mjs" | "cjs" => arborium_javascript::language().into(),
            _ => arborium_typescript::language().into(),
        },
        LanguageId::Angular => match ext.as_str() {
            "html" | "htm" => arborium_html::language().into(),
            "tsx" => arborium_tsx::language().into(),
            _ => arborium_typescript::language().into(),
        },
        LanguageId::CSharp => arborium_c_sharp::language().into(),
        LanguageId::VbNet => match ext.as_str() {
            "cs" => arborium_c_sharp::language().into(),
            _ => arborium_vb::language().into(),
        },
        LanguageId::Python => arborium_python::language().into(),
        LanguageId::Rust => arborium_rust::language().into(),
        LanguageId::Java => arborium_java::language().into(),
        LanguageId::Kotlin => arborium_kotlin::language().into(),
        LanguageId::Go => arborium_go::language().into(),
        LanguageId::Ruby => arborium_ruby::language().into(),
        LanguageId::Dart => arborium_dart::language().into(),
        LanguageId::Cpp => match ext.as_str() {
            "c" | "h" => arborium_c::language().into(),
            _ => arborium_cpp::language().into(),
        },
        LanguageId::Php => arborium_php::language().into(),
        LanguageId::R => arborium_r::language().into(),
        LanguageId::Perl => arborium_perl::language().into(),
        LanguageId::Clojure => arborium_clojure::language().into(),
        LanguageId::Elixir => arborium_elixir::language().into(),
        LanguageId::Elm => arborium_elm::language().into(),
        LanguageId::Terraform => arborium_hcl::language().into(),
        LanguageId::Swift => arborium_swift::language().into(),
        LanguageId::Bash => arborium_bash::language().into(),
        LanguageId::Zig => arborium_zig::language().into(),
        LanguageId::Lua => arborium_lua::language().into(),
        LanguageId::Nix => arborium_nix::language().into(),
        LanguageId::Erlang => arborium_erlang::language().into(),
        LanguageId::Ocaml => arborium_ocaml::language().into(),
        LanguageId::FSharp => arborium_fsharp::language().into(),
        LanguageId::Rego => arborium_rego::language().into(),
        LanguageId::Scala => arborium_scala::language().into(),
        LanguageId::Julia => arborium_julia::language().into(),
        LanguageId::Haskell => arborium_haskell::language().into(),
        LanguageId::Lean4 => arborium_lean::language().into(),
        LanguageId::Groovy => arborium_groovy::language().into(),
        LanguageId::Vue => match ext.as_str() {
            "tsx" => arborium_tsx::language().into(),
            "ts" | "mts" | "cts" => arborium_typescript::language().into(),
            "js" | "jsx" | "mjs" | "cjs" => arborium_javascript::language().into(),
            _ => arborium_vue::language().into(),
        },
        LanguageId::Svelte => match ext.as_str() {
            "ts" | "mts" | "cts" => arborium_typescript::language().into(),
            "js" | "mjs" | "cjs" => arborium_javascript::language().into(),
            _ => arborium_svelte::language().into(),
        },
        LanguageId::PowerShell => arborium_powershell::language().into(),
        LanguageId::Matlab => arborium_matlab::language().into(),
        LanguageId::Ada => arborium_ada::language().into(),
        LanguageId::Gleam => arborium_gleam::language().into(),
        LanguageId::Markdown => arborium_markdown::language().into(),
        LanguageId::Yaml | LanguageId::Ansible => arborium_yaml::language().into(),
        LanguageId::Json => arborium_json::language().into(),
        LanguageId::Toml => arborium_toml::language().into(),
        LanguageId::Hlsl => arborium_hlsl::language().into(),
        LanguageId::SystemVerilog => arborium_verilog::language().into(),
        LanguageId::Solidity => arborium_solidity::language().into(),
        LanguageId::Html => arborium_html::language().into(),
        LanguageId::Scss => match ext.as_str() {
            "css" => arborium_css::language().into(),
            _ => arborium_scss::language().into(),
        },
        LanguageId::GraphQl => arborium_graphql::language().into(),
        _ => return None,
    };
    Some(language)
}
