//! Tool: `spawn_parallel_agents` — fan out independent sub-agent tasks.

use crate::openhuman::agent::harness::definition::AgentDefinitionRegistry;
#[cfg(test)]
use crate::openhuman::agent_orchestration::spawn_parallel_graph::with_ownership_boundary;
#[cfg(test)]
use crate::openhuman::agent_orchestration::spawn_parallel_graph::ParallelAgentLineage;
#[cfg(test)]
use crate::openhuman::agent_orchestration::spawn_parallel_graph::ParallelAgentResult;
#[cfg(test)]
use crate::openhuman::agent_orchestration::spawn_parallel_graph::ParallelAgentTask;
use crate::openhuman::agent_orchestration::spawn_parallel_graph::{
    format_spawn_parallel_success, run_spawn_parallel_graph, SpawnParallelGraphOutcome,
    SpawnParallelTaskValidationError,
};
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolResult};
use async_trait::async_trait;
use serde_json::json;

pub struct SpawnParallelAgentsTool;

impl SpawnParallelAgentsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SpawnParallelAgentsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for SpawnParallelAgentsTool {
    fn name(&self) -> &str {
        "spawn_parallel_agents"
    }

    fn description(&self) -> &str {
        "Run two or more independent sub-agent tasks concurrently and collect their results. \
         Use only when tasks have clear non-overlapping ownership or read-only scopes. Each task \
         has `{agent_id, prompt, context?, toolkit?, ownership?}`; include `ownership` for file, \
         module, or responsibility boundaries so workers do not overlap."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let agent_ids: Vec<String> = AgentDefinitionRegistry::global()
            .map(|reg| reg.list().iter().map(|d| d.id.clone()).collect())
            .unwrap_or_default();
        let agent_id_schema = if agent_ids.is_empty() {
            json!({ "type": "string" })
        } else {
            json!({ "type": "string", "enum": agent_ids })
        };
        json!({
            "type": "object",
            "required": ["tasks"],
            "properties": {
                "tasks": {
                    "type": "array",
                    "minItems": 2,
                    "items": {
                        "type": "object",
                        "required": ["agent_id", "prompt"],
                        "properties": {
                            "agent_id": agent_id_schema,
                            "prompt": { "type": "string" },
                            "context": { "type": "string" },
                            "toolkit": { "type": "string" },
                            "ownership": {
                                "type": "string",
                                "description": "Disjoint file/module/responsibility boundary for this worker."
                            },
                            "isolation": {
                                "type": "string",
                                "enum": ["none", "worktree"],
                                "description": "File-isolation strategy. `none` (default) shares the workspace; `worktree` gives this edit-capable worker its own git worktree checkout so parallel edits never collide. Use `worktree` only for edit-capable coding workers, not read-only ones."
                            },
                            "base_ref": {
                                "type": "string",
                                "enum": ["head", "fresh"],
                                "description": "For `isolation = worktree`: branch the worktree from current HEAD (`head`, default) or the repo's default branch (`fresh`)."
                            }
                        }
                    }
                }
            }
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        tracing::debug!("[spawn_parallel_agents] execute entry");
        let outcome = run_spawn_parallel_graph(args)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        match outcome {
            SpawnParallelGraphOutcome::Collected(collected) => Ok(ToolResult::success(
                format_spawn_parallel_success(&collected),
            )),
            SpawnParallelGraphOutcome::InvalidRequest(
                SpawnParallelTaskValidationError::MissingTasks(message),
            ) => {
                tracing::debug!("[spawn_parallel_agents] missing_tasks_parameter");
                Err(anyhow::anyhow!(message))
            }
            SpawnParallelGraphOutcome::InvalidRequest(
                SpawnParallelTaskValidationError::InvalidTasks(message),
            ) => {
                tracing::debug!(error = %message, "[spawn_parallel_agents] invalid_tasks_array");
                Err(anyhow::anyhow!(message))
            }
            SpawnParallelGraphOutcome::InvalidRequest(
                SpawnParallelTaskValidationError::Rejected(message),
            ) => {
                tracing::debug!("[spawn_parallel_agents] rejected_too_few_tasks");
                Ok(ToolResult::error(message))
            }
            SpawnParallelGraphOutcome::Rejected(message) => Ok(ToolResult::error(message)),
        }
    }
}

#[cfg(test)]
#[path = "spawn_parallel_agents_tests.rs"]
mod tests;
