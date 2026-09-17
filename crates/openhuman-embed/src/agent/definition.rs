//! What an agent *is*: prompt, tool scope, sandbox mode, iteration cap.
//!
//! A thin re-spelling of the core's
//! [`AgentDefinition`](openhuman_core::agent::harness::definition::AgentDefinition)
//! so the library surface does not track that thirty-field struct
//! field-by-field. The default is the built-in orchestrator's definition
//! under the agent's own id — the same dynamic prompt and delegation
//! surface the desktop's main agent runs with — except that **every
//! registered tool is visible** ([`ToolScopeSpec::Wildcard`]). The desktop
//! orchestrator names its direct tools and delegates the rest (MCP bridge,
//! integrations) to specialists; a library agent that declared an MCP
//! server expects to call it, so the host narrows with
//! [`AgentDefinitionSpec::tools`] rather than widening.

use openhuman_core::agent::harness::definition::{
    AgentDefinition, AgentDefinitionRegistry, PromptSource, SandboxMode, ToolScope,
};

use super::AgentError;

/// Which tools an agent may call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolScopeSpec {
    /// Every tool the runtime registered (subject to `disallow_tools`).
    Wildcard,
    /// Exactly these tool names; unknown names are dropped at build time.
    Named(Vec<String>),
}

/// How an agent's shell and file tools are confined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SandboxModeSpec {
    /// Only the access tier and path policy apply.
    #[default]
    None,
    /// Write and execute tools are filtered out.
    ReadOnly,
    /// Commands run under the platform jail or Docker backend.
    Sandboxed,
}

/// Builder for an agent's definition. See the module docs for the default.
#[derive(Debug, Clone, Default)]
pub struct AgentDefinitionSpec {
    system_prompt: Option<String>,
    tools: Option<ToolScopeSpec>,
    disallowed_tools: Vec<String>,
    sandbox: SandboxModeSpec,
    max_iterations: Option<usize>,
    temperature: Option<f64>,
    display_name: Option<String>,
    when_to_use: Option<String>,
}

impl AgentDefinitionSpec {
    /// The orchestrator's definition, to be narrowed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the dynamic system prompt with this fixed text.
    ///
    /// The identity, memory and safety sections the orchestrator prompt
    /// carries are kept around it; only the body changes.
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Restrict which tools the agent may call.
    pub fn tools(mut self, scope: ToolScopeSpec) -> Self {
        self.tools = Some(scope);
        self
    }

    /// Hide these tools even when the scope would include them.
    pub fn disallow_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.disallowed_tools
            .extend(tools.into_iter().map(Into::into));
        self
    }

    /// Confine the agent's shell and file tools.
    pub fn sandbox(mut self, mode: SandboxModeSpec) -> Self {
        self.sandbox = mode;
        self
    }

    /// Cap the tool-call iterations per turn.
    pub fn max_iterations(mut self, n: usize) -> Self {
        self.max_iterations = Some(n);
        self
    }

    /// Default sampling temperature.
    pub fn temperature(mut self, t: f64) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Human-readable name shown in transcripts and delegation catalogs.
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// One-line description of when this agent should be used.
    pub fn when_to_use(mut self, text: impl Into<String>) -> Self {
        self.when_to_use = Some(text.into());
        self
    }

    /// Materialize the core definition for agent `id`.
    pub(crate) fn into_core(self, id: &str) -> Result<AgentDefinition, AgentError> {
        let registry = AgentDefinitionRegistry::builtins_only();
        let mut def = registry.get(ORCHESTRATOR_ID).cloned().ok_or_else(|| {
            AgentError::Invalid("built-in orchestrator definition is missing".to_string())
        })?;
        def.id = id.to_string();
        def.display_name = Some(self.display_name.unwrap_or_else(|| id.to_string()));
        if let Some(text) = self.when_to_use {
            def.when_to_use = text;
        }
        if let Some(prompt) = self.system_prompt {
            def.system_prompt = PromptSource::Inline(prompt);
        }
        def.tools = match self.tools.unwrap_or(ToolScopeSpec::Wildcard) {
            ToolScopeSpec::Wildcard => ToolScope::Wildcard,
            ToolScopeSpec::Named(names) => ToolScope::Named(names),
        };
        def.disallowed_tools.extend(self.disallowed_tools);
        def.sandbox_mode = match self.sandbox {
            SandboxModeSpec::None => SandboxMode::None,
            SandboxModeSpec::ReadOnly => SandboxMode::ReadOnly,
            SandboxModeSpec::Sandboxed => SandboxMode::Sandboxed,
        };
        if let Some(n) = self.max_iterations {
            def.max_iterations = n;
        }
        if let Some(t) = self.temperature {
            def.temperature = t;
        }
        log::debug!(
            "[embed][agent] definition id={id} prompt={} tools={:?} sandbox={:?} max_iterations={}",
            match &def.system_prompt {
                PromptSource::Inline(_) => "inline",
                PromptSource::File { .. } => "file",
                PromptSource::Dynamic(_) => "dynamic",
            },
            def.tools,
            def.sandbox_mode,
            def.max_iterations
        );
        Ok(def)
    }
}

/// The built-in definition every embedded agent starts from.
const ORCHESTRATOR_ID: &str = "orchestrator";

#[cfg(test)]
#[path = "definition_tests.rs"]
mod tests;
