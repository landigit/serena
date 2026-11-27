//! The Rust port of `rust/oracle/diagnostics_group.py`.
//!
//! Group diagnostics by file -> severity -> name path -> list.

#![forbid(unsafe_code)]

use serde_json::{Map, Value, json};

fn py_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

fn attr_err(v: &Value) -> String {
    format!(
        "AttributeError: '{}' object has no attribute 'get'",
        py_type(v)
    )
}

/// Port of `severity_name` (`rust/oracle/diagnostics_group.py:10`).
pub fn severity_name(severity: Option<i64>) -> Result<String, String> {
    Ok(match severity {
        None => "Unknown".into(),
        Some(1) => "Error".into(),
        Some(2) => "Warning".into(),
        Some(3) => "Information".into(),
        Some(4) => "Hint".into(),
        Some(n) => format!("Severity_{n}"),
    })
}

/// Port of `diagnostic_output` (`rust/oracle/diagnostics_group.py:16`).
pub fn diagnostic_output(d: Value) -> Result<Value, String> {
    if !d.is_object() {
        return Err(attr_err(&d));
    }
    let message = d
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let range = d.get("range").cloned().unwrap_or_else(|| json!({}));
    let mut out = json!({ "message": message, "range": range });
    for key in ["code", "source"] {
        if let Some(v) = d.get(key) {
            out[key] = v.clone();
        }
    }
    Ok(out)
}

/// Port of `group` (`rust/oracle/diagnostics_group.py:25`).
pub fn group(relative_path: String, entries: Vec<Value>) -> Result<Value, String> {
    let mut file_map: Map<String, Value> = Map::new();
    for entry in entries {
        if !entry.is_object() {
            return Err(attr_err(&entry));
        }
        let name_path = entry
            .get("name_path")
            .and_then(Value::as_str)
            .unwrap_or("<file>")
            .to_string();
        let diag = match entry.get("diagnostic") {
            Some(v) if !v.is_object() => return Err(attr_err(v)),
            Some(v) => v.clone(),
            None => json!({}),
        };
        let sev = severity_name(diag.get("severity").and_then(Value::as_i64))?;
        let out = diagnostic_output(diag)?;
        file_map
            .entry(sev)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or("severity map must be an object")?
            .entry(name_path)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("list must be an array")?
            .push(out);
    }
    Ok(json!({ relative_path: file_map }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(severity: i64, msg: &str) -> Value {
        json!({"message": msg, "range": {}, "severity": severity, "source": "Pyright"})
    }

    #[test]
    fn groups_by_severity_and_name_path() {
        let g = group(
            "src/example.py".into(),
            vec![
                json!({"name_path": "<file>", "diagnostic": diag(2, "Unexpected argument")}),
                json!({"name_path": "MyClass/my_method", "diagnostic": diag(1, "Type mismatch")}),
            ],
        )
        .unwrap();
        let file = &g["src/example.py"];
        assert_eq!(file["Warning"]["<file>"].as_array().unwrap().len(), 1);
        assert_eq!(
            file["Error"]["MyClass/my_method"].as_array().unwrap().len(),
            1
        );
    }

    #[test]
    fn severity_names() {
        assert_eq!(severity_name(Some(2)).unwrap(), "Warning");
        assert_eq!(severity_name(Some(9)).unwrap(), "Severity_9");
        assert_eq!(severity_name(None).unwrap(), "Unknown");
    }
}
