//! The orchestrator's tool-output payload summarizer, wired at session build.
//!
//! Split out of `factory.rs`, which builds the rest of the session.

/// Issue #574 — when a tool returns a huge payload (Composio
/// dump, long file read, web scrape), it should be compressed
/// by TinyJuice's summary stage before entering the orchestrator's
/// history. TinyJuice owns the prompt and the thresholds (installed
/// from `ContextConfig`); the host supplies only the model call,
/// through a `SubagentPayloadSummarizer` built from the `summarizer`
/// agent definition. Every other agent id gets
/// `None` and their tool results stay untouched (the summarizer
/// itself MUST be `None` to avoid recursive self-summarization).
pub(super) fn payload_summarizer_for(
    agent_id: &str,
    config: &crate::config::Config,
) -> Option<std::sync::Arc<dyn crate::agent::tinyagents::payload_summarizer::PayloadSummarizer>> {
    if super::summarizes_tool_output(agent_id, config) {
        match crate::agent::harness::definition::AgentDefinitionRegistry::global() {
            Some(reg) => match reg.get("summarizer") {
                Some(summarizer_def) => {
                    log::info!(
                        "[agent::builder] wiring payload_summarizer for orchestrator: \
                         threshold_tokens={} max_tokens={}",
                        config.context.summarizer_payload_threshold_tokens,
                        config.context.summarizer_max_payload_tokens
                    );
                    Some(std::sync::Arc::new(
                        crate::agent::tinyagents::payload_summarizer::SubagentPayloadSummarizer::new(
                            summarizer_def.clone(),
                        ),
                    ))
                }
                None => {
                    log::warn!(
                        "[agent::builder] orchestrator requested payload_summarizer but \
                         `summarizer` definition is not in the registry — proceeding without it"
                    );
                    None
                }
            },
            None => {
                log::warn!(
                    "[agent::builder] orchestrator requested payload_summarizer but \
                     AgentDefinitionRegistry is not initialised — proceeding without it"
                );
                None
            }
        }
    } else {
        None
    }
}
