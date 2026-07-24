//! Hosted Medulla reflection for the memory subconscious.
//!
//! Observation and baseline ownership stay local. This module only submits the
//! prepared reflection prompt, advertises the bounded subconscious tool set,
//! and executes requested calls on-device.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::agent::turn_origin::{with_origin, AgentTurnOrigin, TrustedAutomationSource};
use crate::openhuman::agent_orchestration::parent_context::with_root_parent;
use crate::openhuman::config::Config;
use crate::openhuman::orchestration::medulla::{self, MedullaRunError};
use crate::openhuman::tools::{Tool, ToolResult};

const HOSTED_FLAVOR: &str = "openhuman";

/// Execute a hosted reflection under the same trust and parent contexts as the
/// local subconscious decision agent.
pub(crate) async fn reflect(
    config: &Config,
    prompt: &str,
    has_external_content: bool,
) -> Result<usize, MedullaRunError> {
    let mut effective = config.clone();
    effective.autonomy.level = crate::openhuman::security::AutonomyLevel::Full;
    let tools = subconscious_tools(&effective);
    let source = if has_external_content {
        TrustedAutomationSource::SubconsciousTainted
    } else {
        TrustedAutomationSource::Subconscious
    };
    let origin = AgentTurnOrigin::TrustedAutomation {
        job_id: format!("subconscious:hosted:{}", uuid::Uuid::new_v4()),
        source,
    };

    let scoped = with_origin(
        origin,
        with_root_parent(
            &effective,
            "subconscious",
            "subconscious",
            "subconscious-hosted",
            medulla::run_scoped(&effective, prompt, None, Some(HOSTED_FLAVOR), &tools),
        ),
    )
    .await
    .map_err(|err| MedullaRunError::preflight(format!("build subconscious parent: {err:#}")))?;
    let result = scoped?;
    Ok(result.reply.chars().count())
}

pub(crate) fn subconscious_tools(config: &Config) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(super::user_thread::NotifyUserTool),
        Box::new(crate::openhuman::agent::tools::UpdateTaskTool::new()),
        Box::new(crate::openhuman::memory_goals::GoalsListTool::new(
            config.workspace_dir.clone(),
        )),
        Box::new(crate::openhuman::memory_goals::GoalsAddTool::new(
            config.workspace_dir.clone(),
        )),
        Box::new(crate::openhuman::memory_goals::GoalsEditTool::new(
            config.workspace_dir.clone(),
        )),
        Box::new(BlockingSpawnSubagentTool::new(Box::new(
            crate::openhuman::agent_orchestration::tools::SpawnSubagentTool::new(),
        ))),
    ]
}

/// Headless subconscious ticks have no delivery thread. Force delegation to
/// complete inline so its result returns through the Medulla continuation.
struct BlockingSpawnSubagentTool {
    inner: Box<dyn Tool>,
}

impl BlockingSpawnSubagentTool {
    fn new(inner: Box<dyn Tool>) -> Self {
        Self { inner }
    }

    fn force_blocking(args: Value) -> Value {
        let mut object = args.as_object().cloned().unwrap_or_default();
        object.insert("blocking".to_string(), Value::Bool(true));
        Value::Object(object)
    }
}

#[async_trait]
impl Tool for BlockingSpawnSubagentTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        "Delegate deeper research or multi-step work to a specialised sub-agent. \
         This background workflow always waits for the sub-agent and returns its \
         result inline."
    }

    fn parameters_schema(&self) -> Value {
        let mut schema = self.inner.parameters_schema();
        if let Some(properties) = schema.get_mut("properties").and_then(Value::as_object_mut) {
            properties.insert(
                "blocking".to_string(),
                json!({
                    "type": "boolean",
                    "const": true,
                    "description": "Always true for subconscious delegation."
                }),
            );
        }
        schema
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.inner.execute(Self::force_blocking(args)).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct CaptureTool(Arc<Mutex<Option<Value>>>);

    #[async_trait]
    impl Tool for CaptureTool {
        fn name(&self) -> &str {
            "spawn_subagent"
        }

        fn description(&self) -> &str {
            "capture"
        }

        fn parameters_schema(&self) -> Value {
            json!({"type":"object","properties":{"blocking":{"type":"boolean"}}})
        }

        async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
            *self.0.lock().unwrap() = Some(args);
            Ok(ToolResult::success("captured"))
        }
    }

    #[test]
    fn hosted_catalog_is_exactly_the_subconscious_action_surface() {
        let config = Config::default();
        let names: Vec<String> = subconscious_tools(&config)
            .iter()
            .map(|tool| tool.name().to_string())
            .collect();
        assert_eq!(
            names,
            [
                "notify_user",
                "update_task",
                "goals_list",
                "goals_add",
                "goals_edit",
                "spawn_subagent",
            ]
        );
    }

    #[tokio::test]
    async fn subagent_wrapper_forces_blocking_execution() {
        let captured = Arc::new(Mutex::new(None));
        let tool = BlockingSpawnSubagentTool::new(Box::new(CaptureTool(Arc::clone(&captured))));

        tool.execute(json!({"agent_id":"researcher","blocking":false}))
            .await
            .unwrap();

        assert_eq!(captured.lock().unwrap().as_ref().unwrap()["blocking"], true);
        assert_eq!(
            tool.parameters_schema()["properties"]["blocking"]["const"],
            true
        );
    }
}
