//! Debug helper that renders the exact system prompt a live session
//! would see for a given agent.
//!
//! Instead of re-implementing prompt assembly, this module routes
//! through [`Agent::from_config_for_agent`] — the same entry point the
//! Tauri web channel, CLI, and `welcome_proactive` all use — and then
//! calls [`Agent::build_system_prompt`] on the constructed session. The
//! output is byte-identical to what the LLM would receive on turn 1 of
//! that agent.
//!
//! Entry points:
//! * [`dump_agent_prompt`] — dump a single agent by id.
//! * [`dump_all_agent_prompts`] — dump every registered agent in one call.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use crate::openhuman::agent::harness::definition::AgentDefinitionRegistry;
use crate::openhuman::agent::harness::session::Agent;
use crate::openhuman::config::Config;
use crate::openhuman::context::prompt::LearnedContextData;
use crate::openhuman::tools::ToolCategory;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Inputs for [`dump_agent_prompt`].
#[derive(Debug, Clone)]
pub struct DumpPromptOptions {
    /// Target agent id (any id registered in [`AgentDefinitionRegistry`]).
    pub agent_id: String,
    /// Optional override for the workspace directory.
    pub workspace_dir_override: Option<PathBuf>,
    /// Optional override for the resolved model name.
    pub model_override: Option<String>,
}

impl DumpPromptOptions {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            workspace_dir_override: None,
            model_override: None,
        }
    }
}

/// Result of a single prompt dump.
#[derive(Debug, Clone)]
pub struct DumpedPrompt {
    /// Echoed from [`DumpPromptOptions::agent_id`].
    pub agent_id: String,
    /// Always `"session"` — dumps come from the live session path.
    pub mode: &'static str,
    /// Resolved model name.
    pub model: String,
    /// Workspace directory used for identity file injection.
    pub workspace_dir: PathBuf,
    /// The final rendered system prompt — frozen bytes that would be
    /// sent verbatim on every turn of a live session.
    pub text: String,
    /// Tool names that made it into the rendered prompt, in order.
    pub tool_names: Vec<String>,
    /// Number of `ToolCategory::Skill` tools in the dump.
    pub skill_tool_count: usize,
}

/// Render and return the system prompt for a single agent via the
/// real [`Agent::from_config_for_agent`] construction path.
pub async fn dump_agent_prompt(options: DumpPromptOptions) -> Result<DumpedPrompt> {
    let config = load_dump_config(
        options.workspace_dir_override.clone(),
        options.model_override.clone(),
    )
    .await?;

    // Ensure the registry is populated — `from_config_for_agent`
    // errors for any non-orchestrator id when the global registry
    // hasn't been initialised.
    AgentDefinitionRegistry::init_global(&config.workspace_dir)
        .context("initialising AgentDefinitionRegistry for prompt dump")?;

    render_via_session(&config, &options.agent_id).await
}

/// Dump every registered agent's system prompt in one shot.
///
/// The synthetic `fork` archetype is skipped (byte-stable replay, no
/// standalone prompt). Order follows [`AgentDefinitionRegistry::list`].
pub async fn dump_all_agent_prompts(
    workspace_dir_override: Option<PathBuf>,
    model_override: Option<String>,
) -> Result<Vec<DumpedPrompt>> {
    let config = load_dump_config(workspace_dir_override, model_override).await?;

    AgentDefinitionRegistry::init_global(&config.workspace_dir)
        .context("initialising AgentDefinitionRegistry for prompt dump")?;

    let registry = AgentDefinitionRegistry::global()
        .ok_or_else(|| anyhow!("AgentDefinitionRegistry missing after init"))?;

    let ids: Vec<String> = registry
        .list()
        .iter()
        .filter(|d| d.id != "fork")
        .map(|d| d.id.clone())
        .collect();

    let mut results = Vec::with_capacity(ids.len());
    for id in ids {
        let dumped = render_via_session(&config, &id)
            .await
            .with_context(|| format!("rendering prompt for agent `{id}`"))?;
        results.push(dumped);
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

async fn load_dump_config(
    workspace_dir_override: Option<PathBuf>,
    model_override: Option<String>,
) -> Result<Config> {
    let mut config = Config::load_or_init()
        .await
        .context("loading Config for prompt dump")?;
    config.apply_env_overrides();
    if let Some(override_dir) = workspace_dir_override {
        config.workspace_dir = override_dir;
    }
    std::fs::create_dir_all(&config.workspace_dir).ok();
    if let Some(model) = model_override {
        config.default_model = Some(model);
    }
    Ok(config)
}

/// Build a real [`Agent`] via `from_config_for_agent`, populate live
/// connected integrations, and render the turn-1 system prompt.
async fn render_via_session(config: &Config, agent_id: &str) -> Result<DumpedPrompt> {
    let mut agent = Agent::from_config_for_agent(config, agent_id)
        .with_context(|| format!("building session agent for `{agent_id}`"))?;

    // Match turn-1 behaviour: fetch the user's active Composio
    // connections so the rendered prompt mirrors what the LLM actually
    // sees. Best-effort — failures degrade to an empty integration
    // list, same as the live runtime.
    agent.fetch_connected_integrations().await;

    let text = agent
        .build_system_prompt(LearnedContextData::default())
        .with_context(|| format!("rendering system prompt for `{agent_id}`"))?;

    let tools = agent.tools();
    let tool_names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();
    let skill_tool_count = tools
        .iter()
        .filter(|t| t.category() == ToolCategory::Skill)
        .count();

    Ok(DumpedPrompt {
        agent_id: agent_id.to_string(),
        mode: "session",
        model: agent.model_name().to_string(),
        workspace_dir: agent.workspace_dir().to_path_buf(),
        text,
        tool_names,
        skill_tool_count,
    })
}
