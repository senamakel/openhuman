//! Re-filtering a cached toolkit action catalogue against the user's
//! *current* per-toolkit scope preference (read/write/admin), used by the
//! typed-mode prompt builder when it reuses a cached connected-integration
//! catalogue instead of re-fetching it.

/// Re-filter `actions` (a cached toolkit action catalogue) against the
/// caller's live per-toolkit scope preference, so a scope change since the
/// catalogue was cached is honored without a re-fetch.
pub(super) async fn filter_cached_toolkit_actions_with_current_scope(
    agent_id: &str,
    toolkit: &str,
    config: &crate::config::Config,
    actions: &[crate::agent::context::prompt::ConnectedIntegrationTool],
) -> Vec<crate::agent::context::prompt::ConnectedIntegrationTool> {
    let pref = crate::integrations::composio::ops::load_user_scope_pref(config, toolkit).await;
    let before = actions.len();
    let filtered: Vec<_> = actions
        .iter()
        .filter(|action| {
            crate::integrations::composio::providers::is_action_visible_with_pref(
                &action.name,
                &pref,
            )
        })
        .cloned()
        .collect();
    tracing::debug!(
        agent_id = %agent_id,
        toolkit = %toolkit,
        cached_actions = before,
        visible_actions = filtered.len(),
        hidden_actions = before.saturating_sub(filtered.len()),
        read = pref.read,
        write = pref.write,
        admin = pref.admin,
        "[subagent_runner:typed] re-filtered cached toolkit catalogue with current user scope"
    );
    filtered
}
