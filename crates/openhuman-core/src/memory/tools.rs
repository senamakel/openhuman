//! The single `memory` agent tool: `recall | fetch | learn | forget`.
//!
//! Registered only while memory is on (see [`crate::tools::ops`]). `learn`
//! stamps the stored learning with what the host knows and the model does
//! not get to choose:
//!
//! - `workspace` — the agent's working folder (`action_dir`, or the turn's
//!   isolated workspace when one is scoped);
//! - `thread_id` and `agent_id` (the memory agent id the turn runs as,
//!   [`super::scope`]);
//! - `namespace` — the layout's learnings node: the root of the acting
//!   identity's layout, which every agent under that root shares;
//! - `tool_call` — this call's name and provider-assigned id;
//! - `source.kind = agent`.
//!
//! `recall`, `fetch` and `forget` are confined to the identity's layout:
//! everything under its root, never another root's (another tenant's or
//! team's). A `reach` in the model's filter is overwritten.

use crate::tools::schema_cache::static_schema;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use tinymemory_api::{MemoryMeta, Namespace, Reach, SourceKind, SourceRef, ToolCallRef};
use tinytools::{PermissionLevel, Tool, ToolCallOptions, ToolResult, ToolRunContext};

use crate::config::Config;
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;

use super::error::MemoryError;
use super::ops;
use super::types::{FetchParams, ForgetParams, LearnParams, RecallParams, TurnCitation};

/// Most citations kept per thread between two drains.
const MAX_TURN_CITATIONS: usize = 20;

type TurnCitations = Mutex<HashMap<String, Vec<TurnCitation>>>;

/// Citations `recall` produced during a thread's in-flight turn, drained by
/// the chat surface once the turn returns ([`take_turn_citations`]).
fn turn_citations() -> Arc<TurnCitations> {
    crate::core::runtime::current_slot::<TurnCitations>()
}

fn record_turn_citations(thread_id: &str, citations: &[tinymemory_api::Citation]) {
    if citations.is_empty() {
        return;
    }
    let citations_by_thread = turn_citations();
    let mut all = citations_by_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = all.entry(thread_id.to_string()).or_default();
    for citation in citations {
        if entry.len() >= MAX_TURN_CITATIONS {
            break;
        }
        if !entry.iter().any(|existing| existing.id == citation.id.0) {
            entry.push(TurnCitation::from(citation));
        }
    }
}

/// Records `citations` for `thread_id`'s in-flight turn: what the turn's
/// memory pack cited (`lifecycle::hooks::pre_turn`), beside what `recall`
/// cites.
pub fn record_pack_citations(thread_id: &str, citations: Vec<TurnCitation>) {
    if citations.is_empty() {
        return;
    }
    let citations_by_thread = turn_citations();
    let mut all = citations_by_thread
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = all.entry(thread_id.to_string()).or_default();
    for citation in citations {
        if entry.len() >= MAX_TURN_CITATIONS {
            break;
        }
        if !entry.iter().any(|existing| existing.id == citation.id) {
            entry.push(citation);
        }
    }
}

/// Drains the citations `recall` and the turn's pack produced for
/// `thread_id` since the last drain.
#[must_use]
pub fn take_turn_citations(thread_id: &str) -> Vec<TurnCitation> {
    turn_citations()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(thread_id)
        .unwrap_or_default()
}

/// The tool's name.
pub const MEMORY_TOOL_NAME: &str = "memory";

/// The `memory` tool.
pub struct MemoryTool {
    config: Arc<Config>,
    budget: super::tool_budget::ToolBudget,
}

impl MemoryTool {
    /// A tool bound to `config`'s engine.
    #[must_use]
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            budget: super::tool_budget::ToolBudget::default(),
        }
    }
}

/// What the host knows about the call that the model does not set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallFacts {
    /// The agent's working folder.
    pub workspace: Option<String>,
    /// The thread the call runs in.
    pub thread_id: Option<String>,
    /// The agent that made the call.
    pub agent_id: Option<String>,
    /// The provider-assigned tool-call id.
    pub tool_call_id: Option<String>,
    /// Where the agent's learnings go: its layout's learnings node.
    pub namespace: Namespace,
    /// What the agent may read and forget: its whole layout.
    pub reach: Reach,
}

impl CallFacts {
    /// Gathers the facts from the run context and the turn's task-locals,
    /// falling back to `config.action_dir` for the workspace.
    #[must_use]
    pub fn gather(config: &Config, context: Option<&dyn ToolRunContext>) -> Self {
        let workspace = context
            .and_then(ToolRunContext::workspace_root)
            .map(std::path::Path::to_path_buf)
            .or_else(crate::agent::turn_workspace::current)
            .unwrap_or_else(|| config.action_dir.clone());
        let identity = super::scope::current().unwrap_or_else(|| {
            // Outside a scoped turn (a legacy driver), fall back to the
            // parent context's agent, else the root.
            crate::agent::harness::fork_context::current_parent()
                .map_or_else(super::scope::MemoryIdentity::root, |parent| {
                    super::scope::MemoryIdentity::agent(&parent.agent_definition_id)
                })
        });
        Self {
            workspace: Some(workspace.display().to_string()),
            thread_id: context
                .and_then(ToolRunContext::thread_id)
                .map(str::to_string),
            tool_call_id: crate::tools::host_extensions::tool_call_id(context),
            ..Self::of(config, &identity)
        }
    }

    /// The facts of a call made by `identity` under `config`, with no run
    /// context.
    #[must_use]
    pub fn of(config: &Config, identity: &super::scope::MemoryIdentity) -> Self {
        let resolved = identity.resolve(config);
        Self {
            agent_id: Some(resolved.agent_id.clone()),
            namespace: resolved.layout.learnings().clone(),
            reach: Reach::subtree(resolved.root().clone()),
            ..Self::default()
        }
    }

    /// The metadata `learn` stamps on the stored item.
    #[must_use]
    pub fn learn_meta(&self) -> MemoryMeta {
        MemoryMeta {
            namespace: self.namespace.clone(),
            workspace: self.workspace.clone(),
            thread_id: self.thread_id.clone(),
            agent_id: self.agent_id.clone(),
            tool_call: Some(ToolCallRef {
                name: MEMORY_TOOL_NAME.to_string(),
                id: self.tool_call_id.clone(),
            }),
            source: SourceRef {
                kind: SourceKind::Agent,
                id: None,
            },
            ..MemoryMeta::default()
        }
    }
}

/// What a `learn` result tells the model: the write was accepted, and
/// recall may not find it for a moment.
pub const LEARN_STATUS: &str = "saved; searchable in recall shortly";

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|error| format!("invalid arguments: {error}"))
}

/// Runs one action. Errors become a model-visible tool error, never a hard
/// failure, so the model can adapt (memory off, unsupported mode, …).
pub async fn run_action(config: &Config, args: &Value, facts: &CallFacts) -> ToolResult {
    let action = arg_str(args, "action").unwrap_or_default();
    tracing::debug!(action = %action, "[memory:tool] dispatch");
    let result: Result<Value, String> = match action.as_str() {
        "recall" => match parse::<RecallParams>(args) {
            Ok(mut params) => {
                confine(&mut params.filter, facts);
                let question = params.question.clone();
                ops::recall(config, params)
                    .await
                    .map(|view| {
                        if let Some(thread_id) = facts.thread_id.as_deref() {
                            record_turn_citations(thread_id, &view.citations);
                        }
                        BUS.publish(DomainEvent::MemoryRecalled {
                            query: question,
                            hit_count: view.citations.len(),
                        });
                        json!(view)
                    })
                    .map_err(render_error)
            }
            Err(error) => Err(error),
        },
        "fetch" => match parse::<FetchParams>(args) {
            Ok(mut params) => {
                confine(&mut params.filter, facts);
                ops::fetch(config, params)
                    .await
                    .map(|view| json!(view))
                    .map_err(render_error)
            }
            Err(error) => Err(error),
        },
        "learn" => match parse::<LearnParams>(args) {
            Ok(mut params) => {
                params.meta = None;
                let kind = params.kind.unwrap_or(tinymemory_api::LearningKind::Fact);
                // Accepted, not visible: indexing can take minutes on a
                // hosted engine and the turn must not wait for it.
                ops::learn_with(
                    config,
                    params,
                    Some(facts.learn_meta()),
                    tinymemory_api::WriteOptions::accepted(),
                )
                .await
                .map(|view| {
                    BUS.publish(DomainEvent::MemoryStored {
                        key: view.id.clone(),
                        category: kind.as_str().to_string(),
                        namespace: "learnings".to_string(),
                    });
                    json!({"id": view.id, "status": LEARN_STATUS})
                })
                .map_err(render_error)
            }
            Err(error) => Err(error),
        },
        "forget" => match parse::<ForgetParams>(args) {
            Ok(mut params) => {
                params.reach = Some(facts.reach.clone());
                ops::forget(config, params)
                    .await
                    .map(|view| json!(view))
                    .map_err(render_error)
            }
            Err(error) => Err(error),
        },
        other => Err(format!(
            "unknown action `{other}`; use one of recall, fetch, learn, forget"
        )),
    };
    match result {
        Ok(value) => ToolResult::success(value.to_string()),
        Err(message) => ToolResult::error(message),
    }
}

/// Confines a model-supplied filter to the calling agent's reach.
fn confine(filter: &mut Option<tinymemory_api::MetaFilter>, facts: &CallFacts) {
    filter.get_or_insert_with(Default::default).reach = Some(facts.reach.clone());
}

fn render_error(error: MemoryError) -> String {
    format!("{} ({})", error, error.code())
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        MEMORY_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Long-term memory across conversations, documents and learnings. \
         `recall` answers a question from memory with citations; `fetch` returns raw \
         matching items (filter by metadata such as workspace, repo, file_path, kinds); \
         `learn` queues a durable fact, preference, procedure or correction about the \
         user or their work, shared with every agent working alongside you; `forget` \
         queues removal of items by id. Write acknowledgements mean locally queued, \
         not yet saved or removed remotely; do not repeat them to force indexing. \
         Cached memory may be added as <memory-context>; use `recall` or `fetch` \
         for current information, and set `refers_to` when \
         the question is about a particular time. Recall before asking \
         the user something they may already have told you; learn things worth \
         remembering next time."
    }

    fn parameters_schema(&self) -> Value {
        static_schema!(include_str!("parameters/memory.json"))
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn permission_level_with_args(&self, args: &Value) -> PermissionLevel {
        match args.get("action").and_then(Value::as_str) {
            Some("recall" | "fetch") => PermissionLevel::ReadOnly,
            _ => PermissionLevel::Write,
        }
    }

    fn is_concurrency_safe(&self, args: &Value) -> bool {
        matches!(
            args.get("action").and_then(Value::as_str),
            Some("recall" | "fetch")
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_context(args, ToolCallOptions::default(), None)
            .await
    }

    async fn execute_with_context(
        &self,
        args: Value,
        _options: ToolCallOptions,
        context: Option<&dyn ToolRunContext>,
    ) -> anyhow::Result<ToolResult> {
        let facts = CallFacts::gather(&self.config, context);
        let run_id = context
            .and_then(ToolRunContext::host_extension)
            .and_then(|any| any.downcast_ref::<tinyagents_harness::tool::ToolExecutionContext>())
            .map(|ctx| ctx.run_id.as_str().to_string());
        Ok(self
            .budget
            .run(run_id, async {
                if matches!(
                    args.get("action").and_then(Value::as_str),
                    Some("learn" | "forget")
                ) {
                    let config = self.config.clone();
                    let write_args = args.clone();
                    let queued = crate::core::runtime::spawn_blocking_scoped(move || {
                        super::tool_writes::enqueue(&config, &write_args, &facts)
                    })
                    .await;
                    match queued.unwrap_or_else(|_| {
                        Err(MemoryError::Unavailable(
                            "memory write queue unavailable".into(),
                        ))
                    }) {
                        Ok(value) => {
                            super::tool_writes::schedule(self.config.clone());
                            ToolResult::success(value.to_string())
                        }
                        Err(error) => ToolResult::error(render_error(error)),
                    }
                } else {
                    run_action(&self.config, &args, &facts).await
                }
            })
            .await)
    }
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tools_agent_tests.rs"]
mod agent_tests;

#[cfg(test)]
#[path = "tools_schema_tests.rs"]
mod schema_tests;

#[cfg(test)]
#[path = "tools_budget_tests.rs"]
mod budget_tests;
