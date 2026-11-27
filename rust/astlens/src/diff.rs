//! Compact unified diffs for edits.

use std::path::Path;

use similar::TextDiff;

pub fn unified(path: &Path, old: &str, new: &str) -> String {
    let name = path.display().to_string();
    TextDiff::from_lines(old, new)
        .unified_diff()
        .context_radius(3)
        .header(&name, &name)
        .to_string()
}
