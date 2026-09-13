//! Persisting a sub-agent turn's (or a failed run's) raw transcript to
//! `session_raw`, mirroring the removed `SubagentObserver::persist_transcript`.

use crate::agent::harness::subagent_runner::types::SubagentRunError;
use crate::agent::messages::ChatMessage;

use super::dispatch::AggregatedUsage;
use super::worker_mirror::mirror_worker_thread_from_history;

/// Persist a sub-agent turn's raw transcript to `session_raw`, mirroring the
/// removed `SubagentObserver::persist_transcript`: `agent_type:"subagent"`, the
/// `task_id`, and the provider/model + usage carried on the last assistant
/// message so per-thread usage reads price the sub-agent at its own model.
#[allow(clippy::too_many_arguments)]
pub(super) fn persist_subagent_transcript(
    workspace_dir: &std::path::Path,
    transcript_stem: &str,
    agent_id: &str,
    task_id: &str,
    provider_label: &str,
    model: &str,
    history: &[ChatMessage],
    usage: &AggregatedUsage,
    context_window: u64,
    dispatcher: &str,
    iteration: u32,
) {
    use crate::agent::harness::session::transcript;

    let path = match transcript::resolve_keyed_transcript_path(workspace_dir, transcript_stem) {
        Ok(p) => p,
        Err(err) => {
            tracing::debug!(
                agent_id,
                error = %err,
                "[subagent_runner:graph] failed to resolve child transcript path"
            );
            return;
        }
    };
    let now = chrono::Utc::now().to_rfc3339();
    let turn_usage = transcript::TurnUsage {
        provider: provider_label.to_string(),
        model: model.to_string(),
        usage: transcript::MessageUsage {
            input: usage.input_tokens,
            output: usage.output_tokens,
            cached_input: usage.cached_input_tokens,
            context_window,
            cost_usd: usage.charged_amount_usd,
        },
        ts: now.clone(),
        reasoning_content: None,
        tool_calls: Vec::new(),
        iteration,
    };
    let meta = transcript::TranscriptMeta {
        agent_name: agent_id.to_string(),
        agent_id: Some(agent_id.to_string()),
        agent_type: Some("subagent".to_string()),
        dispatcher: dispatcher.into(),
        provider: Some(turn_usage.provider.clone()),
        model: Some(turn_usage.model.clone()),
        created: now.clone(),
        updated: now,
        turn_count: 1,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cached_input_tokens: usage.cached_input_tokens,
        charged_amount_usd: usage.charged_amount_usd,
        thread_id: crate::agent::tinyagents::thread_context::current_thread_id(),
        task_id: Some(task_id.to_string()),
    };
    if let Err(err) = transcript::write_transcript(&path, history, &meta, Some(&turn_usage)) {
        tracing::debug!(
            agent_id,
            error = %err,
            "[subagent_runner:graph] failed to write child transcript"
        );
    }
}

/// Persist a **failed** sub-agent run (#4466): write whatever rounds the live
/// transcript-snapshot middleware captured before the harness error to
/// `session_raw` (so `learning/transcript_ingest` can still ingest a failed run,
/// not skip an absent file), mirror those rounds onto the worker thread, and
/// append a trailing failure marker so the record is self-describing. Usage is
/// zeroed — the harness reported no totals on the error path — and the iteration
/// count is the number of completed rounds recovered.
#[allow(clippy::too_many_arguments)]
pub(super) fn persist_failed_run(
    workspace_dir: &std::path::Path,
    transcript_stem: &str,
    agent_id: &str,
    task_id: &str,
    provider_label: &str,
    model: &str,
    recovered: &[ChatMessage],
    context_window: u64,
    dispatcher: &str,
    worker_thread_id: Option<&str>,
    error: &SubagentRunError,
) {
    let marker = format!("[subagent run failed before completion: {error}]");
    let mut history = recovered.to_vec();
    history.push(ChatMessage::assistant(marker.clone()));

    // A failed run has no usage totals; record zeros so the transcript is still a
    // valid, ingestable `session_raw` record with the failure surfaced.
    let usage = AggregatedUsage::default();
    persist_subagent_transcript(
        workspace_dir,
        transcript_stem,
        agent_id,
        task_id,
        provider_label,
        model,
        &history,
        &usage,
        context_window,
        dispatcher,
        recovered.len() as u32,
    );

    if let Some(thread_id) = worker_thread_id {
        mirror_worker_thread_from_history(
            workspace_dir,
            thread_id,
            agent_id,
            task_id,
            recovered,
            Some(marker.as_str()),
        );
    }
}
