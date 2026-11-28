//! Tree-sitter grammar lookup for supported `LanguageId` values.

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
            "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
            "js" | "jsx" | "mjs" | "cjs" => tree_sitter_javascript::LANGUAGE.into(),
            _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        },
        LanguageId::Angular => match ext.as_str() {
            "html" | "htm" => arborium_html::language().into(),
            "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
            _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        },
        LanguageId::Rust => tree_sitter_rust::LANGUAGE.into(),
        LanguageId::Python => tree_sitter_python::LANGUAGE.into(),
        LanguageId::Go => tree_sitter_go::LANGUAGE.into(),
        LanguageId::Java => tree_sitter_java::LANGUAGE.into(),
        LanguageId::Cpp => match ext.as_str() {
            "c" | "h" => tree_sitter_c::LANGUAGE.into(),
            _ => tree_sitter_cpp::LANGUAGE.into(),
        },
        LanguageId::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        LanguageId::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        LanguageId::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        LanguageId::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
        LanguageId::Lean4 => arborium_lean::language().into(),
        LanguageId::Bash => arborium_bash::language().into(),
        LanguageId::Julia => arborium_julia::language().into(),
        LanguageId::Lua => arborium_lua::language().into(),
        LanguageId::Zig => arborium_zig::language().into(),
        LanguageId::Elixir => arborium_elixir::language().into(),
        LanguageId::Dart => arborium_dart::language().into(),
        LanguageId::Haskell => arborium_haskell::language().into(),
        LanguageId::Scala => arborium_scala::language().into(),
        LanguageId::Swift => arborium_swift::language().into(),
        LanguageId::Yaml | LanguageId::Ansible => arborium_yaml::language().into(),
        LanguageId::Toml => arborium_toml::language().into(),
        LanguageId::Markdown => arborium_markdown::language().into(),
        LanguageId::Html => arborium_html::language().into(),
        LanguageId::Scss => arborium_css::language().into(),
        LanguageId::Json => arborium_json::language().into(),
        _ => return None,
    };
    Some(language)
}
