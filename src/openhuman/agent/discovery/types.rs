//! Shared types for the owner-discovery agent.

use serde::{Deserialize, Serialize};

/// What triggered a discovery run. Used for logging and to keep the
/// debounce-bypass path explicit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryTrigger {
    /// Fired automatically from `DomainEvent::SkillOAuthCompleted`.
    SkillOAuthCompleted {
        skill_id: String,
        integration_id: String,
    },
    /// Manually kicked off from an RPC / CLI command — bypasses debounce.
    Manual,
    /// Triggered from a test harness.
    Test,
}

/// A single seed hint surfaced to the discovery agent before the first
/// LLM call. Seeds are extracted from the skill's published state
/// snapshot at the moment OAuth completed (email, username, workspace
/// name, …) and rendered into the initial user-turn prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedFact {
    /// Short label the LLM sees, e.g. `"email"`, `"workspace_name"`.
    pub label: String,
    /// Value to show alongside the label.
    pub value: String,
}

impl SeedFact {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

/// A discovery run description.
#[derive(Debug, Clone)]
pub struct DiscoveryJob {
    pub trigger: DiscoveryTrigger,
    pub seed_facts: Vec<SeedFact>,
}

/// Summary returned from [`crate::openhuman::agent::discovery::run_discovery`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryReport {
    /// Number of structured facts the runner wrote to the profile table.
    pub facts_written: usize,
    /// Number of rich documents the runner wrote to the `owner` namespace.
    pub docs_written: usize,
    /// Number of LLM tool-calling rounds actually consumed.
    pub rounds_used: u32,
    /// Non-fatal errors encountered during the run (e.g. individual
    /// tool calls that failed but didn't prevent others from succeeding).
    pub errors: Vec<String>,
}

/// Errors that bubble out of the discovery runner itself. Individual
/// tool failures are *not* fatal and get captured in
/// [`DiscoveryReport::errors`] instead.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("discovery is disabled in config")]
    Disabled,
    #[error("discovery run was debounced (last run < debounce window)")]
    Debounced,
    #[error("no authenticated backend session — cannot run discovery")]
    NoSession,
    #[error("memory client is not initialized")]
    NoMemory,
    #[error("LLM call failed: {0}")]
    Llm(String),
    #[error("apify call failed: {0}")]
    Apify(String),
    #[error("discovery runner bug: {0}")]
    Internal(String),
}
