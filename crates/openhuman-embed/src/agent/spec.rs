//! Everything a host says about one agent before it exists.
//!
//! An [`AgentSpec`] is pure data: it is applied by
//! [`Runtime::agent`](crate::Runtime::agent), in a fixed order, onto a clone
//! of the runtime's base config — access tier, provider model, MCP servers,
//! then the [`config`](AgentSpec::config) escape hatch last — and onto a
//! per-agent [`AgentProfile`](openhuman_core::agent::profiles::AgentProfile)
//! and [`AgentDefinitionSpec`].

use std::path::PathBuf;

use openhuman_core::agent::profiles::AgentProfile;
use openhuman_core::config::Config;
use openhuman_core::core::runtime::DomainSet;
use openhuman_core::security::TrustedAccess;
use openhuman_core::tools::toolpacks::ToolGroups;

use super::AgentDefinitionSpec;
use crate::harness::{Access, Provider};

/// The [`AgentSpec::config`] escape hatch, applied last onto the agent's config.
type ConfigEdit = Box<dyn FnOnce(&mut Config) + Send>;

/// Where [`AgentSpec::skills_dir`] bundles are copied.
#[cfg(feature = "skills")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillsDest {
    /// `<workspace>/personalities/<id>/skills/` — seen only by this agent.
    ProfileLocal,
    /// `<workspace>/skills/` — the legacy workspace root the one-agent
    /// [`Harness`](crate::Harness) always used; kept for its callers.
    WorkspaceLegacy,
}

/// Description of an agent to instantiate on a [`Runtime`](crate::Runtime).
pub struct AgentSpec {
    id: String,
    definition: AgentDefinitionSpec,
    system_prompt_suffix: Option<String>,
    provider: Option<Provider>,
    access: Option<Access>,
    tool_groups: Option<ToolGroups>,
    domains: Option<DomainSet>,
    allowed_tools: Option<Vec<String>>,
    allowed_skills: Option<Vec<String>>,
    #[cfg(feature = "mcp")]
    mcp_servers: Vec<crate::harness::McpServer>,
    #[cfg(feature = "skills")]
    skills_dir: Option<PathBuf>,
    #[cfg(feature = "skills")]
    skills_dest: SkillsDest,
    include_user_skills: bool,
    action_dir: Option<PathBuf>,
    trusted: Vec<(String, TrustedAccess)>,
    dedicated_memory: bool,
    config_fn: Option<ConfigEdit>,
}

impl AgentSpec {
    /// An agent named `id`, with every setting at the runtime's default.
    ///
    /// The id must match `^[a-z0-9][a-z0-9_-]{0,63}$` (checked at
    /// [`Runtime::agent`](crate::Runtime::agent)); it names the agent's
    /// directories and transcripts. Avoid the built-in ids (`orchestrator`,
    /// `summarizer`, …): the runtime-wide delegation catalog resolves those
    /// to the shipped definitions.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            definition: AgentDefinitionSpec::new(),
            system_prompt_suffix: None,
            provider: None,
            access: None,
            tool_groups: None,
            domains: None,
            allowed_tools: None,
            allowed_skills: None,
            #[cfg(feature = "mcp")]
            mcp_servers: Vec::new(),
            #[cfg(feature = "skills")]
            skills_dir: None,
            #[cfg(feature = "skills")]
            skills_dest: SkillsDest::ProfileLocal,
            include_user_skills: false,
            action_dir: None,
            trusted: Vec::new(),
            dedicated_memory: false,
            config_fn: None,
        }
    }

    /// The id given to [`new`](Self::new).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the agent is: prompt, tool scope, sandbox, iteration cap.
    pub fn definition(mut self, definition: AgentDefinitionSpec) -> Self {
        self.definition = definition;
        self
    }

    /// Shorthand for [`AgentDefinitionSpec::system_prompt`].
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.definition = self.definition.system_prompt(prompt);
        self
    }

    /// Text appended to the system prompt, after the definition's body.
    pub fn system_prompt_suffix(mut self, suffix: impl Into<String>) -> Self {
        self.system_prompt_suffix = Some(suffix.into());
        self
    }

    /// Which model answers and where the request goes. Unset means the
    /// runtime's default provider — the managed TinyHumans backend via the
    /// runtime's API key when no route was given.
    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Pin the model without changing the route.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        let provider = self.provider.take().unwrap_or_else(Provider::inherit);
        self.provider = Some(provider.model(model));
        self
    }

    /// What the agent is allowed to do. Unset means the runtime's default.
    pub fn access(mut self, access: Access) -> Self {
        self.access = Some(access);
        self
    }

    /// Narrow how tool groups are disclosed to this agent. Must not advertise
    /// a group the runtime withheld or turned off.
    pub fn tool_groups(mut self, groups: ToolGroups) -> Self {
        self.tool_groups = Some(groups);
        self
    }

    /// Narrow which domain families this agent sees. Must be a subset of the
    /// runtime's.
    pub fn domains(mut self, domains: DomainSet) -> Self {
        self.domains = Some(domains);
        self
    }

    /// Tool names this agent may see. Unset means every registered tool.
    pub fn allowed_tools<I, S>(mut self, tools: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_tools = Some(tools.into_iter().map(Into::into).collect());
        self
    }

    /// Skill ids this agent may list and run. Unset means every discovered
    /// skill.
    pub fn allowed_skills<I, S>(mut self, skills: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_skills = Some(skills.into_iter().map(Into::into).collect());
        self
    }

    /// Declare an MCP server this agent may call tools on. Call repeatedly to
    /// add several. Other agents on the runtime do not see it.
    #[cfg(feature = "mcp")]
    pub fn mcp(mut self, server: crate::harness::McpServer) -> Self {
        self.mcp_servers.push(server);
        self
    }

    /// Make the skill bundles in `dir` available to this agent alone.
    ///
    /// Copied into `<workspace>/personalities/<id>/skills/` — copied rather
    /// than linked because skill discovery rejects symlinked bundles.
    #[cfg(feature = "skills")]
    pub fn skills_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.skills_dir = Some(dir.into());
        self.skills_dest = SkillsDest::ProfileLocal;
        self
    }

    /// [`skills_dir`](Self::skills_dir), but into the workspace's shared
    /// `skills/` root. What the one-agent [`Harness`](crate::Harness) does.
    #[cfg(feature = "skills")]
    pub(crate) fn skills_dir_legacy(mut self, dir: PathBuf) -> Self {
        self.skills_dir = Some(dir);
        self.skills_dest = SkillsDest::WorkspaceLegacy;
        self
    }

    /// Also let this agent discover the operator's user-scope skills
    /// (`~/.openhuman/skills`, `~/.agents/skills`). Off by default: an
    /// embedded agent sees what its host installed, not what the machine's
    /// user did.
    pub fn include_user_skills(mut self, include: bool) -> Self {
        self.include_user_skills = include;
        self
    }

    /// The agent's read/write root for acting tools.
    ///
    /// Defaults to `<root>/agents/<id>/action` (or, on an inherited
    /// workspace, `<action_dir>/profiles/<id>`). Point it at the project the
    /// agent should work in — this is the directory whose contents it can
    /// change.
    pub fn action_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.action_dir = Some(dir.into());
        self
    }

    /// Grant access to a directory outside the action root. Convenience over
    /// [`Access::trust`].
    pub fn trust(mut self, path: impl Into<String>, access: TrustedAccess) -> Self {
        self.trusted.push((path.into(), access));
        self
    }

    /// Give this agent its own memory store and transcript tree
    /// (`memory-<id>`, `session_raw-<id>`) instead of the workspace's shared
    /// ones.
    ///
    /// Off by default. Transcripts are already kept apart without it — they
    /// are keyed by agent name and every turn resumes only its own thread —
    /// and a dedicated store is opened through the memory module, which a
    /// library runtime only has when its host preloads modules
    /// (`ServiceSet::memory_queue`). Without the module the open times out
    /// on every turn. Turn this on only when memory itself must not be
    /// shared between agents and the module is running.
    pub fn dedicated_memory(mut self, dedicated: bool) -> Self {
        self.dedicated_memory = dedicated;
        self
    }

    /// Arbitrary edits to the agent's config, applied last.
    ///
    /// The escape hatch for the config fields the spec does not model — not
    /// a way to bypass them. `config_path` is reset afterwards: credentials
    /// and the keyring resolve against it and every agent shares them.
    pub fn config(mut self, f: impl FnOnce(&mut Config) + Send + 'static) -> Self {
        self.config_fn = Some(Box::new(f));
        self
    }

    // ── accessors for the build step ─────────────────────────────────────

    pub(crate) fn into_parts(self) -> AgentSpecParts {
        AgentSpecParts {
            id: self.id,
            definition: self.definition,
            system_prompt_suffix: self.system_prompt_suffix,
            provider: self.provider,
            access: self.access,
            tool_groups: self.tool_groups,
            domains: self.domains,
            allowed_tools: self.allowed_tools,
            allowed_skills: self.allowed_skills,
            #[cfg(feature = "mcp")]
            mcp_servers: self.mcp_servers,
            #[cfg(feature = "skills")]
            skills_dir: self.skills_dir,
            #[cfg(feature = "skills")]
            skills_dest: self.skills_dest,
            include_user_skills: self.include_user_skills,
            action_dir: self.action_dir,
            trusted: self.trusted,
            dedicated_memory: self.dedicated_memory,
            config_fn: self.config_fn,
        }
    }
}

/// The spec's fields, destructured for [`super::build::instantiate`].
pub(crate) struct AgentSpecParts {
    pub(crate) id: String,
    pub(crate) definition: AgentDefinitionSpec,
    pub(crate) system_prompt_suffix: Option<String>,
    pub(crate) provider: Option<Provider>,
    pub(crate) access: Option<Access>,
    pub(crate) tool_groups: Option<ToolGroups>,
    pub(crate) domains: Option<DomainSet>,
    pub(crate) allowed_tools: Option<Vec<String>>,
    pub(crate) allowed_skills: Option<Vec<String>>,
    #[cfg(feature = "mcp")]
    pub(crate) mcp_servers: Vec<crate::harness::McpServer>,
    #[cfg(feature = "skills")]
    pub(crate) skills_dir: Option<PathBuf>,
    #[cfg(feature = "skills")]
    pub(crate) skills_dest: SkillsDest,
    pub(crate) include_user_skills: bool,
    pub(crate) action_dir: Option<PathBuf>,
    pub(crate) trusted: Vec<(String, TrustedAccess)>,
    pub(crate) dedicated_memory: bool,
    pub(crate) config_fn: Option<ConfigEdit>,
}

impl std::fmt::Debug for AgentSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The provider may carry a bearer; the config closure is opaque.
        f.debug_struct("AgentSpec")
            .field("id", &self.id)
            .field("access", &self.access)
            .field("action_dir", &self.action_dir)
            .field("dedicated_memory", &self.dedicated_memory)
            .finish_non_exhaustive()
    }
}

/// An [`AgentProfile`] for `id` with nothing enabled beyond the id itself.
pub(crate) fn blank_profile(id: &str) -> AgentProfile {
    AgentProfile {
        id: id.to_string(),
        name: id.to_string(),
        description: String::new(),
        agent_id: id.to_string(),
        model_override: None,
        temperature: None,
        system_prompt_suffix: None,
        allowed_tools: None,
        built_in: false,
        avatar_url: None,
        voice_id: None,
        soul_md: None,
        soul_md_path: None,
        composio_integrations: None,
        memory_sources: None,
        include_agent_conversations: true,
        allowed_skills: None,
        allowed_mcp_servers: None,
        memory_dir_suffix: None,
        is_master: false,
        sort_order: None,
        dedicated_memory: false,
        dedicated_workspace: false,
    }
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
