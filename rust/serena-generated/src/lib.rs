//! Rust port of `src/serena/generated/`.
//!
//! Provides the deterministic edit-capable tool manifest (`tool_capabilities`)
//! and the typed prompt factory surface (`prompt_factory`) mirroring
//! `src/serena/generated/tool_capabilities.py` and
//! `src/serena/generated/generated_prompt_factory.py`.

#![forbid(unsafe_code)]

pub mod prompt_factory;
pub mod tool_capabilities;

pub use prompt_factory::{
    GENERATED_PROMPTS, OnboardingPromptParams, PromptBackend, PromptDescriptor, PromptError,
    PromptFactory, PromptId, PromptValue, SystemPromptParams,
};
pub use tool_capabilities::{
    EDIT_CAPABLE_TOOL_NAMES, edit_capable_tool_names, is_edit_capable_tool,
    render_tool_capabilities_python_module,
};
