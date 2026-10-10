//! Committed-turn goal accounting and billing projection.

/// Account only a receipt-backed turn, using the same direct-plus-completed
/// child total the codec attached to the atomic transcript append.
pub(in crate::agent::session_host) async fn account_committed_turn_against_goal(
    workspace_dir: &std::path::Path,
    thread_id: Option<&str>,
    sidecar: &crate::agent::tinyagents::host::run_context::SessionTurnSidecar,
) {
    let child_input = sidecar
        .subagents
        .iter()
        .map(|entry| entry.usage.input_tokens)
        .sum::<u64>();
    let child_output = sidecar
        .subagents
        .iter()
        .map(|entry| entry.usage.output_tokens)
        .sum::<u64>();
    crate::agent::goals::runtime::account_turn_against_goal(
        workspace_dir,
        thread_id,
        sidecar.input_tokens.saturating_add(child_input),
        sidecar.output_tokens.saturating_add(child_output),
        sidecar
            .duration
            .map(|duration| duration.as_secs())
            .unwrap_or_default(),
    )
    .await;
}

/// UI billing projection for a committed turn. It keeps child records for a
/// detailed display while reporting the same all-in totals the codec persists.
pub(in crate::agent::session_host) fn holistic_last_turn_usage(
    sidecar: &crate::agent::tinyagents::host::run_context::SessionTurnSidecar,
) -> crate::agent::tinyagents::host::LastTurnUsage {
    // Each count is the turn's own plus every synchronous child's.
    let tokens = |own: u64, child: fn(&crate::agent::subagent_host::SubagentUsage) -> u64| {
        sidecar.subagents.iter().fold(own, |total, entry| {
            total.saturating_add(child(&entry.usage))
        })
    };
    let input_tokens = tokens(sidecar.input_tokens, |usage| usage.input_tokens);
    let output_tokens = tokens(sidecar.output_tokens, |usage| usage.output_tokens);
    let cached_input_tokens = tokens(sidecar.cached_input_tokens, |usage| {
        usage.cached_input_tokens
    });
    let mut cost = sidecar.cost;
    for entry in &sidecar.subagents {
        cost.merge(entry.usage.cost());
    }
    crate::agent::tinyagents::host::LastTurnUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens,
        cost_usd: cost.usd(),
        cost_source: cost.source,
        context_window: sidecar.context_window,
        context_tokens: sidecar
            .last_call_input_tokens
            .saturating_add(sidecar.last_call_output_tokens),
        subagents: sidecar.subagents.clone(),
        // The session does not count these; a library turn that asked for a
        // final-response report fills them in (`response_shape`).
        reasoning_tokens: 0,
    }
}
