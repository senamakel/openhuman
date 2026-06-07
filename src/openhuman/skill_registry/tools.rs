//! LLM-callable tools for the skill registry domain.
//!
//! These tools let the orchestrator (and other agents) browse remote
//! registries, search for skills, and install from catalog entries.
//! They complement the existing `list_workflows` / `describe_workflow`
//! tools which operate on *already-installed* skills.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::openhuman::config::Config;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};

use super::ops;

/// Browse all enabled skill registries and return the combined catalog.
pub struct SkillRegistryBrowseTool;

#[async_trait]
impl Tool for SkillRegistryBrowseTool {
    fn name(&self) -> &str {
        "skill_registry_browse"
    }

    fn description(&self) -> &str {
        "Browse all enabled skill registries (OpenHuman, HermesHub, ClawHub) \
         and return a combined catalog of available skills. Each entry includes \
         id, name, description, source, format, and download URL. Use \
         `force_refresh: true` to bypass the cache."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "force_refresh": {
                    "type": "boolean",
                    "description": "Force re-fetch from all sources, bypassing cache.",
                    "default": false
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let force = args
            .get("force_refresh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        log::debug!("[tool][skill_registry] browse invoked, force_refresh={force}");

        match ops::browse_catalog(force).await {
            Ok(entries) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "count": entries.len(),
                "entries": entries,
            }))?)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to browse skill registries: {e}"
            ))),
        }
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Search the skill registry catalog by keyword.
pub struct SkillRegistrySearchTool;

#[async_trait]
impl Tool for SkillRegistrySearchTool {
    fn name(&self) -> &str {
        "skill_registry_search"
    }

    fn description(&self) -> &str {
        "Search available skills across all enabled registries by keyword. \
         Matches against name, description, tags, format, and author. \
         Optionally filter by format or source."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to match against skill name, description, tags, format, or author."
                },
                "format": {
                    "type": "string",
                    "description": "Filter by skill format (e.g. 'openhuman', 'agentskills', 'clawhub')."
                },
                "source": {
                    "type": "string",
                    "description": "Filter by registry source id (e.g. 'openhuman-community', 'hermeshub', 'clawhub')."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let format_filter = args.get("format").and_then(|v| v.as_str());
        let source_filter = args.get("source").and_then(|v| v.as_str());

        log::debug!(
            "[tool][skill_registry] search invoked, query={query:?}, format={format_filter:?}, source={source_filter:?}"
        );

        match ops::search_catalog(query, format_filter, source_filter).await {
            Ok(entries) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "count": entries.len(),
                "entries": entries,
            }))?)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to search skill registries: {e}"
            ))),
        }
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

/// Install a skill from the registry catalog.
pub struct SkillRegistryInstallTool {
    workspace_dir: PathBuf,
}

impl SkillRegistryInstallTool {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            workspace_dir: config.workspace_dir.clone(),
        }
    }
}

#[async_trait]
impl Tool for SkillRegistryInstallTool {
    fn name(&self) -> &str {
        "skill_registry_install"
    }

    fn description(&self) -> &str {
        "Install a skill from the registry catalog by entry_id and source_id. \
         Downloads the SKILL.md from the remote source and installs it locally. \
         Use `skill_registry_browse` or `skill_registry_search` first to find \
         the entry to install."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "entry_id": {
                    "type": "string",
                    "description": "The skill entry id (slug) to install."
                },
                "source_id": {
                    "type": "string",
                    "description": "The registry source id the entry belongs to."
                }
            },
            "required": ["entry_id", "source_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let entry_id = args
            .get("entry_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument `entry_id`"))?;
        let source_id = args
            .get("source_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument `source_id`"))?;

        log::debug!(
            "[tool][skill_registry] install invoked, entry_id={entry_id}, source_id={source_id}"
        );

        let catalog = ops::browse_catalog(false)
            .await
            .map_err(|e| anyhow::anyhow!("failed to load catalog: {e}"))?;

        let entry = catalog
            .iter()
            .find(|e| e.id == entry_id && e.source_id == source_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "skill '{entry_id}' not found in source '{source_id}'. \
                     Run skill_registry_browse first to refresh the catalog."
                )
            })?;

        match ops::install_from_catalog(&self.workspace_dir, entry).await {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "url": outcome.url,
                "stdout": outcome.stdout,
                "stderr": outcome.stderr,
                "new_skills": outcome.new_skills,
            }))?)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to install skill '{entry_id}': {e}"
            ))),
        }
    }
}

/// List the configured skill registry sources.
pub struct SkillRegistrySourcesTool;

#[async_trait]
impl Tool for SkillRegistrySourcesTool {
    fn name(&self) -> &str {
        "skill_registry_sources"
    }

    fn description(&self) -> &str {
        "List the configured skill registry sources (default + custom). \
         Returns each source's id, name, URL, kind, and enabled status."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][skill_registry] sources invoked");
        let sources = ops::list_sources();
        Ok(ToolResult::success(serde_json::to_string(&json!({
            "count": sources.len(),
            "sources": sources,
        }))?))
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}
