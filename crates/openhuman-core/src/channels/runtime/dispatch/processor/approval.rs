//! Gating and intercepting approval-gate replies on channels that surface
//! approvals in chat.

use tinychannels_bus as traits;

/// Whether a channel surfaces approvals in chat: the
/// `ChannelApprovalSurfaceSubscriber` turns `ApprovalRequested` events into
/// chat messages and the user's reply flows back into the [`ApprovalGate`].
/// When `true`, the dispatch loop scopes the agent turn in an
/// [`ApprovalChatContext`] so the gate actually fires for `Prompt`-class tools,
/// and intercepts yes/no replies for parked approvals.
///
/// Driven by the provider's `chat_approvals` capability. Channels without it
/// (email, CLI, webhooks) keep the "no chat context → allow" behaviour, since
/// a prompt there would only TTL-deny every parked call.
///
/// [`ApprovalChatContext`]: crate::security::approval::ApprovalChatContext
/// [`ApprovalGate`]: crate::security::approval::ApprovalGate
pub(crate) fn channel_has_approval_surface(
    capabilities: &tinychannels_bus::ChannelCapabilities,
) -> bool {
    capabilities.chat_approvals
}

/// Admission's approval intercept: a yes/no reply decides the approval parked
/// on `thread_id`, but only on a channel instance that reports
/// `chat_approvals`. A channel missing from the runtime map fails closed, so
/// the reply goes out as a fresh turn.
pub(super) fn consumes_approval_reply(
    target_channel: Option<&std::sync::Arc<dyn crate::channels::Channel>>,
    gate: Option<&crate::security::approval::ApprovalGate>,
    msg: &traits::ChannelInboundEnvelope,
    thread_id: &str,
) -> bool {
    target_channel.is_some_and(|channel| channel_has_approval_surface(&channel.capabilities()))
        && route_approval_reply(gate, msg, thread_id)
}

/// If the inbound message is a yes/no reply for a parked approval on
/// this thread, route it to [`ApprovalGate::decide`] and return `true`.
/// Otherwise return `false` so the caller can dispatch the message as a
/// fresh turn (which intentionally cancels any parked approval — the
/// user is redirecting). Mirrors the web channel intercept at
/// `web_chat/`. With no gate installed nothing is intercepted.
///
/// [`ApprovalGate::decide`]: crate::security::approval::ApprovalGate::decide
fn route_approval_reply(
    gate: Option<&crate::security::approval::ApprovalGate>,
    msg: &traits::ChannelInboundEnvelope,
    thread_id: &str,
) -> bool {
    let Some(gate) = gate else {
        return false;
    };
    let Some(request_id) = gate.pending_for_thread(thread_id) else {
        return false;
    };
    let Some(decision) = crate::security::approval::parse_approval_reply(&msg.text) else {
        return false;
    };
    match gate.decide(&request_id, decision) {
        Ok(Some(_)) => {
            tracing::info!(
                "[dispatch] routed chat reply to approval gate channel={} thread_id={} request_id={} decision={}",
                msg.channel.id,
                thread_id,
                request_id,
                decision.as_str()
            );
            true
        }
        Ok(None) => {
            // The request was already decided / cleared between our
            // `pending_for_thread` check and `decide`. Don't claim the
            // intercept; fall through so the reply lands as a normal turn.
            tracing::warn!(
                "[dispatch] approval reply targeted a non-pending request channel={} thread_id={} request_id={} — dispatching as fresh turn",
                msg.channel.id,
                thread_id,
                request_id
            );
            false
        }
        Err(err) => {
            tracing::warn!(
                "[dispatch] approval gate decide failed channel={} thread_id={} request_id={}: {err}",
                msg.channel.id,
                thread_id,
                request_id
            );
            false
        }
    }
}

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
