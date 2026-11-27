//! Python bindings for `src/serena/lsp/`: generated from the signatures in `port.rs` by rust-anything.
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

fn grouped_diagnostics_new_inner() -> Result<port::GroupedDiagnostics, String> {
    let out = port::GroupedDiagnostics::new()?;
    Ok(out)
}

fn grouped_diagnostics_add_inner(
    inner: &mut port::GroupedDiagnostics,
    relative_path: String,
    name_path: String,
    diagnostic: Bound<'_, PyAny>,
) -> Result<(), String> {
    let diagnostic: serde_json::Value = crate::pyconv::FromPy::from_py(&diagnostic)?;
    inner.add(relative_path, name_path, diagnostic)?;
    Ok(())
}

fn grouped_diagnostics_get_dict_inner(
    inner: &mut port::GroupedDiagnostics,
) -> Result<serde_json::Value, String> {
    let out = inner.get_dict()?;
    Ok(out)
}

/// Class `GroupedDiagnostics`.
#[pyclass(name = "GroupedDiagnostics")]
pub struct GroupedDiagnostics {
    inner: port::GroupedDiagnostics,
}

#[pymethods]
impl GroupedDiagnostics {
    #[new]
    #[pyo3(signature = ())]
    fn new() -> PyResult<Self> {
        grouped_diagnostics_new_inner()
            .map(|inner| Self { inner })
            .map_err(error)
    }

    #[pyo3(name = "add", signature = (relative_path, name_path, diagnostic))]
    fn add(
        &mut self,
        relative_path: String,
        name_path: String,
        diagnostic: Bound<'_, PyAny>,
    ) -> PyResult<()> {
        grouped_diagnostics_add_inner(&mut self.inner, relative_path, name_path, diagnostic)
            .map_err(error)
    }

    #[pyo3(name = "get_dict", signature = ())]
    fn get_dict(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        grouped_diagnostics_get_dict_inner(&mut self.inner)
            .map_err(error)
            .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
    }
}

fn diagnostics_diff_new_inner(
    before_snapshot: Bound<'_, PyAny>,
    edited_files: Bound<'_, PyAny>,
    symbol_retriever: Bound<'_, PyAny>,
) -> Result<port::DiagnosticsDiff, String> {
    let before_snapshot: serde_json::Value = crate::pyconv::FromPy::from_py(&before_snapshot)?;
    let edited_files: Vec<serde_json::Value> = crate::pyconv::FromPy::from_py(&edited_files)?;
    let symbol_retriever: serde_json::Value = crate::pyconv::FromPy::from_py(&symbol_retriever)?;
    let out = port::DiagnosticsDiff::new(before_snapshot, edited_files, symbol_retriever)?;
    Ok(out)
}

fn diagnostics_diff_get_grouped_diagnostics_inner(
    inner: &mut port::DiagnosticsDiff,
) -> Result<serde_json::Value, String> {
    let out = inner.get_grouped_diagnostics()?;
    Ok(out)
}

/// Class `DiagnosticsDiff`.
#[pyclass(name = "DiagnosticsDiff")]
pub struct DiagnosticsDiff {
    inner: port::DiagnosticsDiff,
}

#[pymethods]
impl DiagnosticsDiff {
    #[new]
    #[pyo3(signature = (before_snapshot, edited_files, symbol_retriever))]
    fn new(
        before_snapshot: Bound<'_, PyAny>,
        edited_files: Bound<'_, PyAny>,
        symbol_retriever: Bound<'_, PyAny>,
    ) -> PyResult<Self> {
        diagnostics_diff_new_inner(before_snapshot, edited_files, symbol_retriever)
            .map(|inner| Self { inner })
            .map_err(error)
    }

    #[pyo3(name = "get_grouped_diagnostics", signature = ())]
    fn get_grouped_diagnostics(&mut self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        diagnostics_diff_get_grouped_diagnostics_inner(&mut self.inner)
            .map_err(error)
            .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
    }
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<GroupedDiagnostics>()?;
    m.add_class::<DiagnosticsDiff>()?;
    Ok(())
}
