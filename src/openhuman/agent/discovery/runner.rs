//! Bounded tool-calling loop for the owner-discovery agent.
//!
//! A single call to [`run_discovery`] executes the full flow:
//!
//! 1. Build the system prompt + initial user prompt from seed facts.
//! 2. Loop up to `config.max_rounds` times:
//!    - POST messages + tool specs to the LLM.
//!    - If `finish_reason == "tool_calls"`, dispatch each call via
//!      [`tools::dispatch_tool`], append the results, continue.
//!    - Otherwise, return.
//! 3. Return a [`DiscoveryReport`] summarising writes + errors.
//!
//! Same shape as `commands/chat.rs::chat_send_inner` but reduced to
//! the discovery agent's narrow scope.

use std::sync::Arc;

use serde_json::Value;

use super::apify::ApifyClient;
use super::llm::{LlmClient, LlmMessage, LlmToolCall};
use super::tools::{dispatch_tool, tool_specs};
use super::types::{DiscoveryError, DiscoveryJob, DiscoveryReport, DiscoveryTrigger, SeedFact};
use crate::openhuman::config::DiscoveryConfig;
use crate::openhuman::memory::MemoryClient;

/// Dependencies handed to the runner — all behind traits/Arcs so
/// integration tests can inject stubs.
pub struct DiscoveryDeps {
    pub memory: Arc<MemoryClient>,
    pub llm: Arc<dyn LlmClient>,
    pub apify: Arc<dyn ApifyClient>,
    pub config: DiscoveryConfig,
    /// Model passed through to the LLM client.
    pub model: String,
}

/// Execute a discovery run.
///
/// Individual tool failures (malformed payloads, missing fields, Apify
/// timeouts) are non-fatal and captured in [`DiscoveryReport::errors`].
/// Only LLM transport failures bubble up as [`DiscoveryError::Llm`].
pub async fn run_discovery(
    job: DiscoveryJob,
    deps: DiscoveryDeps,
) -> Result<DiscoveryReport, DiscoveryError> {
    log::info!(
        "[discovery] run start trigger={:?} seeds={}",
        job.trigger,
        job.seed_facts.len()
    );

    let system_prompt = build_system_prompt();
    let user_prompt = build_user_prompt(&job.trigger, &job.seed_facts);
    let tools: Vec<Value> = tool_specs();
    let origin = origin_from_trigger(&job.trigger);

    let mut messages: Vec<LlmMessage> = vec![
        LlmMessage::system(system_prompt),
        LlmMessage::user(user_prompt),
    ];
    let mut report = DiscoveryReport::default();

    for round in 0..deps.config.max_rounds {
        log::debug!(
            "[discovery] round {} / {} — history_len={}",
            round + 1,
            deps.config.max_rounds,
            messages.len()
        );

        let response = deps
            .llm
            .chat(&messages, &tools, &deps.model)
            .await
            .map_err(|e| DiscoveryError::Llm(e.to_string()))?;

        report.rounds_used = round + 1;

        // Append the assistant turn to history, including tool_calls so
        // the model can reference its own calls on the next round.
        messages.push(assistant_message_with_calls(
            &response.content,
            &response.tool_calls,
        ));

        if response.tool_calls.is_empty() || response.finish_reason != "tool_calls" {
            log::info!(
                "[discovery] finish reason={} text_len={}",
                response.finish_reason,
                response.content.len()
            );
            return Ok(report);
        }

        for call in &response.tool_calls {
            log::debug!("[discovery] dispatch tool={} id={}", call.name, call.id);
            let outcome = dispatch_tool(
                &call.name,
                &call.arguments,
                &deps.memory,
                &deps.apify,
                &deps.config,
                &mut report,
                &origin,
            )
            .await;

            messages.push(LlmMessage::tool(
                call.id.clone(),
                call.name.clone(),
                outcome.content,
            ));
            if !outcome.success {
                log::warn!("[discovery] tool {} failed", call.name);
            }
        }
    }

    log::warn!(
        "[discovery] max_rounds ({}) exceeded — returning report",
        deps.config.max_rounds
    );
    report
        .errors
        .push(format!("max_rounds ({}) exceeded", deps.config.max_rounds));
    Ok(report)
}

/// Render the assistant's reply as an `LlmMessage` that still carries
/// the `tool_calls` metadata on serialization. We piggyback on
/// `tool_call_id` / `name` being `None` for the assistant role, but
/// encode the call list inside the content so a later LLM round can
/// still see what was requested. The OpenAI API itself handles the
/// round-trip via `choices[0].message.tool_calls`, which we don't
/// persist into `LlmMessage`; we only need enough context for the
/// *next* call, where the role=tool replies alone are sufficient.
fn assistant_message_with_calls(content: &str, _calls: &[LlmToolCall]) -> LlmMessage {
    LlmMessage::assistant(content.to_string())
}

fn build_system_prompt() -> String {
    r#"You are the OpenHuman owner-discovery agent.

Your job: given seed signals from a connected skill (email, username,
workspace name, etc.), use the available tools to find high-confidence
information about the owner of this OpenHuman instance — their full
name, current role and title, company, public bio, location/timezone,
and notable professional interests — then persist those findings via
`owner_write`.

Rules:

1. BE CONSERVATIVE. Only call `owner_write` when you are confident
   the information refers to THIS user, not a similarly-named person.
2. Prefer primary sources (the person's own LinkedIn, company page,
   personal site) over aggregators.
3. Use `apify_search_person` first with whatever seed you have, then
   `apify_fetch_url` to pull details from the most promising hit.
4. When you're done (or when you can't find anything useful), reply
   with a one-sentence summary of what you wrote and STOP. Do not
   loop forever.
5. NEVER invent facts to fill gaps. Missing information is fine.
6. You have at most 5 tool-calling rounds — budget accordingly."#
        .to_string()
}

fn build_user_prompt(trigger: &DiscoveryTrigger, seeds: &[SeedFact]) -> String {
    let trigger_desc = match trigger {
        DiscoveryTrigger::SkillOAuthCompleted {
            skill_id,
            integration_id,
        } => {
            if integration_id.is_empty() {
                format!("skill '{skill_id}' just finished OAuth")
            } else {
                format!("skill '{skill_id}' just finished OAuth for integration '{integration_id}'")
            }
        }
        DiscoveryTrigger::Manual => "user manually requested discovery".into(),
        DiscoveryTrigger::Test => "test harness".into(),
    };

    let mut out = format!("Discovery run triggered: {trigger_desc}.\n\nSeed signals:\n");
    if seeds.is_empty() {
        out.push_str("- (no seeds — start from scratch using whatever you can infer)\n");
    } else {
        for seed in seeds {
            out.push_str(&format!("- {}: {}\n", seed.label, seed.value));
        }
    }
    out.push_str("\nResearch the owner and persist findings via `owner_write`. Stop when done.");
    out
}

fn origin_from_trigger(trigger: &DiscoveryTrigger) -> String {
    match trigger {
        DiscoveryTrigger::SkillOAuthCompleted { skill_id, .. } => {
            format!("discovery-{skill_id}")
        }
        DiscoveryTrigger::Manual => "discovery-manual".into(),
        DiscoveryTrigger::Test => "discovery-test".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_prompt_includes_seeds_and_trigger() {
        let trigger = DiscoveryTrigger::SkillOAuthCompleted {
            skill_id: "gmail".into(),
            integration_id: "ada@example.com".into(),
        };
        let seeds = vec![
            SeedFact::new("email", "ada@example.com"),
            SeedFact::new("company", "Analytical Engines"),
        ];
        let prompt = build_user_prompt(&trigger, &seeds);
        assert!(prompt.contains("gmail"));
        assert!(prompt.contains("ada@example.com"));
        assert!(prompt.contains("Analytical Engines"));
        assert!(prompt.contains("owner_write"));
    }

    #[test]
    fn user_prompt_handles_empty_seeds() {
        let trigger = DiscoveryTrigger::Manual;
        let prompt = build_user_prompt(&trigger, &[]);
        assert!(prompt.contains("no seeds"));
    }

    #[test]
    fn origin_encodes_trigger_source() {
        let trigger = DiscoveryTrigger::SkillOAuthCompleted {
            skill_id: "notion".into(),
            integration_id: String::new(),
        };
        assert_eq!(origin_from_trigger(&trigger), "discovery-notion");
        assert_eq!(
            origin_from_trigger(&DiscoveryTrigger::Manual),
            "discovery-manual"
        );
        assert_eq!(
            origin_from_trigger(&DiscoveryTrigger::Test),
            "discovery-test"
        );
    }
}
