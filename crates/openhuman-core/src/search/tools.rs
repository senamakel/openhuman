//! Agent tools backed by the TinySearch module.
//!
//! Declarations come from `modules::search::configured_tool_specs`, which is
//! computed synchronously from config so a turn's tool list is stable and
//! needs no module round-trip. Each call re-reads the live config and goes
//! through `modules::search::execute_tool`, so a provider or login change is
//! honoured on the next call without rebuilding the session.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tinysearch_bus::{errors, ExecuteToolRequest, ToolSpec};
use tinytools::{Tool, ToolCallOptions, ToolCategory, ToolExposure, ToolResult};

use crate::config::Config;

/// One TinySearch tool (a role tool such as `web_search_tool`, or a provider
/// tool in `all_tools` presentation).
pub struct TinySearchTool {
    spec: ToolSpec,
    /// Spawn-time config. `None` for a deferred instance rebuilt from a
    /// recorded transcript, which resolves the live config per call.
    config: Option<Arc<Config>>,
    exposure: ToolExposure,
}

impl TinySearchTool {
    pub fn new(config: Arc<Config>, spec: ToolSpec) -> Self {
        Self {
            spec,
            config: Some(config),
            exposure: ToolExposure::Direct,
        }
    }

    /// A tool recorded in a resumed thread's transcript. It keeps the recorded
    /// declaration and resolves the live config on every call.
    pub fn recorded(spec: ToolSpec) -> Self {
        Self {
            spec,
            config: None,
            exposure: ToolExposure::Direct,
        }
    }

    pub fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    async fn live_config(&self) -> anyhow::Result<Config> {
        match &self.config {
            Some(config) => Ok(config.as_ref().clone()),
            None => crate::config::rpc::load_config_with_timeout()
                .await
                .map_err(|error| anyhow::anyhow!(error)),
        }
    }
}

/// Map a module error to a message the model can act on. The module prefixes
/// classified failures with `tinysearch.<code>:`; the detail after the prefix
/// may echo the query, so it is not logged.
pub fn user_facing_error(error: &str) -> String {
    match error_code(error) {
        Some(code) if code == errors::INSUFFICIENT_BALANCE => {
            "Web search is unavailable: the TinyHumans balance is too low for managed search. \
             Top up the balance, or add your own provider key under Connections → Search."
                .to_string()
        }
        Some(code) if code == errors::RATE_LIMITED => {
            "Web search is rate limited right now. Wait a moment and try again.".to_string()
        }
        Some(code) if code == errors::UNAVAILABLE => {
            "Every configured search provider for this request is unavailable right now."
                .to_string()
        }
        Some(code) if code == errors::INVALID_ARGUMENTS => {
            let marker = format!("{}{code}: ", errors::PREFIX);
            let detail = error
                .split_once(marker.as_str())
                .map(|(_, detail)| detail)
                .unwrap_or(error);
            format!("The search request was rejected: {detail}")
        }
        _ => format!("Web search failed: {error}"),
    }
}

/// The classified error code in a module error message, if any.
pub fn error_code(error: &str) -> Option<&'static str> {
    errors::code_of(error)
}

#[async_trait]
impl Tool for TinySearchTool {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn description(&self) -> &str {
        &self.spec.description
    }

    fn parameters_schema(&self) -> Value {
        self.spec.parameters.clone()
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Workflow
    }

    fn exposure(&self) -> ToolExposure {
        self.exposure
    }

    fn supports_markdown(&self) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_options(args, ToolCallOptions::default())
            .await
    }

    async fn execute_with_options(
        &self,
        args: Value,
        options: ToolCallOptions,
    ) -> anyhow::Result<ToolResult> {
        let config = self.live_config().await?;
        let subject = super::render::subject(&args);
        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(config.search.max_results)
            .clamp(1, 20);
        tracing::debug!(
            tool = %self.spec.name,
            subject_len = subject.chars().count(),
            "[search][tool] execute"
        );
        let request = ExecuteToolRequest {
            name: self.spec.name.clone(),
            arguments: args,
        };
        match crate::modules::search::execute_tool(&config, request).await {
            Ok(response) => {
                tracing::debug!(
                    tool = %self.spec.name,
                    provider = %response.provider,
                    fallbacks = response.fallback_from.len(),
                    results = response.results.len(),
                    "[search][tool] completed"
                );
                Ok(super::render::render(
                    &response,
                    &subject,
                    max_results,
                    options.prefer_markdown,
                ))
            }
            Err(error) => {
                tracing::warn!(
                    tool = %self.spec.name,
                    code = error_code(&error).unwrap_or("unclassified"),
                    "[search][tool] failed"
                );
                Ok(ToolResult::error(user_facing_error(&error)))
            }
        }
    }
}

/// Build the agent's search tools from config. Empty when search is off or no
/// provider is usable.
pub fn build_search_tools(config: &Config) -> Vec<Box<dyn Tool>> {
    let specs = crate::modules::search::configured_tool_specs(config);
    tracing::debug!(
        tools = ?specs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        "[search][tool] registered search tools"
    );
    let shared = Arc::new(config.clone());
    specs
        .into_iter()
        .map(|spec| Box::new(TinySearchTool::new(shared.clone(), spec)) as Box<dyn Tool>)
        .collect()
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
