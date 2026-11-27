//! Python bindings for `repo_dir_sync.py`: generated from the signatures in `port.rs` by rust-anything.
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

fn popen_inner(cmd: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let cmd: serde_json::Value = crate::pyconv::FromPy::from_py(&cmd)?;
    let out = port::popen(cmd)?;
    Ok(out)
}

/// `popen`.
#[pyfunction]
#[pyo3(name = "popen", signature = (cmd))]
fn popen(py: Python<'_>, cmd: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    popen_inner(cmd)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn call_inner(cmd: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let cmd: serde_json::Value = crate::pyconv::FromPy::from_py(&cmd)?;
    let out = port::call(cmd)?;
    Ok(out)
}

/// `call`.
#[pyfunction]
#[pyo3(name = "call", signature = (cmd))]
fn call(py: Python<'_>, cmd: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    call_inner(cmd)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn execute_inner(
    cmd: Bound<'_, PyAny>,
    exception_on_error: bool,
) -> Result<serde_json::Value, String> {
    let cmd: serde_json::Value = crate::pyconv::FromPy::from_py(&cmd)?;
    let out = port::execute(cmd, exception_on_error)?;
    Ok(out)
}

/// `execute`.
#[pyfunction]
#[pyo3(name = "execute", signature = (cmd, exception_on_error=true))]
fn execute(py: Python<'_>, cmd: Bound<'_, PyAny>, exception_on_error: bool) -> PyResult<Py<PyAny>> {
    execute_inner(cmd, exception_on_error)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn git_log_inner(
    path: Bound<'_, PyAny>,
    arg: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let path: serde_json::Value = crate::pyconv::FromPy::from_py(&path)?;
    let arg: serde_json::Value = crate::pyconv::FromPy::from_py(&arg)?;
    let out = port::git_log(path, arg)?;
    Ok(out)
}

/// `gitLog`.
#[pyfunction]
#[pyo3(name = "gitLog", signature = (path, arg))]
fn git_log(py: Python<'_>, path: Bound<'_, PyAny>, arg: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    git_log_inner(path, arg)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn git_commit_inner(msg: Bound<'_, PyAny>) -> Result<serde_json::Value, String> {
    let msg: serde_json::Value = crate::pyconv::FromPy::from_py(&msg)?;
    let out = port::git_commit(msg)?;
    Ok(out)
}

/// `gitCommit`.
#[pyfunction]
#[pyo3(name = "gitCommit", signature = (msg))]
fn git_commit(py: Python<'_>, msg: Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    git_commit_inner(msg)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn git_commit_with_message_from_file_inner(
    commit_msg_filename: Bound<'_, PyAny>,
) -> Result<serde_json::Value, String> {
    let commit_msg_filename: serde_json::Value =
        crate::pyconv::FromPy::from_py(&commit_msg_filename)?;
    let out = port::git_commit_with_message_from_file(commit_msg_filename)?;
    Ok(out)
}

/// `gitCommitWithMessageFromFile`.
#[pyfunction]
#[pyo3(name = "gitCommitWithMessageFromFile", signature = (commit_msg_filename))]
fn git_commit_with_message_from_file(
    py: Python<'_>,
    commit_msg_filename: Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    git_commit_with_message_from_file_inner(commit_msg_filename)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(popen, m)?)?;
    m.add_function(wrap_pyfunction!(call, m)?)?;
    m.add_function(wrap_pyfunction!(execute, m)?)?;
    m.add_function(wrap_pyfunction!(git_log, m)?)?;
    m.add_function(wrap_pyfunction!(git_commit, m)?)?;
    m.add_function(wrap_pyfunction!(git_commit_with_message_from_file, m)?)?;
    Ok(())
}
