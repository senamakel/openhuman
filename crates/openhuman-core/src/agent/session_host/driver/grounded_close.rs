//! Tools-disabled, evidence-grounded repairs for terminal session replies.
//!
//! The normal loop owns the ordinary final model response.  This module only
//! runs when that response is absent, a no-progress breaker stopped the loop,
//! or an older harness configuration reached a cap without its in-loop close.
//! It deliberately uses the same explicit model source as the turn and keeps
//! its usage in a sidecar; it never owns transcript history or persistence.

use tinyagents_runtime::transcript::message_to_native_transcript as message_to_native_chat_message;
use tinyinference_llm::model::{ChatModel, ModelRequest, ModelResponse, ModelStream};
use tinytools_agent::dialect::ToolDialect;

use tinyagents_harness::middleware::wrap_harness_instruction;

use crate::agent::{
    session_host::turn_checkpoint::{
        self, build_deterministic_checkpoint, build_deterministic_final_summary,
        close_verification_prompt, final_answer_instruction, harness_instruction_needles,
        render_tool_results, results_from_tool_outcomes,
    },
    tinyagents::{TinyagentsTurnOutcome, TurnModelSource},
};
use crate::inference::provider::{BilledUsage, AGENT_TURN_MAX_OUTPUT_TOKENS};

/// Accounting from model calls performed after the harness loop has ended.
#[derive(Default)]
pub(super) struct RepairUsage {
    pub(super) model_calls: usize,
    pub(super) input_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) cached_input_tokens: u64,
    /// Cost of these calls: reported charge, catalog estimate, or unknown.
    pub(super) cost: tinyagents_harness::cost::CostTally,
    /// Model the calls ran on, for pricing a call that reported no charge.
    pub(super) pricing_model: String,
    /// The newest repair call's own input/output: a repair runs after the
    /// harness loop, so when one happened it is the turn's final call.
    pub(super) last_call_input_tokens: u64,
    pub(super) last_call_output_tokens: u64,
}

impl RepairUsage {
    /// Empty accounting for calls made on `model`.
    pub(super) fn for_model(model: &str) -> Self {
        Self {
            pricing_model: model.to_string(),
            ..Self::default()
        }
    }

    fn record(&mut self, usage: Option<BilledUsage>, measures_context: bool) {
        self.model_calls += 1;
        if let Some(usage) = usage {
            if measures_context {
                self.last_call_input_tokens = usage.input_tokens;
                self.last_call_output_tokens = usage.output_tokens;
            }
            self.input_tokens += usage.input_tokens;
            self.output_tokens += usage.output_tokens;
            self.cached_input_tokens += usage.cached_input_tokens();
            self.cost
                .add(crate::agent::cost::call_cost(&self.pricing_model, &usage));
        }
    }

    fn record_close_call(
        &mut self,
        kind: tinyagents_harness::close::CloseRequestKind,
        response: Option<&ModelResponse>,
    ) {
        self.record(
            response.and_then(crate::inference::provider::model::usage_info_from_response),
            kind != tinyagents_harness::close::CloseRequestKind::Verification,
        );
    }
}

/// Resolve OpenHuman's selected summarizer per close request and forward that
/// request unchanged. TinyAgents owns streaming, validation and repair.
struct SummarizerSourceModel<'a> {
    source: &'a TurnModelSource,
    model: &'a str,
    temperature: f64,
    thread_id: Option<&'a str>,
}

#[async_trait::async_trait]
impl ChatModel<()> for SummarizerSourceModel<'_> {
    async fn invoke(
        &self,
        state: &(),
        request: ModelRequest,
    ) -> tinyinference_llm::Result<ModelResponse> {
        let model = self
            .source
            .build_summarizer(self.model, self.temperature, self.thread_id)
            .map_err(|error| tinyinference_llm::Error::Model(error.to_string()))?;
        model.invoke(state, request).await
    }

    async fn stream(
        &self,
        state: &(),
        request: ModelRequest,
    ) -> tinyinference_llm::Result<ModelStream> {
        let model = self
            .source
            .build_summarizer(self.model, self.temperature, self.thread_id)
            .map_err(|error| tinyinference_llm::Error::Model(error.to_string()))?;
        model.stream(state, request).await
    }
}

/// A repaired terminal reply and the extra provider usage it incurred.
pub(super) struct GroundedClose {
    pub(super) output: String,
    pub(super) usage: RepairUsage,
}

/// A classified halt has enough evidence for a deterministic partial result.
/// Keeping this separate from the model-driven repair path guarantees zero
/// additional provider calls once its recovery budget is exhausted.
fn classified_halt_close(outcome: &TinyagentsTurnOutcome) -> Option<GroundedClose> {
    let reason = outcome.breaker_halt.as_deref()?;
    if !reason.starts_with("Stopping after ") {
        return None;
    }
    let records = results_from_tool_outcomes(&outcome.tool_outcomes);
    Some(GroundedClose {
        output: build_deterministic_final_summary(&records, Some(reason)),
        usage: RepairUsage::default(),
    })
}

/// Repair an otherwise valid terminal reply which omits the host's required
/// structured-output block.  This mirrors the legacy session contract while
/// keeping the repair call and its usage inside the explicit driver sidecar.
/// The call records into the caller-owned `usage` before streaming, so a
/// repair cut off by the turn deadline is still billed.
#[allow(clippy::too_many_arguments)]
pub(super) async fn repair_required_output(
    source: &TurnModelSource,
    model: &str,
    temperature: f64,
    thread_id: Option<&str>,
    dispatcher: &dyn ToolDialect,
    contract: &tinyagents_harness::config::RequiredOutput,
    history: &[tinyinference_llm::message::Message],
    reply: &str,
    reply_already_streamed: bool,
    progress: Option<&tokio::sync::mpsc::Sender<crate::progress::AgentProgress>>,
    iteration: u32,
    usage: &mut RepairUsage,
) -> Option<String> {
    use tinyagents_harness::config as required;

    let prompt_history: Vec<tinyinference_llm::message::Message> = history
        .iter()
        .filter_map(message_to_native_chat_message)
        .map(|message| crate::agent::tinyagents::chat_message_to_message(&message))
        .collect();
    let repair_instruction = wrap_harness_instruction(&required::repair_instruction(contract));
    let mut on_call_completed = |kind, response: Option<&ModelResponse>| {
        usage.record_close_call(kind, response);
    };
    let options = tinyagents_harness::close::RequiredOutputRepairOptions {
        request: tinyagents_harness::close::CloseRequestOptions {
            model: Some(model),
            temperature: Some(temperature),
            max_output_tokens: Some(AGENT_TURN_MAX_OUTPUT_TOKENS),
            minimum_harness_phrase_chars: 1,
            on_call_completed: Some(&mut on_call_completed),
        },
        fallback_placement: tinyagents_harness::close::RequiredOutputFallbackPlacement::Prepend,
    };
    let model_client = SummarizerSourceModel {
        source,
        model,
        temperature,
        thread_id,
    };
    let repaired = tinyagents_harness::close::repair_required_output_with_repair_options(
        &model_client,
        &(),
        dispatcher,
        tinyagents_harness::close::RequiredOutputRepairConfig {
            history: &prompt_history,
            reply,
            contract,
            repair_instruction: &repair_instruction,
            reply_already_streamed,
        },
        options,
    )
    .await?;
    // The main loop may already have emitted the reply. In that case send only
    // TinyAgents' returned continuation, then commit the same full text.
    if reply_already_streamed {
        let continuation = repaired
            .text
            .strip_prefix(reply)
            .unwrap_or(&repaired.text)
            .to_owned();
        if let Some(progress) = progress {
            if let Err(error) = progress
                .send(crate::progress::AgentProgress::TextDelta {
                    delta: continuation,
                    iteration,
                })
                .await
            {
                tracing::debug!(%error, "[session-runtime] required-output repair progress receiver closed");
            }
        }
    }
    Some(repaired.text)
}

/// The tool-less closing instruction for `outcome`: the cap checkpoint when the
/// run stopped at its call cap, otherwise the final-answer directive — which
/// names the run's real stop cause (a breaker halt, or a reply that ran out of
/// output tokens while reasoning) rather than claiming the model finished.
fn close_instruction(
    outcome: &TinyagentsTurnOutcome,
    needs_cap_close: bool,
    rendered: &str,
) -> String {
    if needs_cap_close {
        return format!(
            "{}\n\n<tool_records>\n{}\n</tool_records>",
            wrap_harness_instruction(turn_checkpoint::MAX_ITER_CHECKPOINT_INSTRUCTION),
            if rendered.is_empty() {
                "(no tool calls completed)"
            } else {
                rendered
            }
        );
    }
    if outcome.truncated {
        tracing::info!(
            model_calls = outcome.model_calls,
            tool_calls = outcome.tool_calls,
            "[session-runtime] closing a turn whose last reply ran out of output tokens"
        );
    }
    final_answer_instruction(outcome.breaker_halt.as_deref(), outcome.truncated, rendered)
}

/// Return `None` when the loop's terminal text is already usable.
#[allow(clippy::too_many_arguments)]
pub(super) async fn close_if_needed(
    source: &TurnModelSource,
    model: &str,
    temperature: f64,
    thread_id: Option<&str>,
    dispatcher: &dyn ToolDialect,
    base_history: &[tinyinference_llm::message::Message],
    user_message: &str,
    outcome: &TinyagentsTurnOutcome,
    budget: Option<std::time::Duration>,
) -> Option<GroundedClose> {
    let needs_cap_close = outcome.hit_cap && !outcome.wrap_up_injected;
    let needs_final_close = outcome.text.trim().is_empty() || outcome.breaker_halt.is_some();
    if !needs_cap_close && !needs_final_close {
        return None;
    }

    if let Some(close) = classified_halt_close(outcome) {
        return Some(close);
    }
    let records = results_from_tool_outcomes(&outcome.tool_outcomes);
    let rendered = render_tool_results(&records, turn_checkpoint::GROUNDING_TOTAL_CHARS);
    let instruction = close_instruction(outcome, needs_cap_close, &rendered);
    let history: Vec<tinyinference_llm::message::Message> = base_history
        .iter()
        .filter_map(message_to_native_chat_message)
        .map(|message| crate::agent::tinyagents::chat_message_to_message(&message))
        .collect();
    let stop_reason = outcome.breaker_halt.as_deref();
    let harness_needles = harness_instruction_needles(stop_reason);
    let harness_phrases: Vec<&str> = harness_needles.iter().map(String::as_str).collect();
    let fallback = || {
        if needs_cap_close {
            build_deterministic_checkpoint(&records, outcome.model_calls)
        } else {
            build_deterministic_final_summary(&records, stop_reason)
        }
    };

    let mut usage = RepairUsage::for_model(model);
    let model_client = SummarizerSourceModel {
        source,
        model,
        temperature,
        thread_id,
    };
    let mut on_call_completed = |kind, response: Option<&ModelResponse>| {
        usage.record_close_call(kind, response);
    };
    let close_options = tinyagents_harness::close::CloseRequestOptions {
        model: Some(model),
        temperature: Some(temperature),
        max_output_tokens: Some(AGENT_TURN_MAX_OUTPUT_TOKENS),
        minimum_harness_phrase_chars: 1,
        on_call_completed: Some(&mut on_call_completed),
    };
    let verification_prompt = |candidate: &str| -> String {
        close_verification_prompt(user_message, &rendered, candidate)
    };
    let repair_instruction = |violation: tinyagents_harness::close::CloseViolation| -> String {
        let violation = match violation {
            tinyagents_harness::close::CloseViolation::NoReply => {
                crate::agent::session_host::turn_checkpoint::CloseViolation::NoReply
            }
            tinyagents_harness::close::CloseViolation::QuotedInstruction => {
                crate::agent::session_host::turn_checkpoint::CloseViolation::QuotedHarnessText
            }
            tinyagents_harness::close::CloseViolation::Unverified => {
                crate::agent::session_host::turn_checkpoint::CloseViolation::Unverified
            }
        };
        crate::agent::session_host::turn_checkpoint::close_repair_instruction(
            &instruction,
            violation,
        )
    };
    let output = tinyagents_harness::close::grounded_close_with_options(
        &model_client,
        &(),
        dispatcher,
        tinyagents_harness::close::GroundedCloseConfig {
            history: &history,
            initial_prompt: &instruction,
            verification_prompt,
            repair_instruction,
            harness_phrases: &harness_phrases,
            budget,
            deterministic_fallback: fallback,
        },
        close_options,
    )
    .await
    .text;
    Some(GroundedClose { output, usage })
}

#[cfg(test)]
#[path = "grounded_close_tests.rs"]
mod tests;
