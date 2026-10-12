//! Runtime execution knobs of a definition: model selection
//! ([`ModelSpec`]), tool visibility ([`ToolScope`]) and sandboxing
//! ([`SandboxMode`]).

use serde::{Deserialize, Serialize};

/// Model selection for a sub-agent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelSpec {
    /// Use the parent agent's currently-selected model at spawn time.
    #[default]
    Inherit,
    /// Exact model name (e.g. `"neocortex-mk1"`).
    Exact(String),
    /// Router hint (e.g. `"reasoning"`, `"coding"`, `"local"`). Resolved
    /// to a real model by the routing provider.
    Hint(String),
}

impl ModelSpec {
    /// Resolve this spec into the model name string the provider expects.
    /// `parent_model` is the model the parent agent is using right now.
    ///
    /// Hints are resolved to the `hint:{hint}` role alias (e.g. `"agentic"` →
    /// `"hint:agentic"`), which the inference factory translates to the role's
    /// configured route — the managed default model, or the BYOK/local model
    /// routed to that workload. When a `RouterProvider` is present its route
    /// table takes priority over this default.
    pub fn resolve(&self, parent_model: &str) -> String {
        match self {
            Self::Inherit => parent_model.to_string(),
            Self::Exact(name) => name.clone(),
            Self::Hint(hint) => format!("hint:{hint}"),
        }
    }
}

/// Which tools a sub-agent is allowed to call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolScope {
    /// All tools the parent has (subject to `disallowed_tools` and
    /// `skill_filter`).
    #[default]
    Wildcard,
    /// An explicit allowlist of tool names. Names not present in the parent
    /// registry at spawn time are silently dropped (logged at debug).
    Named(Vec<String>),
}

/// The tools through which an agent spawns sub-agents of its own choosing.
/// A named scope that lists none of them delegates only to what its host
/// declared, never to an inherited roster.
pub const SPAWN_TOOL_NAMES: &[&str] = &[
    "spawn_subagent",
    "spawn_async_subagent",
    "spawn_parallel_agents",
    "spawn_worker_thread",
];

impl ToolScope {
    /// Whether this is a named scope that lists one of [`SPAWN_TOOL_NAMES`].
    pub fn names_a_spawn_tool(&self) -> bool {
        matches!(self, ToolScope::Named(names)
            if names.iter().any(|name| SPAWN_TOOL_NAMES.contains(&name.as_str())))
    }
}

/// Sandbox mode for a sub-agent's tool execution. Serialises as a simple
/// `snake_case` string in TOML (`none` / `read_only` / `sandboxed`). In
/// the future this may map directly into a `SecurityPolicy` builder.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// No additional sandboxing beyond what the parent already enforces.
    #[default]
    None,
    /// Read-only — write/execute tools are filtered out.
    ReadOnly,
    /// Drop privileges, restrict filesystem (Landlock / Bubblewrap).
    Sandboxed,
}

#[cfg(test)]
#[path = "execution_spec_tests.rs"]
mod tests;
