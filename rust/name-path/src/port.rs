//! The Rust port of `rust/oracle/name_path.py`.
//!
//! Mirrors Serena's `NamePathMatcher`: simple names, relative suffix paths and
//! absolute paths, with optional `[i]` overload indexes. `rust-anything diff`
//! checks behavior against what the source recorded.

#![forbid(unsafe_code)]

use serde_json::{Value, json};

/// A parsed pattern component: `name` or `name[i]`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Component {
    name: String,
    overload_idx: Option<i64>,
}

impl Component {
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

    fn matches(&self, name: &str, overload_idx: Option<i64>, substring: bool) -> bool {
        if substring {
            if !name.contains(&self.name) {
                return false;
            }
        } else if name != self.name {
            return false;
        }
        match self.overload_idx {
            Some(wanted) => overload_idx == Some(wanted),
            None => true,
        }
    }
}

struct Matcher {
    absolute: bool,
    components: Vec<Component>,
}

impl Matcher {
    fn parse(pattern: &str) -> Result<Self, String> {
        if pattern.is_empty() {
            return Err("AssertionError: name_path must not be empty".to_string());
        }
        let body = pattern.trim_matches('/');
        Ok(Self {
            absolute: pattern.starts_with('/'),
            components: body.split('/').map(Component::parse).collect(),
        })
    }

    /// Port of `matches_reversed_components`; `parts` are the symbol's name path.
    fn matches(&self, parts: &[String], substring_matching: bool) -> bool {
        let mut symbol = parts.iter().rev();
        for (i, component) in self.components.iter().rev().enumerate() {
            let Some(name) = symbol.next() else {
                return false;
            };
            let substring = substring_matching && i == 0;
            if !component.matches(name, None, substring) {
                return false;
            }
        }
        if self.absolute {
            symbol.next().is_none()
        } else {
            true
        }
    }
}

/// Port of `match_name_path` (`rust/oracle/name_path.py:34`).
pub fn match_name_path(
    pattern: String,
    parts: Vec<String>,
    substring: bool,
) -> Result<bool, String> {
    Ok(Matcher::parse(&pattern)?.matches(&parts, substring))
}

/// Port of `pattern_components` (`rust/oracle/name_path.py:41`).
pub fn pattern_components(pattern: String) -> Result<Vec<Value>, String> {
    Ok(Matcher::parse(&pattern)?
        .components
        .into_iter()
        .map(|component| json!([component.name, component.overload_idx]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_suffix_and_absolute() {
        assert!(
            match_name_path(
                "bar/foo".into(),
                vec!["mod".into(), "bar".into(), "foo".into()],
                false
            )
            .unwrap()
        );
        assert!(!match_name_path("/foo".into(), vec!["bar".into(), "foo".into()], false).unwrap());
        assert!(match_name_path("foo".into(), vec!["my_foobar_func".into()], true).unwrap());
    }

    #[test]
    fn overload_index_is_parsed_and_blocks_indexless_symbols() {
        assert_eq!(
            pattern_components("ClassA/method[1]".into()).unwrap(),
            vec![json!(["ClassA", null]), json!(["method", 1])]
        );
        assert!(
            !match_name_path(
                "ClassA/method[1]".into(),
                vec!["ClassA".into(), "method".into()],
                false
            )
            .unwrap()
        );
    }

    #[test]
    fn empty_pattern_is_an_assertion_error() {
        assert_eq!(
            match_name_path(String::new(), vec![], false).unwrap_err(),
            "AssertionError: name_path must not be empty"
        );
    }
}
