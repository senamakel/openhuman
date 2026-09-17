//! Read-only resolvers on [`Config`]: memory-tree content root, per-workload
//! local-model routing, and exact agent model pins.

use std::path::PathBuf;

use super::output_language::output_language_directive;
use crate::config::schema::Config;

impl Config {
    /// Resolve the root directory where chunk `.md` files are stored.
    ///
    /// Resolution order:
    /// 1. `memory_tree.content_dir` if `Some`.
    /// 2. Default: `<workspace_dir>/memory_tree/content/`.
    ///
    /// This is the only place in the codebase that should compute the content
    /// root — all code that needs the path should call this method.
    pub fn memory_tree_content_root(&self) -> PathBuf {
        self.memory_tree
            .content_dir
            .clone()
            .unwrap_or_else(|| self.workspace_dir.join("memory_tree").join("content"))
    }

    /// Read the per-workload provider string and return the local model id
    /// when the workload is routed to Ollama.
    ///
    /// Recognised workload names:
    /// `"chat"`, `"reasoning"`, `"agentic"`, `"coding"`, `"memory"`, `"embeddings"`,
    /// `"heartbeat"`, `"learning"`, `"subconscious"`.
    ///
    /// Returns `None` when the provider isn't `"ollama:<model>"` (including
    /// when the field is unset, blank, `"cloud"`, or any other prefix).
    /// This is the single source of truth for "is this workload local?" —
    /// callers MUST NOT consult the legacy `local_ai.usage.*` booleans or
    /// `memory_tree.llm_backend`. Those fields are deprecated zombies kept
    /// for migration only.
    pub fn workload_local_model(&self, workload: &str) -> Option<String> {
        let raw = match workload {
            "chat" => self.chat_provider.as_deref(),
            "reasoning" => self.reasoning_provider.as_deref(),
            "agentic" => self.agentic_provider.as_deref(),
            "coding" => self.coding_provider.as_deref(),
            "vision" => self.vision_provider.as_deref(),
            "memory" => self.memory_provider.as_deref(),
            "embeddings" => self.embeddings_provider.as_deref(),
            "heartbeat" => self.heartbeat_provider.as_deref(),
            "learning" => self.learning_provider.as_deref(),
            "subconscious" => self.subconscious_provider.as_deref(),
            _ => None,
        }?;
        let trimmed = raw.trim();
        let model = trimmed.strip_prefix("ollama:")?.trim();
        if model.is_empty() {
            None
        } else {
            Some(model.to_string())
        }
    }

    /// `true` when `workload_local_model` returns `Some` for the named
    /// workload. Convenience wrapper for the common "do I dispatch
    /// locally?" branch.
    pub fn workload_uses_local(&self, workload: &str) -> bool {
        self.workload_local_model(workload).is_some()
    }

    /// Prompt directive for background LLM artifacts, if configured.
    pub fn output_language_directive(&self) -> Option<String> {
        output_language_directive(self.output_language.as_deref())
    }

    /// Resolve an exact model pin for an agent, if configured.
    ///
    /// Precedence is intentionally narrow and deterministic:
    /// 1. `orchestrator.model` when resolving the front-line orchestrator.
    /// 2. `[teams.<agent_id>]` entries, with `lead_model` used for agents
    ///    that can delegate and `agent_model` used for leaf workers.
    /// 3. Built-in aliases such as `[teams.research]` for `researcher` and
    ///    `[teams.code]` for `code_executor`, matching the issue examples.
    ///
    /// Empty strings are ignored so partially-written configs fall back to
    /// the existing auto-routing path.
    pub fn configured_agent_model(&self, agent_id: &str, is_team_lead: bool) -> Option<&str> {
        fn clean(model: Option<&str>) -> Option<&str> {
            model.map(str::trim).filter(|value| !value.is_empty())
        }

        let agent_id = agent_id.trim();
        if agent_id.is_empty() {
            return None;
        }

        if agent_id == "orchestrator" {
            if let Some(model) = clean(self.orchestrator.model.as_deref()) {
                return Some(model);
            }
        }

        if let Some(model) = self
            .teams
            .get(agent_id)
            .and_then(|team| team.model_for_role(is_team_lead))
        {
            return Some(model);
        }

        if let Some(stripped) = agent_id.strip_suffix("_agent") {
            if let Some(model) = self
                .teams
                .get(stripped)
                .and_then(|team| team.model_for_role(is_team_lead))
            {
                return Some(model);
            }
        }

        let aliases: &[&str] = match agent_id {
            "researcher" => &["research"],
            "code_executor" => &["code"],
            "tool_maker" | "tools_agent" => &["tools"],
            "integrations_agent" => &["integrations"],
            _ => &[],
        };

        aliases.iter().find_map(|alias| {
            self.teams
                .get(*alias)
                .and_then(|team| team.model_for_role(is_team_lead))
        })
    }
}
