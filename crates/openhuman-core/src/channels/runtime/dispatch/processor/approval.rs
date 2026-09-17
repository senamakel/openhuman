//! Gating and intercepting approval-gate replies on channels that have a
//! registered approval surface.

use crate::channels::context::conversation_history_key;
use crate::channels::providers::telegram::TELEGRAM_APPROVAL_CLIENT_ID;
use crate::channels::traits;

/// Whether a channel currently has a registered approval surface — i.e.
/// a subscriber that turns `ApprovalRequested` events into chat messages
/// and a way for the user's reply to flow back into the
/// [`ApprovalGate`]. When `true`, the dispatch loop scopes the agent
/// turn in an [`ApprovalChatContext`] so the gate actually fires for
/// `Prompt`-class tools and intercepts yes/no replies for parked
/// approvals.
///
/// Only Telegram has a surface today (sub-issue 2 of #3098). Discord /
/// Slack / iMessage / Mattermost remain in the legacy "no chat context
/// → silently allow" state until each gets a per-channel surface in a
/// follow-up PR; surfacing approvals there without a subscriber would
/// just TTL-deny every parked call, which is worse than the status quo.
///
/// [`ApprovalChatContext`]: crate::security::approval::ApprovalChatContext
/// [`ApprovalGate`]: crate::security::approval::ApprovalGate
pub(crate) fn channel_has_approval_surface(channel: &str) -> bool {
    channel == TELEGRAM_APPROVAL_CLIENT_ID
}

/// If the inbound message is a yes/no reply for a parked approval on
/// this thread, route it to [`ApprovalGate::decide`] and return `true`.
/// Otherwise return `false` so the caller can dispatch the message as a
/// fresh turn (which intentionally cancels any parked approval — the
/// user is redirecting). Mirrors the web channel intercept at
/// `web_chat/`.
///
/// [`ApprovalGate::decide`]: crate::security::approval::ApprovalGate::decide
pub(super) async fn try_route_approval_reply(msg: &traits::ChannelMessage) -> bool {
    let Some(gate) = crate::security::approval::ApprovalGate::try_global() else {
        return false;
    };
    let thread_id = conversation_history_key(msg);
    let Some(request_id) = gate.pending_for_thread(&thread_id) else {
        return false;
    };
    let Some(decision) = crate::security::approval::parse_approval_reply(&msg.content) else {
        return false;
    };
    match gate.decide(&request_id, decision) {
        Ok(Some(_)) => {
            tracing::info!(
                "[dispatch] routed chat reply to approval gate channel={} thread_id={} request_id={} decision={}",
                msg.channel,
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
                msg.channel,
                thread_id,
                request_id
            );
            false
        }
        Err(err) => {
            tracing::warn!(
                "[dispatch] approval gate decide failed channel={} thread_id={} request_id={}: {err}",
                msg.channel,
                thread_id,
                request_id
            );
            false
        }
    }
}
