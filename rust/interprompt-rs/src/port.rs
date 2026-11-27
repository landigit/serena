//! The Rust port of `src/interprompt/`.
//!
//! Replace each `unimplemented!` with the port of the source function. Return `Err` where
//! the source throws or raises: the caller gets an error there too. After changing a
//! signature, run `rust-anything refresh` to regenerate the bindings. `rust-anything diff` checks
//! behavior against what the source recorded.

#![forbid(unsafe_code)]

/// Port of `autogenerate_prompt_factory_module` (`src/interprompt/prompt_factory.py:45`).
///
/// `interprompt_library_package` defaults to `"interprompt"` in the source.
///
/// `class_name` defaults to `"PromptFactory"` in the source.
pub fn autogenerate_prompt_factory_module(
    prompts_dir: String,
    target_module_path: String,
    interprompt_library_package: String,
    class_name: String,
) -> Result<(), String> {
    let _ = (
        &prompts_dir,
        &target_module_path,
        &interprompt_library_package,
        &class_name,
    );
    unimplemented!(
        "port `autogenerate_prompt_factory_module` from src/interprompt/prompt_factory.py"
    )
}

/// Port of class `ParameterizedTemplateInterface` (`src/interprompt/jinja_template.py:14`): holds the instance state.
pub struct ParameterizedTemplateInterface {}

impl ParameterizedTemplateInterface {
    /// Port of `ParameterizedTemplateInterface` constructor (`src/interprompt/jinja_template.py:14`).
    pub fn new() -> Result<Self, String> {
        unimplemented!("port the `ParameterizedTemplateInterface` constructor")
    }

    /// Port of `ParameterizedTemplateInterface.get_parameters` (`src/interprompt/jinja_template.py:15`).
    pub fn get_parameters(&mut self) -> Result<Vec<String>, String> {
        unimplemented!("port `ParameterizedTemplateInterface.get_parameters`")
    }
}
