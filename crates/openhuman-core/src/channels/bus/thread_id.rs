//! Per-sender thread and client id derivation for inbound channel messages.

/// Per-sender thread-id derivation for inbound channel messages.
///
/// Matches the shape `channels::context::conversation_history_key` builds
/// for the canonical channel paths so the inbound bus handler does not
/// re-introduce a session-collapse where distinct participants in a
/// shared channel share a cached agent session.
///
/// Layout: `channel:<channel>[/<sender>][/<reply_target>][#thread:<ts>]`.
/// Each optional segment is appended only when the publisher surfaced
/// that field; legacy callers that pass only `channel` fall back to the
/// historical `channel:<channel>` key so single-DM flows keep working.
pub(crate) fn derive_inbound_thread_id(
    channel: &str,
    sender: Option<&str>,
    reply_target: Option<&str>,
    thread_ts: Option<&str>,
) -> String {
    let mut key = format!("channel:{channel}");
    let clean = |s: &str| -> Option<String> {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };
    if let Some(s) = sender.and_then(clean) {
        key.push('/');
        key.push_str(&s);
    }
    if let Some(r) = reply_target.and_then(clean) {
        key.push('/');
        key.push_str(&r);
    }
    // Telegram threads its messages by `thread_ts` for transport routing
    // but should not split memory/history per message — match the
    // `conversation_history_key` carve-out and skip the thread suffix
    // there. The socket layer addresses Telegram with raw channel ids
    // like `tg:123` as well as the literal `telegram` slug, so the
    // carve-out keys off whichever provider prefix the channel string
    // exposes, not the full id.
    if !channel_is_telegram(channel) {
        if let Some(t) = thread_ts.and_then(clean) {
            key.push_str("#thread:");
            key.push_str(&t);
        }
    }
    key
}

/// Build the per-turn `client_id` for an inbound socket message. Inbound
/// messages do not have a Socket.IO client id of their own — they arrive
/// from the channel transport layer rather than from a connected web
/// browser. Mint a stable label so downstream consumers that key on
/// `client_id` (the agent-turn origin, approval-chat-context, wallet
/// QuoteOwner pair, future audit-log keys) see distinct values for
/// distinct senders sharing a single Discord / Slack channel.
///
/// `None` (legacy publisher that didn't fill `sender`) maps to the bare
/// `"inbound"` literal that the path used historically, preserving
/// behavior for single-DM flows where no co-channel attacker exists.
pub(crate) fn derive_inbound_client_id(channel: &str, sender: Option<&str>) -> String {
    let trimmed_channel = channel.trim();
    let trimmed = sender.map(|s| s.trim()).filter(|s| !s.is_empty());
    match trimmed {
        Some(s) if !trimmed_channel.is_empty() => format!("inbound:{trimmed_channel}:{s}"),
        Some(s) => format!("inbound:{s}"),
        None => "inbound".to_string(),
    }
}

/// True for any inbound channel string that addresses Telegram, whether
/// the publisher uses the canonical slug (`"telegram"`) or the raw
/// provider-prefixed form the socket layer emits (`"tg:<chat_id>"`,
/// `"telegram:<chat_id>"`).
fn channel_is_telegram(channel: &str) -> bool {
    if channel == "telegram" || channel == "tg" {
        return true;
    }
    let provider = channel.split(':').next().unwrap_or("");
    matches!(provider, "telegram" | "tg")
}
