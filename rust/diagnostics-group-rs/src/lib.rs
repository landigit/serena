//! Python bindings for `rust/oracle/diagnostics_group.py`: generated from the signatures in `port.rs` by rust-anything.
//! Regenerate with `rust-anything refresh` after changing them.

#![allow(clippy::too_many_arguments)]

use pyo3::prelude::*;

pub mod js;
pub mod port;
pub mod pyconv;
pub mod pynum;

#[allow(unused_imports)]
use pynum::Num;

fn error(message: String) -> PyErr {
    use pyo3::exceptions as e;
    /// `Err("Kind: text")` for a `Kind` the match below does not know: a built-in exception's name or
    /// `package.module.Class`, called with the text. None when it names no `Exception` subclass or the
    /// class cannot be made from the text alone (its constructor takes more arguments).
    fn named_error(kind: &str, text: &str) -> Option<PyErr> {
        Python::attach(|py| {
            // Each dotted part must be a Python identifier, by Python's own rules (`str.isidentifier`).
            let identifier = |part: &str| {
                pyo3::types::PyString::new(py, part)
                    .call_method0("isidentifier")
                    .and_then(|ok| ok.extract::<bool>())
                    .unwrap_or(false)
            };
            if !kind.split('.').all(identifier) {
                return None;
            }
            let (module, name) = kind.rsplit_once('.').unwrap_or(("builtins", kind));
            let class = py.import(module).ok()?.getattr(name).ok()?;
            let class = class.cast_into::<pyo3::types::PyType>().ok()?;
            if !class
                .is_subclass_of::<pyo3::exceptions::PyException>()
                .ok()?
            {
                return None;
            }
            let value = class.call1((text,)).ok()?;
            if !value.is_instance(&class).ok()? {
                return None;
            }
            Some(PyErr::from_value(value))
        })
    }
    let (kind, text) = message.split_once(": ").unwrap_or(("", message.as_str()));
    let text = text.to_string();
    match kind {
        "TypeError" => e::PyTypeError::new_err(text),
        "KeyError" => e::PyKeyError::new_err(text),
        "IndexError" => e::PyIndexError::new_err(text),
        "OverflowError" => e::PyOverflowError::new_err(text),
        "ZeroDivisionError" => e::PyZeroDivisionError::new_err(text),
        "AttributeError" => e::PyAttributeError::new_err(text),
        "RuntimeError" => e::PyRuntimeError::new_err(text),
        "NotImplementedError" => e::PyNotImplementedError::new_err(text),
        "LookupError" => e::PyLookupError::new_err(text),
        "ArithmeticError" => e::PyArithmeticError::new_err(text),
        "UnicodeError" => e::PyUnicodeError::new_err(text),
        "ValueError" => e::PyValueError::new_err(text),
        _ => named_error(kind, &text).unwrap_or_else(|| e::PyValueError::new_err(message)),
    }
}

fn severity_name_inner(severity: Option<i64>) -> Result<String, String> {
    let out = port::severity_name(severity)?;
    Ok(out)
}

/// `severity_name`.
#[pyfunction]
#[pyo3(name = "severity_name", signature = (severity))]
fn severity_name(severity: Option<i64>) -> PyResult<String> {
    severity_name_inner(severity).map_err(error)
}

fn diagnostic_output_inner(diagnostic: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let diagnostic: serde_json::Value = crate::pyconv::FromPy::from_py(&diagnostic)?;
    let out = port::diagnostic_output(diagnostic)?;
    Ok(out)
}

/// `diagnostic_output`.
#[pyfunction]
#[pyo3(name = "diagnostic_output", signature = (diagnostic))]
fn diagnostic_output(py: Python<'_>, diagnostic: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    diagnostic_output_inner(diagnostic)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn group_inner(
    relative_path: String,
    entries: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let entries: Vec<serde_json::Value> = crate::pyconv::FromPy::from_py(&entries)?;
    let out = port::group(relative_path, entries)?;
    Ok(out)
}

/// `group`.
#[pyfunction]
#[pyo3(name = "group", signature = (relative_path, entries))]
fn group(py: Python<'_>, relative_path: String, entries: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    group_inner(relative_path, entries)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(severity_name, m)?)?;
    m.add_function(wrap_pyfunction!(diagnostic_output, m)?)?;
    m.add_function(wrap_pyfunction!(group, m)?)?;
    Ok(())
}
