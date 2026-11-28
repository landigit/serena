//! Structural symbol extraction and overload-aware Serena name-path matching.

use std::collections::HashMap;

use serde::Serialize;

/// Parsed `(name, overload_idx)` segment used by Serena name-path matching.
pub type PathComponent = (String, Option<i64>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct PatternComponent {
    name: String,
    overload_idx: Option<i64>,
}

impl PatternComponent {
    fn parse(text: &str) -> Self {
        if text.ends_with(']')
            && let Some(bracket) = text.rfind('[')
        {
            let index = &text[bracket + 1..text.len() - 1];
            if !index.is_empty()
                && index.bytes().all(|b| b.is_ascii_digit())
                && let Ok(overload_idx) = index.parse::<i64>()
            {
                return Self {
                    name: text[..bracket].to_string(),
                    overload_idx: Some(overload_idx),
                };
            }
        }
        Self {
            name: text.to_string(),
            overload_idx: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedPattern {
    absolute: bool,
    components: Vec<PatternComponent>,
}

/// Parses a Serena name-path pattern into components with optional `[i]` overload indices.
pub fn pattern_components(pattern: &str) -> Result<Vec<PathComponent>, String> {
    let parsed = parse_pattern(pattern)?;
    Ok(parsed
        .components
        .into_iter()
        .map(|c| (c.name, c.overload_idx))
        .collect())
}

fn parse_pattern(pattern: &str) -> Result<ParsedPattern, String> {
    if pattern.is_empty() {
        return Err("AssertionError: name_path must not be empty".to_string());
    }
    let body = pattern.trim_matches('/');
    Ok(ParsedPattern {
        absolute: pattern.starts_with('/'),
        components: body.split('/').map(PatternComponent::parse).collect(),
    })
}

/// Matches a Serena name-path pattern against a symbol's component path.
pub fn match_name_path(
    pattern: &str,
    symbol: &[PathComponent],
    substring_matching: bool,
) -> Result<bool, String> {
    let parsed = parse_pattern(pattern)?;
    Ok(matches_components(&parsed, symbol, substring_matching))
}

fn matches_components(
    parsed: &ParsedPattern,
    symbol: &[PathComponent],
    substring_matching: bool,
) -> bool {
    let pattern = &parsed.components;
    if pattern.is_empty() || pattern.len() > symbol.len() {
        return false;
    }
    if parsed.absolute && pattern.len() != symbol.len() {
        return false;
    }
    let offset = symbol.len() - pattern.len();
    for (idx, (pat, (sym_name, sym_idx))) in pattern.iter().zip(symbol[offset..].iter()).enumerate()
    {
        let is_last = idx + 1 == pattern.len();
        let name_ok = if substring_matching && is_last {
            sym_name.contains(pat.name.as_str())
        } else {
            sym_name == &pat.name
        };
        if !name_ok {
            return false;
        }
        if let Some(wanted) = pat.overload_idx
            && *sym_idx != Some(wanted)
        {
            return false;
        }
    }
    true
}

/// A structural symbol extracted from tree-sitter or LSP `documentSymbol`.
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub name_path: String,
    #[serde(skip)]
    pub overload_idx: Option<usize>,
    #[serde(skip)]
    pub path_components: Vec<PathComponent>,
    pub line: usize,
    pub end_line: usize,
    #[serde(skip)]
    pub start_byte: usize,
    #[serde(skip)]
    pub name_byte: usize,
    #[serde(skip)]
    pub end_byte: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Symbol>,
}

impl Symbol {
    fn flatten<'a>(&'a self, out: &mut Vec<&'a Symbol>) {
        out.push(self);
        for child in &self.children {
            child.flatten(out);
        }
    }
}

/// Flattens a hierarchical symbol tree into depth-first slice order.
pub fn flatten(symbols: &[Symbol]) -> Vec<&Symbol> {
    let mut out = Vec::new();
    for symbol in symbols {
        symbol.flatten(&mut out);
    }
    out
}

/// Assigns sibling overload indices (`[0]`, `[1]`) and builds full `name_path` strings.
pub fn finalize_paths(
    symbols: &mut [Symbol],
    prefix_str: &str,
    prefix_components: &[PathComponent],
) {
    let mut total_counts: HashMap<String, usize> = HashMap::new();
    for symbol in symbols.iter() {
        *total_counts.entry(symbol.name.clone()).or_insert(0) += 1;
    }
    let mut seen_counts: HashMap<String, usize> = HashMap::new();
    for symbol in symbols.iter_mut() {
        let total = total_counts.get(&symbol.name).copied().unwrap_or(0);
        let overload_idx = if total > 1 {
            let entry = seen_counts.entry(symbol.name.clone()).or_insert(0);
            let idx = *entry;
            *entry += 1;
            Some(idx)
        } else {
            None
        };
        let segment = match overload_idx {
            Some(idx) => format!("{}[{idx}]", symbol.name),
            None => symbol.name.clone(),
        };
        let name_path = if prefix_str.is_empty() {
            segment
        } else {
            format!("{prefix_str}/{segment}")
        };
        let mut components = prefix_components.to_vec();
        components.push((symbol.name.clone(), overload_idx.map(|idx| idx as i64)));
        symbol.overload_idx = overload_idx;
        symbol.name_path = name_path;
        symbol.path_components = components;
        finalize_paths(
            &mut symbol.children,
            &symbol.name_path,
            &symbol.path_components,
        );
    }
}

/// Collects raw hierarchical symbols from a tree-sitter AST root.
pub fn collect_symbols(root: tree_sitter::Node<'_>, source: &str) -> Vec<Symbol> {
    let mut symbols = collect(root, source);
    finalize_paths(&mut symbols, "", &[]);
    symbols
}

/// Searches flattened symbols by Serena name-path pattern.
pub fn find_in_symbols<'a>(
    symbols: &'a [Symbol],
    pattern: &str,
    substring: bool,
) -> Vec<&'a Symbol> {
    let Ok(parsed_pattern) = parse_pattern(pattern) else {
        return Vec::new();
    };
    flatten(symbols)
        .into_iter()
        .filter(|symbol| matches_components(&parsed_pattern, &symbol.path_components, substring))
        .collect()
}

/// Selects the best matching symbol (preferring exact `name_path` equality).
pub fn best_match_in_symbols<'a>(
    symbols: &'a [Symbol],
    pattern: &str,
    substring: bool,
) -> Option<&'a Symbol> {
    let matches = find_in_symbols(symbols, pattern, substring);
    let trimmed = pattern.trim_matches('/');
    matches
        .iter()
        .find(|symbol| symbol.name_path == trimmed)
        .copied()
        .or_else(|| matches.first().copied())
}

fn text<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "theorem" => "theorem",
        "def"
        | "abbrev"
        | "axiom"
        | "singleton_method"
        | "method"
        | "local_function_statement"
        | "function_declaration"
        | "function_definition"
        | "function" => "function",
        "structure" | "inductive" | "record_declaration" => "type",
        "instance" => "impl",
        "section" | "file_scoped_namespace_declaration" | "module_definition" => "module",
        "object_declaration"
        | "companion_object"
        | "object_definition"
        | "contract_declaration" => "class",
        "constant" | "enum_case" => "const",
        "property_declaration" | "property_element" | "block_mapping_pair" | "pair" => "variable",
        "table" | "table_array_element" => "section",
        "rule_set" => "rule",
        "element" => "element",
        k if k.contains("class") => "class",
        k if k.contains("interface") => "interface",
        k if k.contains("trait") => "trait",
        k if k.contains("protocol") => "interface",
        k if k.contains("enum") => "enum",
        k if k.contains("struct") || k.contains("union_specifier") => "struct",
        k if k.contains("impl") => "impl",
        k if k.contains("module") || k.contains("namespace") => "module",
        k if k.contains("constructor") || k.contains("destructor") => "constructor",
        k if k.contains("method") || k.contains("function") => "function",
        k if k.contains("type") => "type",
        k if k.contains("const") => "const",
        k if k.contains("variable") || k.contains("field") => "variable",
        _ => "declaration",
    }
}

fn is_named_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function_definition"
            | "function_item"
            | "function_signature"
            | "function"
            | "generator_function_declaration"
            | "data_type"
            | "newtype"
            | "method"
            | "singleton_method"
            | "method_definition"
            | "method_declaration"
            | "method_signature"
            | "constructor_declaration"
            | "destructor_declaration"
            | "local_function_statement"
            | "class"
            | "class_declaration"
            | "abstract_class_declaration"
            | "class_definition"
            | "contract_declaration"
            | "object_declaration"
            | "object_definition"
            | "companion_object"
            | "interface_declaration"
            | "trait_declaration"
            | "trait_definition"
            | "protocol_declaration"
            | "type_alias_declaration"
            | "type_definition"
            | "type_item"
            | "type_spec"
            | "enum_declaration"
            | "enum_item"
            | "enum_specifier"
            | "enum_case"
            | "struct_item"
            | "struct_specifier"
            | "struct_declaration"
            | "struct_definition"
            | "abstract_definition"
            | "union_item"
            | "trait_item"
            | "impl_item"
            | "mod_item"
            | "namespace_definition"
            | "namespace_declaration"
            | "file_scoped_namespace_declaration"
            | "module"
            | "module_definition"
            | "internal_module"
            | "record_declaration"
            | "annotation_type_declaration"
            | "const_item"
            | "static_item"
            | "macro_definition"
            | "function_declarator"
            | "def"
            | "theorem"
            | "abbrev"
            | "axiom"
            | "constant"
            | "structure"
            | "inductive"
            | "class_inductive"
            | "instance"
            | "namespace"
            | "section"
    )
}

fn declarator_name(node: tree_sitter::Node<'_>, source: &str) -> Option<(String, usize)> {
    if let Some(name) = node.child_by_field_name("declarator") {
        return declarator_name(name, source);
    }
    if matches!(
        node.kind(),
        "identifier" | "field_identifier" | "type_identifier" | "property_identifier" | "name"
    ) {
        return Some((text(node, source).to_string(), node.start_byte()));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(name) = declarator_name(child, source) {
            return Some(name);
        }
    }
    None
}

fn first_identifier(node: tree_sitter::Node<'_>, source: &str) -> Option<(String, usize)> {
    if matches!(
        node.kind(),
        "identifier" | "word" | "name" | "simple_identifier" | "type_identifier" | "alias"
    ) {
        return Some((text(node, source).trim().to_string(), node.start_byte()));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(found) = first_identifier(child, source) {
            return Some(found);
        }
    }
    None
}

fn strip_quotes(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn elixir_call_declaration(
    node: tree_sitter::Node<'_>,
    source: &str,
) -> Option<(String, String, usize)> {
    let target = node.child_by_field_name("target")?;
    let kw = text(target, source).trim();
    let kind = match kw {
        "defmodule" | "defprotocol" => "module",
        "defimpl" => "impl",
        "defstruct" => "struct",
        "def" | "defp" | "defmacro" | "defmacrop" | "defguard" | "defguardp" => "function",
        _ => return None,
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "arguments" {
            let mut arg_cursor = child.walk();
            for arg in child.named_children(&mut arg_cursor) {
                if arg.kind() == "call"
                    && let Some(fn_target) = arg.child_by_field_name("target")
                {
                    return Some((
                        text(fn_target, source).trim().to_string(),
                        kind.to_string(),
                        fn_target.start_byte(),
                    ));
                }
                if let Some((name, byte)) = first_identifier(arg, source) {
                    return Some((name, kind.to_string(), byte));
                }
            }
        }
    }
    None
}

fn declarations(node: tree_sitter::Node<'_>, source: &str) -> Vec<(String, String, usize)> {
    let kind = node.kind();

    if matches!(
        kind,
        "lexical_declaration" | "variable_declaration" | "const_declaration" | "var_declaration"
    ) {
        let mut out = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_declarator"
                && let Some(name) = child.child_by_field_name("name")
            {
                let display = match child.child_by_field_name("value").map(|value| value.kind()) {
                    Some(
                        "arrow_function"
                        | "function"
                        | "function_expression"
                        | "generator_function",
                    ) => "function",
                    _ => "variable",
                };
                out.push((
                    text(name, source).to_string(),
                    display.to_string(),
                    name.start_byte(),
                ));
            }
        }
        if !out.is_empty() {
            return out;
        }
        // Zig `const Foo = ...;` or similar variable declarations with an identifier child.
        if let Some((name, byte)) = first_identifier(node, source) {
            return vec![(name, "variable".to_string(), byte)];
        }
        return out;
    }

    if kind == "export_statement"
        || kind == "decorated_definition"
        || kind == "template_declaration"
    {
        return Vec::new();
    }

    if kind == "impl_item"
        && let Some(type_node) = node.child_by_field_name("type")
    {
        return vec![(
            text(type_node, source).to_string(),
            "impl".to_string(),
            type_node.start_byte(),
        )];
    }

    if kind == "property_declaration" {
        if let Some(name) = node.child_by_field_name("name") {
            return vec![(
                text(name, source).to_string(),
                "variable".to_string(),
                name.start_byte(),
            )];
        }
        let mut out = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "property_element"
                && let Some(name) = child.child_by_field_name("name")
            {
                out.push((
                    text(name, source).to_string(),
                    "variable".to_string(),
                    name.start_byte(),
                ));
            } else if child.kind() == "variable_declaration" {
                let mut vcursor = child.walk();
                for vchild in child.named_children(&mut vcursor) {
                    if vchild.kind() == "identifier" {
                        out.push((
                            text(vchild, source).to_string(),
                            "variable".to_string(),
                            vchild.start_byte(),
                        ));
                        break;
                    }
                }
            }
        }
        return out;
    }

    if kind == "call"
        && let Some(decl) = elixir_call_declaration(node, source)
    {
        return vec![decl];
    }

    // Zig `Decl` wrapping `FnProto` (`function`) or `VarDecl` (`variable_type_function`)
    if kind == "Decl" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "FnProto"
                && let Some(fn_name) = child.child_by_field_name("function")
            {
                return vec![(
                    text(fn_name, source).trim().to_string(),
                    "function".to_string(),
                    fn_name.start_byte(),
                )];
            }
            if child.kind() == "VarDecl"
                && let Some(var_name) = child.child_by_field_name("variable_type_function")
            {
                return vec![(
                    text(var_name, source).trim().to_string(),
                    "variable".to_string(),
                    var_name.start_byte(),
                )];
            }
        }
        return Vec::new();
    }

    // YAML `block_mapping_pair` and JSON `pair`
    if matches!(kind, "block_mapping_pair" | "pair") {
        if let Some(key) = node.child_by_field_name("key") {
            let name = strip_quotes(text(key, source));
            if !name.is_empty() {
                return vec![(name, "variable".to_string(), key.start_byte())];
            }
        }
        let mut cursor = node.walk();
        if let Some(first) = node.named_children(&mut cursor).next() {
            let name = strip_quotes(text(first, source));
            if !name.is_empty() {
                return vec![(name, "variable".to_string(), first.start_byte())];
            }
        }
    }

    // TOML `table` / `table_array_element`
    if matches!(kind, "table" | "table_array_element") {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "bare_key" | "dotted_key" | "quoted_key") {
                let name = strip_quotes(text(child, source));
                if !name.is_empty() {
                    return vec![(name, "section".to_string(), child.start_byte())];
                }
            }
        }
    }

    // Markdown `section` wrapping headings (`# Title`)
    if kind == "section" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "atx_heading" | "setext_heading") {
                let raw = text(child, source).trim();
                let cleaned = raw.trim_start_matches('#').trim();
                if !cleaned.is_empty() {
                    return vec![(
                        cleaned.to_string(),
                        "section".to_string(),
                        child.start_byte(),
                    )];
                }
            }
        }
        return Vec::new();
    }

    // HTML `element` -> tag name (plus `#id` if present? Keep tag_name clean for outline).
    if kind == "element" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "start_tag" | "self_closing_tag") {
                let mut tcursor = child.walk();
                for tchild in child.named_children(&mut tcursor) {
                    if tchild.kind() == "tag_name" {
                        return vec![(
                            text(tchild, source).trim().to_string(),
                            "element".to_string(),
                            tchild.start_byte(),
                        )];
                    }
                }
            }
        }
        return Vec::new();
    }

    // CSS `rule_set` -> selector string
    if kind == "rule_set" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "selectors" {
                let sel = text(child, source).trim().to_string();
                if !sel.is_empty() {
                    return vec![(sel, "rule".to_string(), child.start_byte())];
                }
            }
        }
        return Vec::new();
    }

    // Julia `signature` inside `function_definition` or `macro_definition`, plus direct identifier children (e.g. Kotlin)
    if is_named_declaration(kind) {
        if let Some(name) = node.child_by_field_name("name") {
            return vec![(
                text(name, source).trim().to_string(),
                kind_label(kind).to_string(),
                name.start_byte(),
            )];
        }
        if let Some(sig) = node.child_by_field_name("signature")
            && let Some((name, byte)) = first_identifier(sig, source)
        {
            return vec![(name, kind_label(kind).to_string(), byte)];
        }
        let mut direct_cursor = node.walk();
        for child in node.named_children(&mut direct_cursor) {
            if matches!(
                child.kind(),
                "identifier"
                    | "simple_identifier"
                    | "type_identifier"
                    | "field_identifier"
                    | "property_identifier"
                    | "name"
                    | "word"
                    | "alias"
            ) {
                return vec![(
                    text(child, source).trim().to_string(),
                    kind_label(kind).to_string(),
                    child.start_byte(),
                )];
            }
        }
        if matches!(
            kind,
            "function_definition"
                | "function_declaration"
                | "module_definition"
                | "struct_definition"
                | "abstract_definition"
                | "macro_definition"
                | "function_signature"
        ) && let Some((name, byte)) = first_identifier(node, source)
        {
            return vec![(name, kind_label(kind).to_string(), byte)];
        }
    }

    if kind == "function_definition"
        && let Some((name, byte)) = declarator_name(node, source)
    {
        return vec![(name, "function".to_string(), byte)];
    }

    Vec::new()
}

fn collect(node: tree_sitter::Node<'_>, source: &str) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let declared = declarations(child, source);
        if declared.is_empty() {
            out.extend(collect(child, source));
            continue;
        }
        for (name, kind, name_byte) in declared {
            let children = collect(child, source);
            let start = child.start_position();
            let end = child.end_position();
            out.push(Symbol {
                name,
                kind,
                name_path: String::new(),
                overload_idx: None,
                path_components: Vec::new(),
                line: start.row + 1,
                end_line: end.row + 1,
                start_byte: child.start_byte(),
                name_byte,
                end_byte: child.end_byte(),
                children,
            });
        }
    }
    out
}
