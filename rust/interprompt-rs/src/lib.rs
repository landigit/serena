//! Python bindings for `src/interprompt/`: generated from the signatures in `port.rs` by rust-anything.
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

fn autogenerate_prompt_factory_module_inner(
    prompts_dir: String,
    target_module_path: String,
    interprompt_library_package: String,
    class_name: String,
) -> Result<(), String> {
    port::autogenerate_prompt_factory_module(
        prompts_dir,
        target_module_path,
        interprompt_library_package,
        class_name,
    )?;
    Ok(())
}

/// `autogenerate_prompt_factory_module`.
#[pyfunction]
#[pyo3(name = "autogenerate_prompt_factory_module", signature = (prompts_dir, target_module_path, interprompt_library_package=String::from("interprompt"), class_name=String::from("PromptFactory")))]
fn autogenerate_prompt_factory_module(
    prompts_dir: String,
    target_module_path: String,
    interprompt_library_package: String,
    class_name: String,
) -> PyResult<()> {
    autogenerate_prompt_factory_module_inner(
        prompts_dir,
        target_module_path,
        interprompt_library_package,
        class_name,
    )
    .map_err(error)
}

fn parameterized_template_interface_new_inner()
-> Result<port::ParameterizedTemplateInterface, String> {
    let out = port::ParameterizedTemplateInterface::new()?;
    Ok(out)
}

fn parameterized_template_interface_get_parameters_inner(
    inner: &mut port::ParameterizedTemplateInterface,
) -> Result<Vec<String>, String> {
    let out = inner.get_parameters()?;
    Ok(out)
}

/// Class `ParameterizedTemplateInterface`.
#[pyclass(name = "ParameterizedTemplateInterface")]
pub struct ParameterizedTemplateInterface {
    inner: port::ParameterizedTemplateInterface,
}

#[pymethods]
impl ParameterizedTemplateInterface {
    #[new]
    #[pyo3(signature = ())]
    fn new() -> PyResult<Self> {
        parameterized_template_interface_new_inner()
            .map(|inner| Self { inner })
            .map_err(error)
    }

    #[pyo3(name = "get_parameters", signature = ())]
    fn get_parameters(&mut self) -> PyResult<Vec<String>> {
        parameterized_template_interface_get_parameters_inner(&mut self.inner).map_err(error)
    }
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(autogenerate_prompt_factory_module, m)?)?;
    m.add_class::<ParameterizedTemplateInterface>()?;
    Ok(())
}
