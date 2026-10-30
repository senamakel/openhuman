//! A turn that hits its one-model-call cap, so the harness closes it.
//!
//! The provider asks for the host tool `probe` once and then answers `done`.
//! The agent's cap is one model call, so the turn stops after the tool ran and
//! is closed by a tool-less call. The provider's `done` never passes the
//! close check, which leaves the turn's reply to the deterministic checkpoint
//! built from the close copy: both the instruction the model saw and the
//! reply the user gets come from the copy in force.

#![allow(dead_code)] // Each test target includes this module but runs some of it.

use crate::common;

use common::{
    chat_requests, offline_config, route, scripted_provider, stub_backend, tool_call_completion,
};
use openhuman_embed::{
    Agent, AgentDefinitionSpec, AgentSpec, HostTurnTools, Runtime, RuntimeBuilder, Tool,
    ToolScopeSpec, Workspace,
};
use serde_json::{json, Value};

/// A host tool the turn's provider calls; `file_write` is one the harness
/// keeps on the belt for the call before a turn's last.
struct Probe(&'static str);

#[async_trait::async_trait]
impl Tool for Probe {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "Perform a host operation"
    }
    fn parameters_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    async fn execute(&self, _: Value) -> anyhow::Result<tinytools::ToolResult> {
        Ok(tinytools::ToolResult::success("ran"))
    }
}

/// What one capped turn showed the model and sent back.
pub struct CappedTurn {
    /// Every chat request the provider received, as text.
    pub requests: Vec<String>,
    /// The reply the turn returned.
    pub reply: String,
}

impl CappedTurn {
    /// Whether any request the model received contains `needle`.
    pub fn model_saw(&self, needle: &str) -> bool {
        self.requests.iter().any(|body| body.contains(needle))
    }

    /// Whether the `index`th request the model received contains `needle`.
    pub fn request_has(&self, index: usize, needle: &str) -> bool {
        self.requests
            .get(index)
            .is_some_and(|body| body.contains(needle))
    }
}

/// A runtime over a stub backend, built from `configure`.
pub async fn runtime_with(configure: impl FnOnce(RuntimeBuilder) -> RuntimeBuilder) -> Runtime {
    let backend = stub_backend().await;
    openhuman_core::storage::install(std::sync::Arc::new(
        tinystoragedrivers_testkit::MemoryBackend::new(),
    ));
    configure(
        Runtime::builder()
            .config(offline_config())
            .workspace(Workspace::Ephemeral)
            .backend_url(backend.uri()),
    )
    .build()
    .await
    .expect("the runtime builds")
}

/// An agent that stops at one model call, answered by a provider whose
/// requests are read back through the returned server.
pub async fn capped_agent(runtime: &Runtime, id: &str) -> (Agent, wiremock::MockServer) {
    let provider = scripted_provider(vec![tool_call_completion("probe", "{}")], "done").await;
    let agent = runtime
        .agent(
            AgentSpec::new(id)
                .provider(route(&provider, "fixture"))
                .definition(
                    AgentDefinitionSpec::new()
                        .bare_prompt("Use probe then answer")
                        .tools(ToolScopeSpec::Named(vec!["probe".into()]))
                        .max_iterations(1),
                )
                .tools(|_| {
                    Box::pin(async { HostTurnTools::advertised(vec![Box::new(Probe("probe"))]) })
                }),
        )
        .expect("the agent is created");
    (agent, provider)
}

/// An agent allowed four model calls whose provider keeps calling tools until
/// the cap: `probe`, `probe`, then `file_write` on the call before the last.
/// The harness closes such a turn with its wrap-up instructions rather than
/// with the cap checkpoint, so the model's last call is a text answer.
pub async fn wrapped_up_agent(runtime: &Runtime, id: &str) -> (Agent, wiremock::MockServer) {
    let provider = scripted_provider(
        vec![
            tool_call_completion("probe", r#"{"n":1}"#),
            tool_call_completion("probe", r#"{"n":2}"#),
            tool_call_completion("file_write", r#"{"n":3}"#),
        ],
        "wrapped up",
    )
    .await;
    let agent = runtime
        .agent(
            AgentSpec::new(id)
                .provider(route(&provider, "fixture"))
                .definition(
                    AgentDefinitionSpec::new()
                        .bare_prompt("Use the tools then answer")
                        .tools(ToolScopeSpec::Named(vec![
                            "probe".into(),
                            "file_write".into(),
                        ]))
                        .max_iterations(4),
                )
                .tools(|_| {
                    Box::pin(async {
                        HostTurnTools::advertised(vec![
                            Box::new(Probe("probe")),
                            Box::new(Probe("file_write")),
                        ])
                    })
                }),
        )
        .expect("the agent is created");
    (agent, provider)
}

/// Run `turn` on a fresh thread and collect what the model saw.
pub async fn run(turn: openhuman_embed::Turn, provider: &wiremock::MockServer) -> CappedTurn {
    let outcome = turn.send().await.expect("the capped turn answers");
    let requests = chat_requests(provider)
        .await
        .iter()
        .map(|request| String::from_utf8_lossy(&request.body).into_owned())
        .collect();
    CappedTurn {
        requests,
        reply: outcome.reply,
    }
}
