use std::sync::Arc;
use std::time::Instant;

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::agent::host_runtime::{NativeRuntime, RuntimeAdapter};
use crate::openhuman::config::Config;
use crate::openhuman::memory::Memory;
use crate::openhuman::runtime_node::types::{ExecuteToolOutcome, RuntimeToolSummary};
use crate::openhuman::security::SecurityPolicy;
use crate::openhuman::tools::{self, Tool, ToolCallOptions, ToolScope};

fn tool_scope_label(scope: ToolScope) -> &'static str {
    match scope {
        ToolScope::All => "all",
        ToolScope::AgentOnly => "agent_only",
        ToolScope::CliRpcOnly => "cli_rpc_only",
    }
}

fn summarize_tool(tool: &dyn Tool) -> RuntimeToolSummary {
    RuntimeToolSummary {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        category: tool.category().to_string(),
        permission_level: tool.permission_level().to_string(),
        scope: tool_scope_label(tool.scope()).to_string(),
        supports_markdown: tool.supports_markdown(),
        parameters: tool.parameters_schema(),
    }
}

fn build_runtime_tools(config: &Config) -> Result<Vec<Box<dyn Tool>>, String> {
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let runtime: Arc<dyn RuntimeAdapter> = Arc::new(NativeRuntime::new());
    let local_embedding = config.workload_local_model("embeddings");
    let memory: Arc<dyn Memory> = Arc::from(
        crate::openhuman::memory::create_memory_with_local_ai(
            &config.memory,
            local_embedding.as_deref(),
            &config.embedding_routes,
            Some(&config.storage.provider.config),
            &config.workspace_dir,
        )
        .map_err(|error| error.to_string())?,
    );

    Ok(tools::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        memory,
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &config.agents,
        config,
    ))
}

pub fn list_tools(config: &Config) -> Result<Vec<RuntimeToolSummary>, String> {
    let mut summaries: Vec<RuntimeToolSummary> = build_runtime_tools(config)?
        .into_iter()
        .map(|tool| summarize_tool(tool.as_ref()))
        .collect();
    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(summaries)
}

pub async fn execute_tool(
    config: &Config,
    tool_name: &str,
    args: serde_json::Value,
    prefer_markdown: bool,
) -> Result<ExecuteToolOutcome, String> {
    let tools = build_runtime_tools(config)?;
    let tool = tools
        .into_iter()
        .find(|tool| tool.name() == tool_name)
        .ok_or_else(|| format!("unknown tool `{tool_name}`"))?;

    let started = Instant::now();
    publish_global(DomainEvent::ToolExecutionStarted {
        tool_name: tool_name.to_string(),
        session_id: "javascript".to_string(),
    });

    let execution = tool
        .execute_with_options(args, ToolCallOptions { prefer_markdown })
        .await
        .map_err(|error| format!("tool `{tool_name}` failed: {error:#}"));

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let success = execution
        .as_ref()
        .map(|result| !result.is_error)
        .unwrap_or(false);
    publish_global(DomainEvent::ToolExecutionCompleted {
        tool_name: tool_name.to_string(),
        session_id: "javascript".to_string(),
        success,
        elapsed_ms,
    });

    let result = execution?;
    Ok(ExecuteToolOutcome {
        tool_name: tool_name.to_string(),
        elapsed_ms,
        result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::json;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str {
            "dummy_tool"
        }

        fn description(&self) -> &str {
            "Dummy tool"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            _args: serde_json::Value,
        ) -> anyhow::Result<crate::openhuman::skills::types::ToolResult> {
            Ok(crate::openhuman::skills::types::ToolResult::success("ok"))
        }
    }

    #[test]
    fn summarize_tool_exposes_metadata() {
        let summary = summarize_tool(&DummyTool);
        assert_eq!(summary.name, "dummy_tool");
        assert_eq!(summary.category, "system");
        assert_eq!(summary.permission_level, "ReadOnly");
        assert_eq!(summary.scope, "all");
    }

    #[test]
    fn tool_scope_labels_are_stable() {
        assert_eq!(tool_scope_label(ToolScope::All), "all");
        assert_eq!(tool_scope_label(ToolScope::AgentOnly), "agent_only");
        assert_eq!(tool_scope_label(ToolScope::CliRpcOnly), "cli_rpc_only");
    }
}
