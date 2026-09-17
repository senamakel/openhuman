//! LLM-callable tools for the skill registry domain.
//!
//! These tools let the orchestrator (and other agents) browse the aggregated
//! Hermes catalog, search for skills, and install from catalog entries.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::config::Config;
use crate::tools::status::{NOT_FOUND_MARKER, UNSUPPORTED_MARKER};
use crate::tools::traits::{PermissionLevel, Tool, ToolResult};

use super::ops;

pub struct SkillRegistryBrowseTool;

#[async_trait]
impl Tool for SkillRegistryBrowseTool {
    fn name(&self) -> &str {
        "skill_registry_browse"
    }

    fn description(&self) -> &str {
        "Browse the aggregated skill catalog (HermesHub, ClawHub, skills.sh, \
         LobeHub, browse.sh). Returns all available skills with metadata. \
         Use `force_refresh: true` to bypass the cache."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "force_refresh": {
                    "type": "boolean",
                    "description": "Force re-fetch from the Hermes API, bypassing cache.",
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
        tracing::debug!(force_refresh = force, "[tool][skill_registry] browse");

        match ops::browse_catalog(force).await {
            Ok(entries) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "count": entries.len(),
                "entries": entries,
            }))?)),
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to browse skill catalog: {e}"
            ))),
        }
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

pub struct SkillRegistrySearchTool;

/// Matches returned per `skill_registry_search` call when the caller does not
/// say: enough candidates to compare and pick from in one read. A broad query
/// over the ~100k-entry catalog matches hundreds, and returning them all makes
/// every search a payload the harness has to summarize first (#6286).
const SEARCH_DEFAULT_LIMIT: usize = 20;
/// Largest page a caller may ask for.
const SEARCH_MAX_LIMIT: usize = 100;

#[async_trait]
impl Tool for SkillRegistrySearchTool {
    fn name(&self) -> &str {
        "skill_registry_search"
    }

    fn description(&self) -> &str {
        "Search available skills by keyword. Matches against name, description, \
         tags, category, and author. Optionally filter by source or category. \
         Returns one page of matches (`limit`, default 20) with the `total` \
         match count; pass `next_offset` as `offset` to read the next page."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query to match against skill name, description, tags, category, or author."
                },
                "source": {
                    "type": "string",
                    "description": "Filter by upstream source (e.g. 'ClawHub', 'skills.sh', 'built-in', 'LobeHub')."
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": SEARCH_MAX_LIMIT,
                    "default": SEARCH_DEFAULT_LIMIT,
                    "description": "Maximum number of matches to return."
                },
                "offset": {
                    "type": "integer",
                    "minimum": 0,
                    "default": 0,
                    "description": "Number of matches to skip. Pass `next_offset` from the previous page."
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let source_filter = args.get("source").and_then(|v| v.as_str());
        let category_filter = args.get("category").and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .map_or(SEARCH_DEFAULT_LIMIT, |n| {
                usize::try_from(n).map_or(SEARCH_MAX_LIMIT, |n| n.clamp(1, SEARCH_MAX_LIMIT))
            });
        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));

        tracing::debug!(
            query = %query,
            source = ?source_filter,
            category = ?category_filter,
            limit,
            offset,
            "[tool][skill_registry] search"
        );

        match ops::search_catalog(query, source_filter, category_filter).await {
            Ok(entries) => {
                let total = entries.len();
                let page: Vec<_> = entries.into_iter().skip(offset).take(limit).collect();
                let end = offset.saturating_add(page.len());
                let next_offset = (end < total).then_some(end);
                tracing::debug!(
                    total,
                    returned = page.len(),
                    next_offset = ?next_offset,
                    "[tool][skill_registry] search page"
                );
                Ok(ToolResult::success(serde_json::to_string(&json!({
                    "total": total,
                    "offset": offset,
                    "count": page.len(),
                    "next_offset": next_offset,
                    "entries": page,
                }))?))
            }
            Err(e) => Ok(ToolResult::error(format!(
                "Failed to search skill catalog: {e}"
            ))),
        }
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

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
        "Install a skill from the catalog by its entry_id. Downloads the \
         SKILL.md and installs it locally. Use `skill_registry_search` first \
         to find the entry to install."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "entry_id": {
                    "type": "string",
                    "description": "The `id` of the entry to install, exactly as returned by skill_registry_search (e.g. 'clawhub/apple-design')."
                }
            },
            "required": ["entry_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    /// Installing a skill mutates the user's local skill set and fetches a
    /// remote `SKILL.md`, so it routes through the process-global
    /// `ApprovalGate` (#3993). On an interactive chat turn the user sees an
    /// inline approval card and approves before anything is written; on
    /// background/cron turns (no `APPROVAL_CHAT_CONTEXT`) the gate is bypassed,
    /// matching every other external-effect tool.
    fn external_effect(&self) -> bool {
        true
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let entry_id = args
            .get("entry_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing required argument `entry_id`"))?;

        tracing::debug!(entry_id = %entry_id, "[tool][skill_registry] install");

        let catalog = ops::browse_catalog(false)
            .await
            .map_err(|e| anyhow::anyhow!("failed to load catalog: {e}"))?;

        let entry = ops::find_catalog_entry(&catalog, entry_id)
            .map_err(|e| anyhow::anyhow!("{NOT_FOUND_MARKER} {e}"))?;

        match ops::install_from_catalog(&self.workspace_dir, entry).await {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "url": outcome.url,
                "stdout": outcome.stdout,
                "stderr": outcome.stderr,
                "new_skills": outcome.new_skills,
            }))?)),
            Err(e) => {
                // Tagged from the entry, not sniffed from `e`: an entry with no
                // direct download can never install, whatever the message says.
                let tag = if entry.has_direct_download() {
                    String::new()
                } else {
                    format!("{UNSUPPORTED_MARKER} ")
                };
                Ok(ToolResult::error(format!(
                    "{tag}Failed to install skill '{entry_id}': {e}"
                )))
            }
        }
    }
}

pub struct SkillRegistrySourcesTool;

#[async_trait]
impl Tool for SkillRegistrySourcesTool {
    fn name(&self) -> &str {
        "skill_registry_sources"
    }

    fn description(&self) -> &str {
        "List the distinct upstream sources available in the catalog \
         (e.g. 'built-in', 'ClawHub', 'skills.sh', 'LobeHub', 'browse.sh')."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({ "type": "object", "properties": {} })
    }

    async fn execute(&self, _args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tracing::debug!("[tool][skill_registry] sources");
        match ops::list_sources().await {
            Ok(sources) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "count": sources.len(),
                "sources": sources,
            }))?)),
            Err(e) => Ok(ToolResult::error(format!("Failed to list sources: {e}"))),
        }
    }

    fn is_concurrency_safe(&self, _args: &serde_json::Value) -> bool {
        true
    }
}

pub struct SkillRegistryUninstallTool;

#[async_trait]
impl Tool for SkillRegistryUninstallTool {
    fn name(&self) -> &str {
        "skill_registry_uninstall"
    }

    fn description(&self) -> &str {
        "Uninstall an installed user-scope skill by slug. Use after listing \
         installed workflows or when the user asks to remove a skill."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Installed skill slug to remove."
                }
            },
            "required": ["name"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing required argument `name`"))?;
        tracing::debug!(name = %name, "[tool][skill_registry] uninstall");
        let params = crate::skills::ops_install::UninstallWorkflowParams {
            name: name.to_string(),
        };
        match crate::skills::ops_install::uninstall_workflow(params, None) {
            Ok(outcome) => Ok(ToolResult::success(serde_json::to_string(&json!({
                "name": outcome.name,
                "removed_path": outcome.removed_path,
                "scope": outcome.scope,
            }))?)),
            Err(error) => Ok(ToolResult::error(format!(
                "Failed to uninstall skill '{name}': {error}"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
