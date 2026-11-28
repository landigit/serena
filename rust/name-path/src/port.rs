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
        let components: Vec<(String, Option<i64>)> =
            parts.iter().map(|p| (p.clone(), None)).collect();
        self.matches_with_overloads(&components, substring_matching)
    }

    /// Port of `matches_reversed_components` over `(name, overload_idx)` pairs.
    fn matches_with_overloads(
        &self,
        parts: &[(String, Option<i64>)],
        substring_matching: bool,
    ) -> bool {
        let mut symbol = parts.iter().rev();
        for (i, component) in self.components.iter().rev().enumerate() {
            let Some((name, overload_idx)) = symbol.next() else {
                return false;
            };
            let substring = substring_matching && i == 0;
            if !component.matches(name, *overload_idx, substring) {
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

/// Port of `match_name_path_overloaded` (`rust/oracle/name_path.py`).
pub fn match_name_path_overloaded(
    pattern: String,
    components: Vec<Value>,
    substring: bool,
) -> Result<bool, String> {
    let mut parsed = Vec::with_capacity(components.len());
    for item in components {
        let arr = item
            .as_array()
            .ok_or_else(|| "TypeError: expected [name, overload_idx]".to_string())?;
        let name = arr
            .first()
            .and_then(Value::as_str)
            .ok_or_else(|| "TypeError: component name must be a string".to_string())?
            .to_string();
        let overload_idx = arr.get(1).and_then(Value::as_i64);
        parsed.push((name, overload_idx));
    }
    Ok(Matcher::parse(&pattern)?.matches_with_overloads(&parsed, substring))
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
        assert!(
            match_name_path_overloaded(
                "ClassA/method[1]".into(),
                vec![json!(["ClassA", null]), json!(["method", 1])],
                false
            )
            .unwrap()
        );
        assert!(
            !match_name_path_overloaded(
                "ClassA/method[1]".into(),
                vec![json!(["ClassA", null]), json!(["method", 0])],
                false
            )
            .unwrap()
        );
        assert!(
            match_name_path_overloaded(
                "ClassA/met[1]".into(),
                vec![json!(["ClassA", null]), json!(["method", 1])],
                true
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
