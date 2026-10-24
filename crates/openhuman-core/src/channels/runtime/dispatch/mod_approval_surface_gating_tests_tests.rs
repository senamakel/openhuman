use super::channel_has_approval_surface;
use tinychannels_bus::ChannelCapabilities;

// Sub-issue 2 of #3098: this gate is what decides whether the dispatch
// loop sets an `ApprovalChatContext` (→ gate fires for `Prompt`-class
// tools) versus the legacy bypass (→ tool calls silently allowed).
// Pin the matrix: every chat channel is served by
// `ChannelApprovalSurfaceSubscriber`; channels with no conversational reply
// path must not be scoped, or every parked call would TTL-deny.

#[test]
fn configured_instances_with_chat_approvals_have_an_approval_surface() {
    let mut capabilities = ChannelCapabilities::unknown();
    capabilities.chat_approvals = true;
    assert!(channel_has_approval_surface(&capabilities));
}

#[test]
fn unknown_or_unconfigured_instances_do_not_have_an_approval_surface() {
    assert!(!channel_has_approval_surface(
        &ChannelCapabilities::unknown()
    ));
}
