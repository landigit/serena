//! Tree-sitter structural engine: outlines, symbol bodies and single-node edits.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    TypeScript,
    Tsx,
    JavaScript,
    Rust,
    Python,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Php,
    CSharp,
    Kotlin,
    Lean,
}

impl Lang {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "ts" | "mts" | "cts" => Some(Self::TypeScript),
            "tsx" => Some(Self::Tsx),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "rs" => Some(Self::Rust),
            "py" | "pyi" => Some(Self::Python),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(Self::Cpp),
            "rb" | "rake" | "gemspec" => Some(Self::Ruby),
            "php" | "phtml" => Some(Self::Php),
            "cs" => Some(Self::CSharp),
            "kt" | "kts" => Some(Self::Kotlin),
            "lean" => Some(Self::Lean),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::Tsx => "tsx",
            Self::JavaScript => "javascript",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::CSharp => "csharp",
            Self::Kotlin => "kotlin",
            Self::Lean => "lean",
        }
    }

    fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Self::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Self::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Self::Lean => arborium_lean::language().into(),
        }
    }

    /// Language id sent to the LSP in `didOpen`.
    pub fn lsp_id(self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::Tsx => "typescriptreact",
            Self::JavaScript => "javascript",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::Ruby => "ruby",
            Self::Php => "php",
            Self::CSharp => "csharp",
            Self::Kotlin => "kotlin",
            Self::Lean => "lean4",
        }
    }
}

pub type PathComponent = (String, Option<i64>);

struct ParsedPattern {
    absolute: bool,
    components: Vec<PathComponent>,
}

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

pub struct Parsed {
    pub lang: Lang,
    pub source: String,
    pub symbols: Vec<Symbol>,
}

pub fn parse(path: &Path) -> Result<Parsed, String> {
    let lang = Lang::from_path(path)
        .ok_or_else(|| format!("unsupported file type: {}", path.display()))?;
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let source =
        String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))?;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&lang.grammar())
        .map_err(|error| format!("tree-sitter grammar for {}: {error}", lang.name()))?;
    let tree = parser
        .parse(source.as_bytes(), None)
        .ok_or_else(|| format!("tree-sitter could not parse {}", path.display()))?;

    let mut symbols = collect(tree.root_node(), &source);
    finalize_paths(&mut symbols, "", &[]);
    Ok(Parsed {
        lang,
        source,
        symbols,
    })
}

fn text<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> &'a str {
    &source[node.byte_range()]
}

fn kind_label(kind: &str) -> &'static str {
    match kind {
        "theorem" => "theorem",
        "def" | "abbrev" | "axiom" | "singleton_method" | "method" | "local_function_statement" => {
            "function"
        }
        "structure" | "inductive" | "record_declaration" => "type",
        "instance" => "impl",
        "section" | "file_scoped_namespace_declaration" => "module",
        "object_declaration" | "companion_object" => "class",
        "constant" | "enum_case" => "const",
        "property_declaration" | "property_element" => "variable",
        k if k.contains("class") => "class",
        k if k.contains("interface") => "interface",
        k if k.contains("trait") => "trait",
        k if k.contains("enum") => "enum",
        k if k.contains("struct") || k.contains("union_specifier") => "struct",
        k if k.contains("impl") => "impl",
        k if k.contains("module") || k.contains("namespace") => "module",
        k if k.contains("constructor") | k.contains("destructor") => "constructor",
        k if k.contains("method") || k.contains("function") => "function",
        k if k.contains("type") => "type",
        k if k.contains("const") => "const",
        k if k.contains("variable") || k.contains("field") => "variable",
        _ => "declaration",
    }
}

/// Declaration kinds that carry a `name` field (plus a few special cases below).
fn is_named_declaration(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "function_definition"
            | "function_item"
            | "function_signature"
            | "generator_function_declaration"
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
            | "object_declaration"
            | "companion_object"
            | "interface_declaration"
            | "trait_declaration"
            | "type_alias_declaration"
            | "type_item"
            | "type_spec"
            | "enum_declaration"
            | "enum_item"
            | "enum_specifier"
            | "enum_case"
            | "struct_item"
            | "struct_specifier"
            | "struct_declaration"
            | "union_item"
            | "trait_item"
            | "impl_item"
            | "mod_item"
            | "namespace_definition"
            | "namespace_declaration"
            | "file_scoped_namespace_declaration"
            | "module"
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

/// Names declared by a node, with the kind to display. `lexical_declaration`
/// and friends expand to their declarators so `const handle = () => {}` is a symbol.
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
    if is_named_declaration(kind)
        && let Some(name) = node.child_by_field_name("name")
    {
        return vec![(
            text(name, source).to_string(),
            kind_label(kind).to_string(),
            name.start_byte(),
        )];
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

fn finalize_paths(symbols: &mut [Symbol], prefix_str: &str, prefix_components: &[PathComponent]) {
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

pub fn flatten(symbols: &[Symbol]) -> Vec<&Symbol> {
    let mut out = Vec::new();
    for symbol in symbols {
        symbol.flatten(&mut out);
    }
    out
}

fn parse_pattern(pattern: &str) -> Option<ParsedPattern> {
    let raw = crate::name_path::pattern_components(pattern.to_string()).ok()?;
    let mut components = Vec::with_capacity(raw.len());
    for item in raw {
        let array = item.as_array()?;
        let name = array.first()?.as_str()?.to_string();
        let overload_idx = array.get(1).and_then(serde_json::Value::as_i64);
        components.push((name, overload_idx));
    }
    Some(ParsedPattern {
        absolute: pattern.starts_with('/'),
        components,
    })
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
    for (idx, ((pat_name, pat_idx), (sym_name, sym_idx))) in
        pattern.iter().zip(symbol[offset..].iter()).enumerate()
    {
        let is_last = idx + 1 == pattern.len();
        let name_ok = if substring_matching && is_last {
            sym_name.contains(pat_name.as_str())
        } else {
            sym_name == pat_name
        };
        if !name_ok {
            return false;
        }
        if let Some(wanted) = pat_idx
            && *sym_idx != Some(*wanted)
        {
            return false;
        }
    }
    true
}

/// All symbols whose name path matches the pattern (Serena semantics, overload-aware).
pub fn find<'a>(parsed: &'a Parsed, pattern: &str, substring: bool) -> Vec<&'a Symbol> {
    let Some(parsed_pattern) = parse_pattern(pattern) else {
        return Vec::new();
    };
    flatten(&parsed.symbols)
        .into_iter()
        .filter(|symbol| matches_components(&parsed_pattern, &symbol.path_components, substring))
        .collect()
}

pub fn best_match<'a>(parsed: &'a Parsed, pattern: &str, substring: bool) -> Option<&'a Symbol> {
    let matches = find(parsed, pattern, substring);
    let trimmed = pattern.trim_matches('/');
    matches
        .iter()
        .find(|symbol| symbol.name_path == trimmed)
        .copied()
        .or_else(|| matches.first().copied())
}

pub fn body(parsed: &Parsed, symbol: &Symbol, max_chars: usize) -> (String, bool) {
    let raw = &parsed.source[symbol.start_byte..symbol.end_byte];
    let total = raw.chars().count();
    if total <= max_chars {
        return (raw.to_string(), false);
    }
    let cut: String = raw.chars().take(max_chars).collect();
    (cut, true)
}

pub fn line_preview(source: &str, line0: usize) -> String {
    source
        .lines()
        .nth(line0)
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("astlens-ts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn outlines_nested_tsx_handlers_and_hooks() {
        let path = fixture(
            "sample.tsx",
            "export function App() {\n  const [mode, setMode] = useState('view');\n  const handleClick = () => setMode('edit');\n  return <div onClick={handleClick}>{mode}</div>;\n}\n",
        );
        let parsed = parse(&path).unwrap();
        let names: Vec<&str> = flatten(&parsed.symbols)
            .iter()
            .map(|symbol| symbol.name_path.as_str())
            .collect();
        assert!(names.contains(&"App/handleClick"), "{names:?}");
        assert!(names.contains(&"App/[mode, setMode]"), "{names:?}");
    }

    #[test]
    fn finds_symbols_by_name_path() {
        let path = fixture(
            "sample.py",
            "class Store:\n    def add(self, item):\n        return item\n",
        );
        let parsed = parse(&path).unwrap();
        let symbol = best_match(&parsed, "Store/add", false).expect("Store/add");
        assert_eq!(symbol.name, "add");
        assert_eq!(symbol.line, 2);
    }

    #[test]
    fn outlines_ruby_php_csharp_kotlin_and_lean() {
        let rb = fixture(
            "sample.rb",
            "module Billing\n  class Invoice\n    def total\n      42\n    end\n  end\nend\n",
        );
        let rb_names: Vec<String> = flatten(&parse(&rb).unwrap().symbols)
            .into_iter()
            .map(|s| s.name_path.clone())
            .collect();
        assert!(
            rb_names.contains(&"Billing/Invoice/total".to_string()),
            "{rb_names:?}"
        );

        let php = fixture(
            "sample.php",
            "<?php\nnamespace App;\nclass Greeter {\n  public function hello() { return 'hi'; }\n}\n",
        );
        let php_names: Vec<String> = flatten(&parse(&php).unwrap().symbols)
            .into_iter()
            .map(|s| s.name_path.clone())
            .collect();
        assert!(php_names.contains(&"App".to_string()), "{php_names:?}");
        assert!(
            php_names.contains(&"Greeter/hello".to_string()),
            "{php_names:?}"
        );

        let cs = fixture(
            "Sample.cs",
            "namespace App {\n  public class Calc {\n    public int Add(int a, int b) => a + b;\n  }\n}\n",
        );
        let cs_names: Vec<String> = flatten(&parse(&cs).unwrap().symbols)
            .into_iter()
            .map(|s| s.name_path.clone())
            .collect();
        assert!(
            cs_names.contains(&"App/Calc/Add".to_string()),
            "{cs_names:?}"
        );

        let kt = fixture(
            "Sample.kt",
            "class Repo {\n  val count: Int = 1\n  fun fetch() = count\n}\n",
        );
        let kt_names: Vec<String> = flatten(&parse(&kt).unwrap().symbols)
            .into_iter()
            .map(|s| s.name_path.clone())
            .collect();
        assert!(kt_names.contains(&"Repo/count".to_string()), "{kt_names:?}");
        assert!(kt_names.contains(&"Repo/fetch".to_string()), "{kt_names:?}");

        let lean = fixture(
            "Sample.lean",
            "namespace Math\nstructure Point where\n  x : Nat\ndef idNat (n : Nat) : Nat := n\ntheorem id_eq (n : Nat) : idNat n = n := rfl\nend Math\n",
        );
        let lean_names: Vec<String> = flatten(&parse(&lean).unwrap().symbols)
            .into_iter()
            .map(|s| s.name_path.clone())
            .collect();
        assert!(
            lean_names.contains(&"Math/Point".to_string()),
            "{lean_names:?}"
        );
        assert!(
            lean_names.contains(&"Math/idNat".to_string()),
            "{lean_names:?}"
        );
        assert!(
            lean_names.contains(&"Math/id_eq".to_string()),
            "{lean_names:?}"
        );
    }

    #[test]
    fn assigns_overload_indexes_and_matches_by_index() {
        let java = fixture(
            "Overload.java",
            "class Calc {\n  int add(int a) { return a; }\n  double add(double a) { return a; }\n  void clear() {}\n}\n",
        );
        let parsed = parse(&java).unwrap();
        let paths: Vec<&str> = flatten(&parsed.symbols)
            .iter()
            .map(|symbol| symbol.name_path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["Calc", "Calc/add[0]", "Calc/add[1]", "Calc/clear"]
        );

        assert_eq!(best_match(&parsed, "Calc/add[0]", false).unwrap().line, 2);
        assert_eq!(best_match(&parsed, "Calc/add[1]", false).unwrap().line, 3);
        assert_eq!(best_match(&parsed, "Calc/add[1]/", false).unwrap().line, 3);
        assert_eq!(best_match(&parsed, "/Calc/add[1]", false).unwrap().line, 3);
        assert_eq!(best_match(&parsed, "add[1]", false).unwrap().line, 3);
        assert_eq!(find(&parsed, "Calc/add", false).len(), 2);
        assert!(find(&parsed, "", false).is_empty());
        assert!(best_match(&parsed, "Calc/add[2]", false).is_none());
        assert!(best_match(&parsed, "Calc/clear[0]", false).is_none());
        assert_eq!(best_match(&parsed, "Calc/ad[1]", true).unwrap().line, 3);

        let rs = fixture(
            "overloads.rs",
            "struct Foo {}\nimpl Foo { fn a() {} }\nimpl Foo { fn b() {} }\n",
        );
        let rs_parsed = parse(&rs).unwrap();
        let rs_paths: Vec<&str> = flatten(&rs_parsed.symbols)
            .iter()
            .map(|symbol| symbol.name_path.as_str())
            .collect();
        assert_eq!(
            rs_paths,
            vec!["Foo[0]", "Foo[1]", "Foo[1]/a", "Foo[2]", "Foo[2]/b"]
        );
        assert_eq!(best_match(&rs_parsed, "Foo/b", false).unwrap().line, 3);
        assert_eq!(best_match(&rs_parsed, "Foo[2]/b", false).unwrap().line, 3);
        assert!(best_match(&rs_parsed, "Foo[1]/b", false).is_none());
    }
}
