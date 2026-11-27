//! The Rust port of `rust/oracle/diagnostics_group.py`.
//!
//! Group diagnostics by file -> severity -> name path -> list, the shape
//! Serena's `GroupedDiagnostics` exposes and astlens' `diagnostics` tool returns.

#![forbid(unsafe_code)]

/// Port of `severity_name` (`rust/oracle/diagnostics_group.py:10`).
pub fn severity_name(severity: Option<i64>) -> Result<String, String> {
    let Some(severity) = severity else {
        return Ok("Unknown".to_string());
    };
    Ok(match severity {
        1 => "Error".to_string(),
        2 => "Warning".to_string(),
        3 => "Information".to_string(),
        4 => "Hint".to_string(),
        _ => format!("Severity_{severity}"),
    })
}

/// Port of `diagnostic_output` (`rust/oracle/diagnostics_group.py:16`).
pub fn diagnostic_output(diagnostic: serde_json::Value) -> Result<serde_json::Value, String> {
    let message = diagnostic
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let range = diagnostic
        .get("range")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut result = serde_json::json!({ "message": message, "range": range });
    if let Some(code) = diagnostic.get("code") {
        result["code"] = code.clone();
    }
    if let Some(source) = diagnostic.get("source") {
        result["source"] = source.clone();
    }
    Ok(result)
}

/// Port of `group` (`rust/oracle/diagnostics_group.py:25`).
fn python_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "NoneType",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "int",
        serde_json::Value::String(_) => "str",
        serde_json::Value::Array(_) => "list",
        serde_json::Value::Object(_) => "dict",
    }
}

pub fn group(relative_path: String, entries: Vec<serde_json::Value>) -> Result<serde_json::Value, String> {
    let mut grouped = serde_json::Map::new();
    grouped.insert(relative_path.clone(), serde_json::Value::Object(serde_json::Map::new()));
    let file_map = grouped
        .get_mut(&relative_path)
        .ok_or("group: missing file")?
        .as_object_mut()
        .ok_or("group: file map must be an object")?;
    for entry in entries {
        if !entry.is_object() {
            return Err(format!(
                "AttributeError: '{}' object has no attribute 'get'",
                python_type_name(&entry)
            ));
        }
        let name_path = entry
            .get("name_path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<file>")
            .to_string();
        let diagnostic = match entry.get("diagnostic") {
            Some(value) => {
                if !value.is_object() {
                    return Err(format!(
                        "AttributeError: '{}' object has no attribute 'get'",
                        python_type_name(value)
                    ));
                }
                value.clone()
            }
            None => serde_json::Value::Object(serde_json::Map::new()),
        };
        let severity = severity_name(
            diagnostic
                .get("severity")
                .and_then(serde_json::Value::as_i64),
        )?;
        let output = diagnostic_output(diagnostic.clone())?;
        let mut severity_value = file_map
            .get(&severity)
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let mut severity_map = severity_value
            .as_object_mut()
            .ok_or("group: severity map must be an object")?;
        let mut list_value = severity_map
            .get(&name_path)
            .cloned()
            .unwrap_or(serde_json::Value::Array(Vec::new()));
        let list = list_value
            .as_array_mut()
            .ok_or("group: group list must be an array")?;
        list.push(output);
        file_map.insert(severity.clone(), serde_json::Value::Object(severity_map.clone()));
    }
    Ok(serde_json::Value::Object(grouped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic(severity: i64, message: &str) -> serde_json::Value {
        serde_json::json!({
            "message": message,
            "range": {"start": {"line": 4, "character": 8}, "end": {"line": 4, "character": 21}},
            "severity": severity,
            "source": "Pyright",
        })
    }

    #[test]
    fn groups_by_severity_and_name_path() {
        let grouped = group(
            "src/example.py".to_string(),
            vec![
                serde_json::json!({"name_path": "<file>", "diagnostic": diagnostic(2, "Unexpected argument")}),
                serde_json::json!({"name_path": "MyClass/my_method", "diagnostic": diagnostic(1, "Type mismatch")}),
            ],
        )
        .unwrap();
        let file_map = grouped.as_object().unwrap();
        let file = file_map.get("src/example.py").unwrap();
        let by_severity = file.as_object().unwrap();
        let by_name = by_severity.get("Warning").unwrap().as_object().unwrap();
        assert_eq!(by_name.get("<file>").unwrap().as_array().unwrap().len(), 1);
        let by_name2 = by_severity.get("Error").unwrap().as_object().unwrap();
        assert_eq!(
            by_name2
                .get("MyClass/my_method")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
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
