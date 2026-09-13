//! The `ChannelInboundSubscriber` event handler: runs the agent loop for an
//! inbound channel message and streams the reply back through the REST API.

use super::delivery::{finalize_channel_reply, send_channel_reply};
use super::draft::flush_streaming_edit;
use super::filler::send_filler_message;
use super::progressive_ui::{
    channel_supports_progressive_ui, EDIT_FLUSH_INTERVAL, FILLER_INTERVAL, TYPING_REFRESH_INTERVAL,
};
use super::streaming_state::{send_typing_indicator, StreamingState, TypingState};
use super::thinking::flush_thinking_message;
use super::thread_id::{derive_inbound_client_id, derive_inbound_thread_id};
use crate::core::events::DomainEvent;
use async_trait::async_trait;
use tinybus::EventHandler;

/// Subscribes to `ChannelInboundMessage` events and runs the agent loop,
/// sending replies back to the originating channel via the backend REST API.
pub struct ChannelInboundSubscriber;

impl Default for ChannelInboundSubscriber {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelInboundSubscriber {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventHandler<DomainEvent> for ChannelInboundSubscriber {
    fn name(&self) -> &str {
        "channel::inbound_handler"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["channel"])
    }

    async fn handle(&self, event: &DomainEvent) {
        let DomainEvent::ChannelInboundMessage {
            event_name: _,
            channel,
            message,
            sender,
            reply_target,
            thread_ts,
            raw_data: _,
        } = event
        else {
            return;
        };

        tracing::info!(
            "[channel-inbound] received message from channel='{}' sender={} len={}",
            channel,
            sender.as_deref().unwrap_or("<unknown>"),
            message.len()
        );

        // Mirror `channels::context::conversation_history_key`: the inbound
        // path must key on `(channel, sender, reply_target, thread_ts)` —
        // not channel alone — or distinct participants in a shared
        // Discord / Slack channel get collapsed into one cached agent
        // session, and the second sender resumes the first's in-flight
        // state (including any prepared wallet quote).
        //
        // Legacy publishers that don't fill in `sender` fall back to the
        // old channel-only key so existing single-DM flows keep working.
        let thread_id = derive_inbound_thread_id(
            channel,
            sender.as_deref(),
            reply_target.as_deref(),
            thread_ts.as_deref(),
        );
        // Per-sender client_id so the `AGENT_TURN_ORIGIN.WebChat.client_id`
        // and the wallet `QuoteOwner.client_id` paired with it differ across
        // distinct senders in the same shared channel. The thread_id is
        // already per-sender via `derive_inbound_thread_id`, and the
        // wallet/approval gates compare both halves of the (thread_id,
        // client_id) owner pair for equality — but a single shared
        // `client_id="inbound"` collapses the surface for any downstream
        // consumer that keys on client_id alone (audit logs, future
        // session-scoped caches, etc.). Build a stable per-sender label
        // here so the surface stays segregated end-to-end.
        let client_id = derive_inbound_client_id(channel, sender.as_deref());

        let mut event_rx = crate::web_chat::subscribe_web_channel_events();

        let request_id = match crate::web_chat::start_chat(
            &client_id,
            &thread_id,
            message,
            None,
            None,
            None,
            None,
            None,
            crate::web_chat::ChatRequestMetadata {
                // Tag inbound provider messages so traces classify as
                // run:channel_inbound instead of interactive chat.
                source: Some("channel_inbound".to_string()),
                ..Default::default()
            },
        )
        .await
        {
            Ok(rid) => {
                tracing::debug!(
                    "[channel-inbound] agent started request_id={} thread={}",
                    rid,
                    thread_id
                );
                rid
            }
            Err(err) => {
                tracing::error!("[channel-inbound] start_chat failed: {}", err);
                send_channel_reply(
                    channel,
                    &format!("Sorry, I couldn't process your message: {err}"),
                )
                .await;
                return;
            }
        };

        let timeout = tokio::time::Duration::from_secs(180);
        let deadline = tokio::time::Instant::now() + timeout;

        // ── Progressive-edit streaming state ──────────────────────────
        // We buffer text/tool deltas and flush them as edits on a
        // timer. If the first edit fails (e.g. the backend doesn't
        // implement the PATCH endpoint for this channel) we latch into
        // `edit_disabled` and fall back to atomic-final delivery.
        let mut streaming_state = StreamingState::default();
        let mut edit_timer = tokio::time::interval(EDIT_FLUSH_INTERVAL);
        edit_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Don't fire immediately; wait for the first tick.
        edit_timer.tick().await;

        // ── Typing indicator state ────────────────────────────────────
        // Telegram's `sendChatAction` keeps the "typing…" UI alive for
        // ~5s, so we re-send every 4s while the turn is in flight. The
        // first call fires immediately; on repeated failures we latch
        // `typing_disabled` to stop hitting a backend that doesn't
        // support it.
        let mut typing_state = TypingState::default();
        let mut typing_timer = tokio::time::interval(TYPING_REFRESH_INTERVAL);
        typing_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // Fire immediately on first tick so the indicator shows up as
        // soon as the inbound message is received.
        send_typing_indicator(channel, &mut typing_state).await;
        typing_timer.tick().await; // consume the immediate tick

        // ── Filler messages ──────────────────────────────────────────
        // Once progressive edits + thinking streams go quiet (backend
        // doesn't support PATCH, reasoning has finished, etc.) the user
        // can wait 30–90 s seeing no fresh activity. Post a short filler
        // every FILLER_INTERVAL so the chat keeps moving. All filler ids
        // are tracked in `StreamingState.filler_message_ids` and deleted
        // in `finalize_channel_reply` once the real response is on screen.
        let mut filler_timer = tokio::time::interval(FILLER_INTERVAL);
        filler_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        filler_timer.tick().await; // consume the immediate tick — first filler fires after FILLER_INTERVAL

        loop {
            tokio::select! {
                event = event_rx.recv() => {
                    match event {
                        Ok(ev) if ev.request_id == request_id => {
                            match ev.event.as_str() {
                                "text_delta" => {
                                    if let Some(delta) = ev.delta.as_ref() {
                                        streaming_state.content.push_str(delta);
                                        streaming_state.dirty = true;
                                    }
                                }
                                "tool_call" => {
                                    if let Some(ref name) = ev.tool_name {
                                        streaming_state.last_tool = Some(format!("🔧 {name}…"));
                                        streaming_state.dirty = true;
                                    }
                                }
                                "tool_result" => {
                                    if let Some(ref name) = ev.tool_name {
                                        let ok = ev.success.unwrap_or(true);
                                        streaming_state.last_tool = Some(if ok {
                                            format!("🔧 {name} ✓")
                                        } else {
                                            format!("🔧 {name} ✗")
                                        });
                                        streaming_state.dirty = true;
                                    }
                                }
                                "thinking_delta" => {
                                    if let Some(delta) = ev.delta.as_ref() {
                                        streaming_state.thinking_accumulator.push_str(delta);
                                        streaming_state.thinking_dirty = true;
                                    }
                                }
                                "chat_done" | "chat:done" => {
                                    let reply = ev.full_response.unwrap_or_default();
                                    // Even when the agent produced no visible
                                    // text, we must close out any draft we
                                    // already posted — otherwise the user is
                                    // left staring at a stale "_working…_"
                                    // message indefinitely.
                                    let reply_text = if reply.trim().is_empty() {
                                        tracing::warn!(
                                            "[channel-inbound] agent returned empty response — finalizing draft with fallback",
                                        );
                                        "(No response from agent.)"
                                    } else {
                                        reply.as_str()
                                    };
                                    tracing::info!(
                                        "[channel-inbound] agent done, replying to channel='{}' len={} streamed_msg_id={:?}",
                                        channel,
                                        reply_text.len(),
                                        streaming_state.message_id,
                                    );
                                    // If we've been streaming progressive edits, replace
                                    // the outbound message with the final canonical text.
                                    // Otherwise send a fresh message atomically.
                                    finalize_channel_reply(
                                        channel,
                                        &mut streaming_state,
                                        reply_text,
                                    )
                                    .await;
                                    return;
                                }
                                "chat_error" | "chat:error" => {
                                    let err_msg = ev.message.unwrap_or_else(|| "unknown error".to_string());
                                    tracing::error!("[channel-inbound] agent error: {}", err_msg);
                                    let reply = format!("Sorry, I encountered an error: {err_msg}");
                                    finalize_channel_reply(channel, &mut streaming_state, &reply)
                                        .await;
                                    return;
                                }
                                _ => {}
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!("[channel-inbound] event bus lagged, skipped {} events", n);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::error!("[channel-inbound] event bus closed unexpectedly");
                            return;
                        }
                    }
                }
                _ = edit_timer.tick() => {
                    // Progressive draft/thinking bubbles require edit+delete
                    // support; skip them on channels that lack it (Discord) so
                    // they don't leave un-cleanable placeholder messages.
                    if channel_supports_progressive_ui(channel) {
                        if streaming_state.thinking_dirty && !streaming_state.thinking_edit_disabled {
                            flush_thinking_message(channel, &mut streaming_state).await;
                        }
                        if streaming_state.dirty && !streaming_state.edit_disabled {
                            flush_streaming_edit(channel, &mut streaming_state).await;
                        }
                    }
                }
                _ = typing_timer.tick() => {
                    if !typing_state.disabled {
                        send_typing_indicator(channel, &mut typing_state).await;
                    }
                }
                _ = filler_timer.tick() => {
                    // Fillers ("💭 Still working on it…") are ephemeral and
                    // deleted on finalize — only post them where cleanup works.
                    if channel_supports_progressive_ui(channel) && !streaming_state.filler_disabled {
                        send_filler_message(channel, &mut streaming_state).await;
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    tracing::error!("[channel-inbound] agent timed out after {}s", timeout.as_secs());
                    let reply = "Sorry, the request timed out.";
                    finalize_channel_reply(channel, &mut streaming_state, reply).await;
                    return;
                }
            }
        }
    }
}
