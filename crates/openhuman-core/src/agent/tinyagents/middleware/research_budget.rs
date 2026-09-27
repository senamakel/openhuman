//! Bound direct web research so a run that keeps finding more leads still answers.

use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::{Middleware, ToolInvocationIdentity};
use tinyinference_llm::message::Message;
use tinyinference_llm::model::{ModelRequest, ToolChoice};
use tinytools::ToolResult;

use crate::agent::tinyagents::host::OpenHumanRunContext;

/// Direct web reads allowed before the next model call must conclude the turn.
/// Broad research can use the dedicated research agent; this bounds the master
/// agent's exploratory search while leaving room to read primary sources.
pub(super) const DIRECT_WEB_READ_LIMIT: usize = 8;

const RESEARCH_CLOSE_INSTRUCTION: &str = "The direct web research budget for this turn is exhausted. Answer the user's latest request now using the results already available. State any remaining uncertainty. Do not search again, repeat a page fetch, or merely describe what you plan to read.";

#[derive(Default)]
pub(crate) struct ResearchBudgetMiddleware {
    completed_reads: AtomicUsize,
}

impl ResearchBudgetMiddleware {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Middleware<(), OpenHumanRunContext> for ResearchBudgetMiddleware {
    fn name(&self) -> &str {
        "research_budget"
    }

    async fn after_tool(
        &self,
        _ctx: &mut RunContext<OpenHumanRunContext>,
        _state: &(),
        invocation: &ToolInvocationIdentity,
        _result: &mut ToolResult,
    ) -> TaResult<()> {
        if matches!(
            invocation.tool_name(),
            "web_search_tool" | "web_answer_tool" | "web_contents_tool" | "web_fetch"
        ) {
            self.completed_reads.fetch_add(1, Ordering::Relaxed);
        }
        Ok(())
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<OpenHumanRunContext>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        if self.completed_reads.load(Ordering::Relaxed) < DIRECT_WEB_READ_LIMIT {
            return Ok(());
        }
        tracing::info!(
            web_reads = self.completed_reads.load(Ordering::Relaxed),
            "[tinyagents::mw] direct web research budget reached; concluding turn"
        );
        request.tools.clear();
        request.tool_choice = ToolChoice::None;
        request
            .messages
            .push(Message::user(RESEARCH_CLOSE_INSTRUCTION.to_string()));
        Ok(())
    }
}
