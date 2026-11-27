//! Port of `src/serena/generated/tool_capabilities.py` and the deterministic
//! renderer in `scripts/gen_tool_capabilities.py`.

use std::collections::BTreeSet;

/// Sorted manifest of Serena tools that perform edit/mutating operations.
///
/// Mirrors `EDIT_CAPABLE_TOOL_NAMES` in `src/serena/generated/tool_capabilities.py`.
pub const EDIT_CAPABLE_TOOL_NAMES: &[&str] = &[
    "create_text_file",
    "delete_lines",
    "delete_memory",
    "edit_memory",
    "execute_shell_command",
    "insert_after_symbol",
    "insert_at_line",
    "insert_before_symbol",
    "jet_brains_inline_symbol",
    "jet_brains_move",
    "jet_brains_rename",
    "jet_brains_safe_delete",
    "onboarding",
    "rename_memory",
    "rename_symbol",
    "replace_content",
    "replace_in_files",
    "replace_lines",
    "replace_symbol_body",
    "safe_delete_symbol",
    "serena_repl",
    "write_memory",
];

/// Returns `true` if `tool_name` is in [`EDIT_CAPABLE_TOOL_NAMES`].
pub fn is_edit_capable_tool(tool_name: &str) -> bool {
    EDIT_CAPABLE_TOOL_NAMES.binary_search(&tool_name).is_ok()
}

/// Returns an ordered set of all edit-capable tool names.
pub fn edit_capable_tool_names() -> BTreeSet<&'static str> {
    EDIT_CAPABLE_TOOL_NAMES.iter().copied().collect()
}

/// Renders `tool_capabilities.py` deterministically, matching `_render_tool_capabilities`
/// in `scripts/gen_tool_capabilities.py`.
pub fn render_tool_capabilities_python_module(edit_capable_tools: &[&str]) -> String {
    let entries = edit_capable_tools
        .iter()
        .map(|name| format!("        \"{name}\","))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\"\"\"Generated tool-capability metadata. Do not edit manually.\"\"\"\n\n\
         EDIT_CAPABLE_TOOL_NAMES: frozenset[str] = frozenset(\n    {{\n{entries}\n    }}\n)\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn manifest_is_sorted_unique_and_non_empty() {
        assert_eq!(EDIT_CAPABLE_TOOL_NAMES.len(), 22);
        for pair in EDIT_CAPABLE_TOOL_NAMES.windows(2) {
            assert!(
                pair[0] < pair[1],
                "manifest must be strictly sorted: {:?} >= {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn lookup_matches_expected_tools() {
        assert!(is_edit_capable_tool("create_text_file"));
        assert!(is_edit_capable_tool("serena_repl"));
        assert!(is_edit_capable_tool("write_memory"));
        assert!(!is_edit_capable_tool("read_file"));
        assert!(!is_edit_capable_tool("find_symbol"));
        assert!(!is_edit_capable_tool(""));
        assert!(!is_edit_capable_tool("write_memory "));
        assert_eq!(edit_capable_tool_names().len(), 22);
    }

    #[test]
    fn rendered_module_matches_python_source_file() {
        let py_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../src/serena/generated/tool_capabilities.py");
        let expected = std::fs::read_to_string(&py_path).expect("read python tool_capabilities.py");
        let actual = render_tool_capabilities_python_module(EDIT_CAPABLE_TOOL_NAMES);
        assert_eq!(actual, expected);
    }
}
