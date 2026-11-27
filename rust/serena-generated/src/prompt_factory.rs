//! Port of `src/serena/generated/generated_prompt_factory.py`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Typed identifier for each generated prompt in `generated_prompt_factory.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PromptId {
    InfoJetBrainsDebugRepl,
    OnboardingPrompt,
    ConnectionPrompt,
    SystemPrompt,
    CcSystemPromptOverride,
}

impl PromptId {
    /// Returns the prompt template key used by `PromptFactoryBase`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InfoJetBrainsDebugRepl => "info_jet_brains_debug_repl",
            Self::OnboardingPrompt => "onboarding_prompt",
            Self::ConnectionPrompt => "connection_prompt",
            Self::SystemPrompt => "system_prompt",
            Self::CcSystemPromptOverride => "cc_system_prompt_override",
        }
    }

    /// Returns the sorted list of required keyword parameters for rendering this prompt.
    pub const fn parameters(self) -> &'static [&'static str] {
        match self {
            Self::InfoJetBrainsDebugRepl
            | Self::ConnectionPrompt
            | Self::CcSystemPromptOverride => &[],
            Self::OnboardingPrompt => &["memory_maintenance_name", "system"],
            Self::SystemPrompt => &[
                "available_markers",
                "available_tools",
                "context_system_prompt",
                "global_memories_list",
                "mode_system_prompts",
                "tool_names",
            ],
        }
    }
}

/// Metadata entry describing a prompt exposed by [`PromptFactory`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptDescriptor {
    pub id: PromptId,
    pub name: &'static str,
    pub parameters: &'static [&'static str],
}

/// Ordered catalog of all generated prompts in `src/serena/generated/generated_prompt_factory.py`.
pub const GENERATED_PROMPTS: &[PromptDescriptor] = &[
    PromptDescriptor {
        id: PromptId::InfoJetBrainsDebugRepl,
        name: PromptId::InfoJetBrainsDebugRepl.as_str(),
        parameters: PromptId::InfoJetBrainsDebugRepl.parameters(),
    },
    PromptDescriptor {
        id: PromptId::OnboardingPrompt,
        name: PromptId::OnboardingPrompt.as_str(),
        parameters: PromptId::OnboardingPrompt.parameters(),
    },
    PromptDescriptor {
        id: PromptId::ConnectionPrompt,
        name: PromptId::ConnectionPrompt.as_str(),
        parameters: PromptId::ConnectionPrompt.parameters(),
    },
    PromptDescriptor {
        id: PromptId::SystemPrompt,
        name: PromptId::SystemPrompt.as_str(),
        parameters: PromptId::SystemPrompt.parameters(),
    },
    PromptDescriptor {
        id: PromptId::CcSystemPromptOverride,
        name: PromptId::CcSystemPromptOverride.as_str(),
        parameters: PromptId::CcSystemPromptOverride.parameters(),
    },
];

/// Parameter value passed into a prompt template renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptValue {
    String(String),
    List(Vec<String>),
    Map(BTreeMap<String, String>),
}

impl From<&str> for PromptValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for PromptValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<Vec<String>> for PromptValue {
    fn from(value: Vec<String>) -> Self {
        Self::List(value)
    }
}

impl From<BTreeMap<String, String>> for PromptValue {
    fn from(value: BTreeMap<String, String>) -> Self {
        Self::Map(value)
    }
}

impl From<BTreeSet<String>> for PromptValue {
    fn from(value: BTreeSet<String>) -> Self {
        Self::List(value.into_iter().collect())
    }
}

/// Errors returned by [`PromptBackend`] and [`PromptFactory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptError {
    NotFound(String),
    RenderFailed(String),
}

impl fmt::Display for PromptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "prompt template not found: {name}"),
            Self::RenderFailed(msg) => write!(f, "failed to render prompt template: {msg}"),
        }
    }
}

impl std::error::Error for PromptError {}

/// Backend interface equivalent to Python's `PromptFactoryBase` methods used by `PromptFactory`.
pub trait PromptBackend {
    /// Returns the raw template string for `prompt_name`.
    fn get_prompt_template_string(&self, prompt_name: &str) -> Result<String, PromptError>;

    /// Renders `prompt_name` with the supplied parameter map.
    fn render_prompt(
        &self,
        prompt_name: &str,
        params: &BTreeMap<String, PromptValue>,
    ) -> Result<String, PromptError>;
}

/// Parameters for [`PromptFactory::create_onboarding_prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingPromptParams {
    pub memory_maintenance_name: PromptValue,
    pub system: PromptValue,
}

impl OnboardingPromptParams {
    pub fn into_map(self) -> BTreeMap<String, PromptValue> {
        let mut params = BTreeMap::new();
        params.insert(
            "memory_maintenance_name".to_string(),
            self.memory_maintenance_name,
        );
        params.insert("system".to_string(), self.system);
        params
    }
}

/// Parameters for [`PromptFactory::create_system_prompt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemPromptParams {
    pub available_markers: PromptValue,
    pub available_tools: PromptValue,
    pub context_system_prompt: PromptValue,
    pub global_memories_list: PromptValue,
    pub mode_system_prompts: PromptValue,
    pub tool_names: PromptValue,
}

impl SystemPromptParams {
    pub fn into_map(self) -> BTreeMap<String, PromptValue> {
        let mut params = BTreeMap::new();
        params.insert("available_markers".to_string(), self.available_markers);
        params.insert("available_tools".to_string(), self.available_tools);
        params.insert(
            "context_system_prompt".to_string(),
            self.context_system_prompt,
        );
        params.insert(
            "global_memories_list".to_string(),
            self.global_memories_list,
        );
        params.insert("mode_system_prompts".to_string(), self.mode_system_prompts);
        params.insert("tool_names".to_string(), self.tool_names);
        params
    }
}

/// Generated prompt factory wrapping a [`PromptBackend`].
///
/// Mirrors `PromptFactory(PromptFactoryBase)` in `src/serena/generated/generated_prompt_factory.py`.
#[derive(Debug, Clone)]
pub struct PromptFactory<B> {
    backend: B,
}

impl<B> PromptFactory<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn into_backend(self) -> B {
        self.backend
    }
}

impl<B: PromptBackend> PromptFactory<B> {
    pub fn get_info_jet_brains_debug_repl_template_string(&self) -> Result<String, PromptError> {
        self.backend
            .get_prompt_template_string(PromptId::InfoJetBrainsDebugRepl.as_str())
    }

    pub fn create_info_jet_brains_debug_repl(&self) -> Result<String, PromptError> {
        self.backend
            .render_prompt(PromptId::InfoJetBrainsDebugRepl.as_str(), &BTreeMap::new())
    }

    pub fn get_onboarding_prompt_template_string(&self) -> Result<String, PromptError> {
        self.backend
            .get_prompt_template_string(PromptId::OnboardingPrompt.as_str())
    }

    pub fn create_onboarding_prompt(
        &self,
        params: OnboardingPromptParams,
    ) -> Result<String, PromptError> {
        self.backend
            .render_prompt(PromptId::OnboardingPrompt.as_str(), &params.into_map())
    }

    pub fn get_connection_prompt_template_string(&self) -> Result<String, PromptError> {
        self.backend
            .get_prompt_template_string(PromptId::ConnectionPrompt.as_str())
    }

    pub fn create_connection_prompt(&self) -> Result<String, PromptError> {
        self.backend
            .render_prompt(PromptId::ConnectionPrompt.as_str(), &BTreeMap::new())
    }

    pub fn get_system_prompt_template_string(&self) -> Result<String, PromptError> {
        self.backend
            .get_prompt_template_string(PromptId::SystemPrompt.as_str())
    }

    pub fn create_system_prompt(&self, params: SystemPromptParams) -> Result<String, PromptError> {
        self.backend
            .render_prompt(PromptId::SystemPrompt.as_str(), &params.into_map())
    }

    pub fn get_cc_system_prompt_override_template_string(&self) -> Result<String, PromptError> {
        self.backend
            .get_prompt_template_string(PromptId::CcSystemPromptOverride.as_str())
    }

    pub fn create_cc_system_prompt_override(&self) -> Result<String, PromptError> {
        self.backend
            .render_prompt(PromptId::CcSystemPromptOverride.as_str(), &BTreeMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct RecordingBackend {
        templates: BTreeMap<String, String>,
        calls: RefCell<Vec<(String, BTreeMap<String, PromptValue>)>>,
    }

    impl RecordingBackend {
        fn with_defaults() -> Self {
            let mut templates = BTreeMap::new();
            for desc in GENERATED_PROMPTS {
                templates.insert(desc.name.to_string(), format!("tpl:{}", desc.name));
            }
            Self {
                templates,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl PromptBackend for RecordingBackend {
        fn get_prompt_template_string(&self, prompt_name: &str) -> Result<String, PromptError> {
            self.templates
                .get(prompt_name)
                .cloned()
                .ok_or_else(|| PromptError::NotFound(prompt_name.to_string()))
        }

        fn render_prompt(
            &self,
            prompt_name: &str,
            params: &BTreeMap<String, PromptValue>,
        ) -> Result<String, PromptError> {
            if !self.templates.contains_key(prompt_name) {
                return Err(PromptError::NotFound(prompt_name.to_string()));
            }
            self.calls
                .borrow_mut()
                .push((prompt_name.to_string(), params.clone()));
            Ok(format!("rendered:{prompt_name}:{}", params.len()))
        }
    }

    #[test]
    fn template_getters_and_zero_arg_creators_dispatch_expected_names() {
        let factory = PromptFactory::new(RecordingBackend::with_defaults());

        assert_eq!(
            factory
                .get_info_jet_brains_debug_repl_template_string()
                .unwrap(),
            "tpl:info_jet_brains_debug_repl"
        );
        assert_eq!(
            factory.create_info_jet_brains_debug_repl().unwrap(),
            "rendered:info_jet_brains_debug_repl:0"
        );

        assert_eq!(
            factory.get_connection_prompt_template_string().unwrap(),
            "tpl:connection_prompt"
        );
        assert_eq!(
            factory.create_connection_prompt().unwrap(),
            "rendered:connection_prompt:0"
        );

        assert_eq!(
            factory
                .get_cc_system_prompt_override_template_string()
                .unwrap(),
            "tpl:cc_system_prompt_override"
        );
        assert_eq!(
            factory.create_cc_system_prompt_override().unwrap(),
            "rendered:cc_system_prompt_override:0"
        );
    }

    #[test]
    fn onboarding_and_system_prompts_pass_exact_parameter_keys() {
        let factory = PromptFactory::new(RecordingBackend::with_defaults());

        assert_eq!(
            factory.get_onboarding_prompt_template_string().unwrap(),
            "tpl:onboarding_prompt"
        );
        let onboarding = factory
            .create_onboarding_prompt(OnboardingPromptParams {
                memory_maintenance_name: "memory_maintenance".into(),
                system: "Linux".into(),
            })
            .unwrap();
        assert_eq!(onboarding, "rendered:onboarding_prompt:2");

        assert_eq!(
            factory.get_system_prompt_template_string().unwrap(),
            "tpl:system_prompt"
        );
        let mut tool_names = BTreeMap::new();
        tool_names.insert("find_symbol".to_string(), "find_symbol".to_string());
        let available_markers: BTreeSet<String> =
            ["ToolMarkerSymbolicRead".to_string()].into_iter().collect();
        let available_tools: BTreeSet<String> = ["find_symbol".to_string()].into_iter().collect();
        let system = factory
            .create_system_prompt(SystemPromptParams {
                available_markers: available_markers.into(),
                available_tools: available_tools.into(),
                context_system_prompt: "ctx".into(),
                global_memories_list: "".into(),
                mode_system_prompts: vec!["interactive".to_string()].into(),
                tool_names: tool_names.into(),
            })
            .unwrap();
        assert_eq!(system, "rendered:system_prompt:6");

        let calls = factory.backend().calls.borrow();
        let onboarding_keys: Vec<_> = calls[0].1.keys().map(String::as_str).collect();
        assert_eq!(onboarding_keys, PromptId::OnboardingPrompt.parameters());

        let system_keys: Vec<_> = calls[1].1.keys().map(String::as_str).collect();
        assert_eq!(system_keys, PromptId::SystemPrompt.parameters());
    }

    #[test]
    fn missing_template_propagates_not_found_error() {
        let factory = PromptFactory::new(RecordingBackend::default());
        assert_eq!(
            factory.get_connection_prompt_template_string().unwrap_err(),
            PromptError::NotFound("connection_prompt".to_string())
        );
        assert_eq!(
            factory.create_connection_prompt().unwrap_err(),
            PromptError::NotFound("connection_prompt".to_string())
        );
    }

    #[test]
    fn generated_prompt_catalog_matches_python_module_order() {
        let names: Vec<_> = GENERATED_PROMPTS.iter().map(|d| d.name).collect();
        assert_eq!(
            names,
            vec![
                "info_jet_brains_debug_repl",
                "onboarding_prompt",
                "connection_prompt",
                "system_prompt",
                "cc_system_prompt_override",
            ]
        );

        let py_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../src/serena/generated/generated_prompt_factory.py");
        let py_src = std::fs::read_to_string(&py_path).expect("read generated_prompt_factory.py");

        let mut parsed_getters = Vec::new();
        let mut parsed_creators: Vec<(String, Vec<String>)> = Vec::new();
        let mut lines = py_src.lines().peekable();
        while let Some(line) = lines.next() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("def get_")
                && let Some(name) = rest.strip_suffix("_template_string(self) -> str:")
            {
                parsed_getters.push(name.to_string());
            } else if let Some(rest) = trimmed.strip_prefix("def create_") {
                let mut sig = rest.to_string();
                while !sig.contains(") -> str:") {
                    let next = lines.next().expect("unterminated create_ signature");
                    sig.push(' ');
                    sig.push_str(next.trim());
                }
                let (name, args_part) = sig.split_once('(').expect("signature paren");
                let args_inner = args_part.split(") -> str:").next().expect("return arrow");
                let mut params = Vec::new();
                for part in args_inner.split(',') {
                    let part = part.trim();
                    if part.is_empty() || part == "self" || part == "*" {
                        continue;
                    }
                    let param_name = part
                        .split(':')
                        .next()
                        .expect("param annotation")
                        .trim()
                        .to_string();
                    params.push(param_name);
                }
                parsed_creators.push((name.to_string(), params));
            }
        }

        assert_eq!(parsed_getters, names);
        assert_eq!(parsed_creators.len(), GENERATED_PROMPTS.len());
        for (desc, (py_name, py_params)) in GENERATED_PROMPTS.iter().zip(parsed_creators.iter()) {
            assert_eq!(desc.name, py_name);
            let rust_params: Vec<String> =
                desc.parameters.iter().map(|s| (*s).to_string()).collect();
            assert_eq!(
                &rust_params, py_params,
                "parameter mismatch for {}",
                desc.name
            );
        }
    }
}
