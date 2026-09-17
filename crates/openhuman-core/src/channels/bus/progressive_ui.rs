//! Progressive-UI capability gating: which channels support evolving
//! draft/thinking/filler bubbles, and the per-provider latch that remembers
//! when a backend has answered "no edit route" so we stop re-probing it.

/// Minimum interval between progressive edits of the outbound channel
/// message. Tuned to stay comfortably below Telegram's ~1 edit/sec cap
/// per chat. Slack has a similar soft limit.
pub(super) const EDIT_FLUSH_INTERVAL: tokio::time::Duration =
    tokio::time::Duration::from_millis(1000);

/// Maximum consecutive edit failures tolerated before giving up on
/// progressive streaming and falling back to atomic-final delivery.
pub(super) const MAX_EDIT_FAILURES: u32 = 2;

/// How often to re-send the "typing…" indicator while a turn is in
/// flight. Telegram's `sendChatAction` keeps the UI alive for about
/// 5 seconds per call, so we refresh every 4 s to ensure continuity.
pub(super) const TYPING_REFRESH_INTERVAL: tokio::time::Duration =
    tokio::time::Duration::from_secs(4);

/// Maximum consecutive typing-indicator failures before we stop
/// trying. One failure is usually "endpoint doesn't exist"; two is
/// enough to conclude the backend doesn't support it on this channel.
pub(super) const MAX_TYPING_FAILURES: u32 = 2;

/// How often to post a filler "still working" message to the channel
/// so the user keeps seeing activity during long agent turns. Deleted
/// on finalization alongside the ephemeral thinking bubble.
pub(super) const FILLER_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_secs(13);

/// Maximum consecutive filler-send failures before we stop trying.
/// Same rationale as the thinking/typing latches.
pub(super) const MAX_FILLER_FAILURES: u32 = 2;

/// Maximum number of Unicode scalars to include in a dynamic filler
/// derived from the thinking accumulator. Keeps each bubble compact.
pub(super) const MAX_FILLER_CHARS: usize = 200;

/// Fallback rotating pool used when the thinking stream has produced
/// nothing new since the previous filler (or nothing at all). Index in
/// `StreamingState.filler_index` advances only when this branch is hit.
pub(super) const STATIC_FILLERS: &[&str] = &[
    "💭 Still working on it…",
    "💭 Just a moment…",
    "💭 Almost there…",
];

/// Maximum length of the thinking snippet shown in the ephemeral
/// channel message. Longer reasoning is truncated with "…" to avoid
/// overwhelming the chat.
pub(super) const MAX_THINKING_DISPLAY_CHARS: usize = 500;

/// Whether a channel supports the progressive-UI placeholders — the
/// evolving draft bubble, the rotating "💭" fillers, and the ephemeral
/// "thinking" bubble. All three rely on the backend supporting **both**
/// message *edit* and *delete*: edit keeps a single bubble evolving in
/// place, delete removes it once the final reply lands. Telegram supports
/// both. Discord's adapter supports **neither** (edits 404, delete is a
/// hard `Delete not supported` stub), so every placeholder becomes a
/// permanent, un-editable, un-deletable message — the channel fills with
/// "💭 Still working on it…" bubbles.
///
/// This is an **allowlist**, not a denylist: only channels confirmed to
/// support edit+delete opt in. A new/unknown adapter therefore fails *safe*
/// (placeholders suppressed) rather than silently re-introducing the spam bug
/// this gate was added to fix.
pub(crate) fn channel_supports_progressive_ui(channel: &str) -> bool {
    // Inbound channels arrive provider-prefixed from the socket layer
    // (e.g. `discord:<guild>`, `tg:<chat>`), so compare the provider prefix,
    // not the whole id — mirroring `channel_is_telegram`.
    let provider = channel.split(':').next().unwrap_or(channel);
    matches!(provider, "telegram" | "tg")
}

/// Why an edit of an already-posted channel message failed.
///
/// These three causes need three different recoveries, and conflating the
/// first two is what broke the live "💭 Thinking:" bubble (#5230).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum EditFailure {
    /// The backend serves no message-edit route at all, so the edit could
    /// never have succeeded. The message itself is untouched and we still own
    /// it: keep its id so it can still be deleted (or finally edited once the
    /// backend ships the route) and just stop attempting edits.
    RouteUnsupported,
    /// The message really is gone on the provider side (user deleted it, or
    /// the backend GC'd the relay row). The id is worthless — drop it.
    MessageGone,
    /// Anything else (transient 5xx, transport error, rate limit). Counts
    /// against the per-turn failure budget and may be retried.
    Transient,
}

/// Classify an edit failure by typed error rather than by message text, so the
/// recovery a call site picks cannot drift with `#[error(...)]` wording.
pub(super) fn classify_edit_failure(err: &anyhow::Error) -> EditFailure {
    match err.downcast_ref::<crate::api::rest::BackendApiError>() {
        Some(crate::api::rest::BackendApiError::ChannelEditUnsupported { .. }) => {
            EditFailure::RouteUnsupported
        }
        Some(crate::api::rest::BackendApiError::MessageNotFound { .. }) => EditFailure::MessageGone,
        _ => EditFailure::Transient,
    }
}

/// Providers whose backend answered "no edit route" at least once this
/// process. Whether the route exists is a property of the deployed backend,
/// not of a turn, so re-probing it on every turn only buys a guaranteed-404
/// round-trip per turn forever. Latching it here keeps that to one attempt per
/// provider per process — and it self-heals on the next core start once the
/// backend ships the route (#5230).
static EDIT_UNSUPPORTED_PROVIDERS: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashSet<String>>,
> = std::sync::OnceLock::new();

/// Provider key for the edit-capability latch. Inbound channels arrive
/// provider-prefixed (`telegram:<chat>`), and the route's existence is a
/// per-provider fact, so latch on the prefix — same reasoning as
/// [`channel_supports_progressive_ui`].
pub(super) fn edit_capability_key(channel: &str) -> String {
    channel.split(':').next().unwrap_or(channel).to_string()
}

/// `true` once this process has learned the backend serves no edit route for
/// `channel`'s provider. Callers should skip the request entirely.
pub(super) fn channel_edits_unsupported(channel: &str) -> bool {
    EDIT_UNSUPPORTED_PROVIDERS
        .get_or_init(Default::default)
        .lock()
        .map(|set| set.contains(&edit_capability_key(channel)))
        .unwrap_or(false)
}

/// Latch `channel`'s provider as having no message-edit route.
pub(super) fn mark_channel_edits_unsupported(channel: &str) {
    let key = edit_capability_key(channel);
    if let Ok(mut set) = EDIT_UNSUPPORTED_PROVIDERS
        .get_or_init(Default::default)
        .lock()
    {
        if set.insert(key.clone()) {
            tracing::warn!(
                "[channel-inbound][edit] backend serves no message-edit route for provider='{}' — \
                 progressive edits disabled for this process; placeholders will be posted once and \
                 cleaned up by delete instead (#5230)",
                key,
            );
        }
    }
}
