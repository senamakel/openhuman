//! The per-message pipeline: typing indicator, ACK reaction, history
//! assembly, dispatching the agentic turn over the native bus, and
//! delivering the draft/final reply.

use crate::agent::bus::AgentTurnResponse;
use crate::channels::context::{
    compact_sender_history, is_context_window_overflow_error, ChannelRuntimeContext,
};
use crate::channels::outbound::text_message;
use crate::channels::routes::{get_or_create_turn_model_source, get_route_selection};
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::progress::AgentProgress;
use crate::util::truncate_with_ellipsis;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tinyagents_session::transcript::TranscriptMessage;
use tinychannels_bus::context::ChatMessage;
use tinychannels_bus::ChannelInboundEnvelope;
use tokio_util::sync::CancellationToken;
use tokio_util::task::AbortOnDropHandle;

use super::super::helpers::{
    build_channel_context_block, log_worker_join_result, spawn_typing_task,
    REPLY_LOG_TRUNCATE_CHARS,
};
use super::super::host_agent::{self, run_host_agent_turn, seed_rows, HostChannelTurn, HostRoute};
use super::admit::{admit_runtime_message, AdmittedInbound};
use super::approval::channel_has_approval_surface;
use super::bus_turn::dispatch_bus_turn;
use super::RuntimeChannelMessage;

pub(crate) async fn process_channel_message(
    ctx: Arc<ChannelRuntimeContext>,
    msg: ChannelInboundEnvelope,
) {
    process_channel_runtime_message(
        ctx,
        RuntimeChannelMessage {
            message: msg,
            authenticated_relay: false,
            prepared_inbound: None,
        },
    )
    .await;
}

pub(crate) async fn process_channel_runtime_message(
    ctx: Arc<ChannelRuntimeContext>,
    runtime_msg: RuntimeChannelMessage,
) {
    if let Some(admitted) = admit_runtime_message(Arc::clone(&ctx), runtime_msg).await {
        run_admitted_turn(ctx, admitted).await;
    }
}

/// Run the agent turn for an admitted (possibly merged) message and deliver
/// its reply.
pub(crate) async fn run_admitted_turn(ctx: Arc<ChannelRuntimeContext>, admitted: AdmittedInbound) {
    let AdmittedInbound { msg, history_key } = admitted;
    let target_channel = ctx.channels_by_name.get(&msg.channel.id).cloned();

    // A channel bound to a host agent nobody answers for is refused before
    // any typing or provider work; see `host_agent`.
    let bound_agent = match host_agent::route(ctx.as_ref(), &msg.channel.id) {
        HostRoute::Unbound => None,
        HostRoute::Agent(host) => Some(host),
        HostRoute::Missing(agent_id) => {
            host_agent::refuse_missing_agent(
                ctx.as_ref(),
                &msg,
                target_channel.as_ref(),
                &agent_id,
            )
            .await;
            return;
        }
    };

    // Fire typing indicator as early as possible — before any async I/O — so the
    // user sees feedback immediately regardless of how fast the LLM responds.
    if let Some(channel) = target_channel.as_ref() {
        if let Err(e) = channel.start_typing(&msg.conversation.id).await {
            tracing::debug!(
                "[dispatch] Early typing start failed on {}: {e}",
                channel.name()
            );
        }
    }

    // Send a smart acknowledgment reaction immediately so the user knows the message
    // was received and understood. The LLM may override this later by including its
    // own [REACTION:...] marker, which Telegram replaces atomically.
    // Keep the reaction owned by this worker so logout also aborts a pending send.
    let mut _acknowledgment_task = None;
    if let Some(channel) = target_channel.as_ref() {
        if channel.supports_reactions() && msg.conversation.thread_id.is_some() {
            let ack_result = crate::channels::context::module_client(&ctx)
                .await
                .call_confidential::<tinychannels_bus::module_control::ChannelControlResult>(
                    "tinychannels",
                    tinychannels_bus::methods::PREPARE_CHANNEL_CONTROL,
                    (
                        tinychannels_bus::module_control::ChannelControlRequest::Acknowledgment {
                            content: msg.text.clone(),
                        },
                    ),
                )
                .await;
            let ack_emoji = match ack_result {
                Ok(tinychannels_bus::module_control::ChannelControlResult::Acknowledgment(
                    emoji,
                )) => Some(emoji),
                Ok(_) => {
                    crate::modules::report::malformed_reply("tinychannels");
                    None
                }
                Err(_) => None,
            };
            if let Some(ack_emoji) = ack_emoji {
                tracing::debug!(
                    channel = msg.channel.id,
                    emoji = ack_emoji,
                    "[dispatch] Sending acknowledgment reaction"
                );
                let react_content = format!("[REACTION:{ack_emoji}]");
                let channel_for_react = Arc::clone(channel);
                let react_msg = text_message(
                    &msg.channel.id,
                    &msg.conversation.id,
                    react_content,
                    msg.conversation.thread_id.clone(),
                );
                _acknowledgment_task = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                    if let Err(e) = channel_for_react.send_typed(&react_msg).await {
                        tracing::debug!("[dispatch] Acknowledgment reaction failed: {e}");
                    }
                })));
            }
        }
    }

    let route = get_route_selection(ctx.as_ref(), &history_key);
    let active_turn_model_source = if ctx.config.is_none() {
        match get_or_create_turn_model_source(ctx.as_ref(), &route.provider).await {
            Ok(source) => Some(source),
            Err(err) => {
                crate::core::observability::report_error(
                    &err,
                    "channels",
                    "provider_init",
                    &[
                        ("channel", msg.channel.id.as_str()),
                        ("provider", route.provider.as_str()),
                    ],
                );
                let safe_err = tinyinference_core::sanitize::sanitize_api_error(&err.to_string());
                let message = format!(
                    "⚠️ Failed to initialize provider `{}`. Please run `/models` to choose another provider.\nDetails: {safe_err}",
                    route.provider
                );
                if let Some(channel) = target_channel.as_ref() {
                    let _ = channel
                        .send_typed(&text_message(
                            &msg.channel.id,
                            &msg.conversation.id,
                            message,
                            msg.conversation.thread_id.clone(),
                        ))
                        .await;
                }
                return;
            }
        }
    } else {
        None
    };

    // No per-turn memory recall or autosave: memory v2 reaches the session
    // through `context.md` (injected by the session host) and ingests the
    // committed turn from `ConversationTurnCommitted`.
    let channel_context = build_channel_context_block(&msg);
    let enriched_message = if channel_context.is_empty() {
        msg.text.clone()
    } else {
        format!("{channel_context}{}", msg.text)
    };

    println!("  ⏳ Processing message...");
    let started_at = Instant::now();

    // Build history from per-sender conversation cache
    let mut prior_turns: Vec<TranscriptMessage> = ctx
        .conversation_histories
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&history_key)
        .map(|turns| {
            // The cache holds the bus's plain `ChatMessage`; convert at the
            // harness boundary.
            turns
                .iter()
                .map(|turn| match turn.role.as_str() {
                    "assistant" => TranscriptMessage::assistant(&turn.content),
                    _ => TranscriptMessage::user(&turn.content),
                })
                .collect()
        })
        .unwrap_or_default();

    // Re-rendered by `ChannelSystemPrompt` only when the active profile or an
    // identity file changed since the last message (#6028); otherwise the
    // same bytes as the previous turn.
    let system_prompt = ctx.system_prompt.current();
    let mut history = vec![TranscriptMessage::system(system_prompt.as_str())];
    history.append(&mut prior_turns);
    history.push(TranscriptMessage::user(&enriched_message));

    // Determine if this channel supports streaming draft updates
    let use_streaming = target_channel
        .as_ref()
        .is_some_and(|ch| ch.supports_draft_updates());

    // Set up streaming channel if supported
    let (progress_tx, progress_rx) = if use_streaming {
        let (tx, rx) = tokio::sync::mpsc::channel::<AgentProgress>(64);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // Send initial draft message if streaming
    let draft_message_id = if use_streaming {
        if let Some(channel) = target_channel.as_ref() {
            match crate::channels::outbound::send_draft(
                channel.as_ref(),
                &msg.conversation.id,
                "...",
                msg.conversation.thread_id.clone(),
            )
            .await
            {
                Ok(id) => id,
                Err(e) => {
                    tracing::debug!("Failed to send draft on {}: {e}", channel.name());
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    // Spawn a task to forward streaming progress to draft updates
    let draft_updater = if let (Some(mut rx), Some(draft_id_ref), Some(channel_ref)) = (
        progress_rx,
        draft_message_id.as_deref(),
        target_channel.as_ref(),
    ) {
        let channel = Arc::clone(channel_ref);
        let reply_target = msg.conversation.id.clone();
        let draft_id = draft_id_ref.to_string();
        Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let mut accumulated = String::new();
            let mut last_thinking_update = None;
            const THINKING_UPDATE_INTERVAL_MS: u128 = 2000;

            while let Some(progress) = rx.recv().await {
                match progress {
                    AgentProgress::TextDelta { delta, .. } => {
                        accumulated.push_str(&delta);
                        if let Err(e) = channel
                            .update_draft(&reply_target, &draft_id, &accumulated)
                            .await
                        {
                            tracing::debug!("Draft update failed: {e}");
                        }
                    }
                    AgentProgress::ThinkingDelta { .. } => {
                        // Suppress thinking text to Telegram; only show a placeholder if we haven't
                        // started receiving the final answer yet.
                        if accumulated.is_empty() {
                            let now = std::time::Instant::now();
                            let should_update = match last_thinking_update {
                                None => true,
                                Some(last) => {
                                    now.duration_since(last).as_millis()
                                        > THINKING_UPDATE_INTERVAL_MS
                                }
                            };

                            if should_update {
                                if let Err(e) = channel
                                    .update_draft(&reply_target, &draft_id, "Thinking...")
                                    .await
                                {
                                    tracing::debug!("Thinking update failed: {e}");
                                }
                                last_thinking_update = Some(now);
                            }
                        }
                    }
                    AgentProgress::ToolCallStarted { tool_name, .. } if accumulated.is_empty() => {
                        let _ = channel
                            .update_draft(
                                &reply_target,
                                &draft_id,
                                &format!("Working ({})...", tool_name),
                            )
                            .await;
                    }
                    _ => {}
                }
            }
        })))
    } else {
        None
    };

    let typing_cancellation = target_channel.as_ref().map(|_| CancellationToken::new());
    let _typing_cancel_on_drop = typing_cancellation
        .as_ref()
        .map(|token| token.clone().drop_guard());
    // Typing was already started early (before memory/provider setup). Here we only
    // spawn the background refresh task that keeps the indicator alive during long turns.
    let typing_task = match (target_channel.as_ref(), typing_cancellation.as_ref()) {
        (Some(channel), Some(token)) => Some(AbortOnDropHandle::new(spawn_typing_task(
            &ctx.dispatch_ctx(),
            Arc::clone(channel),
            msg.conversation.id.clone(),
            token.clone(),
        ))),
        _ => None,
    };

    // Non-web channel turns label themselves as `ExternalChannel` so the
    // approval gate's origin-aware decision tree treats the inbound text
    // as untrusted (remote-attacker-controlled). Cron-driven channel
    // deliveries get a `TrustedAutomation { Cron }` label from the
    // scheduler instead and never reach this dispatch path.
    // Per-sender provenance flows into the origin so a co-channel attacker
    // who reads a leaked `quote_id` / approval prompt from a shared Discord /
    // Slack channel cannot use it from their own session — distinct senders
    // produce distinct origins, and the wallet preparer / parked-approval
    // gates compare these for equality before honouring confirmations.
    let turn_origin = crate::security::turn_origin::AgentTurnOrigin::ExternalChannel {
        sender_name: msg.sender.name.clone(),
        channel: msg.channel.id.clone(),
        sender: Some(msg.sender.id.clone()),
        reply_target: msg.conversation.id.clone(),
        message_id: msg.message_id.clone(),
        history_key: Some(history_key.clone()),
    };

    // Sub-issue 2 of #3098: give the agent turn an `ApprovalTurnContext` for
    // every channel with the `chat_approvals` capability (served by
    // `ChannelApprovalSurfaceSubscriber`). Without a chat context the gate's
    // "no chat context -> allow straight through" branch silently bypasses
    // every `Prompt`-class tool call, voiding the `supervised` autonomy tier
    // on the channel.
    let approval = if target_channel
        .as_ref()
        .is_some_and(|channel| channel_has_approval_surface(&channel.capabilities()))
    {
        crate::security::approval::ApprovalTurnContext::chat(
            crate::security::approval::ApprovalChatContext {
                thread_id: history_key.clone(),
                client_id: msg.channel.id.clone(),
                request_id: Some(msg.message_id.clone()),
            },
        )
    } else {
        crate::security::approval::ApprovalTurnContext::default()
    };

    // A channel bound to a host agent (`agent.channel_agents`) runs as that
    // agent; every other channel goes over the bus to the orchestrator.
    let agent_call: std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<AgentTurnResponse>> + Send + '_>,
    > = match bound_agent {
        Some(host) => Box::pin(run_host_agent_turn(
            *host,
            HostChannelTurn {
                relay_thread: ctx.relay_thread.clone(),
                channel: msg.channel.id.clone(),
                history_key: history_key.clone(),
                prior_turns: seed_rows(&history),
                message: enriched_message.clone(),
                origin: turn_origin,
                progress: progress_tx,
                approval,
            },
        )),
        None => Box::pin(dispatch_bus_turn(
            ctx.as_ref(),
            &msg,
            &route,
            history,
            active_turn_model_source,
            progress_tx,
            turn_origin,
            approval,
        )),
    };
    let llm_result =
        tokio::time::timeout(Duration::from_secs(ctx.message_timeout_secs), agent_call).await;

    // Wait for draft updater to finish
    if let Some(handle) = draft_updater {
        let _ = handle.await;
    }

    if let Some(token) = typing_cancellation.as_ref() {
        token.cancel();
    }
    if let Some(handle) = typing_task {
        log_worker_join_result(handle.await);
    }

    let (success, response_text, response_provider, response_model) = match llm_result {
        Ok(Ok(response)) => {
            let resolved_provider = response
                .resolved_provider
                .clone()
                .unwrap_or_else(|| route.provider.clone());
            let resolved_model = response
                .resolved_model
                .clone()
                .unwrap_or_else(|| route.model.clone());
            let response_text = response.text;
            // Save user + assistant turn to per-sender history
            {
                let mut histories = ctx
                    .conversation_histories
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let turns = histories.entry(history_key).or_default();
                turns.push(ChatMessage::user(&enriched_message));
                turns.push(ChatMessage::assistant(&response_text));
                // Trim to MAX_CHANNEL_HISTORY (keep recent turns)
                while turns.len() > crate::channels::context::MAX_CHANNEL_HISTORY {
                    turns.remove(0);
                }
            }
            println!(
                "  🤖 Reply ({}ms): {}",
                started_at.elapsed().as_millis(),
                truncate_with_ellipsis(&response_text, REPLY_LOG_TRUNCATE_CHARS)
            );
            if let Some(channel) = target_channel.as_ref() {
                if let Some(ref draft_id) = draft_message_id {
                    if let Err(e) = channel
                        .finalize_draft(
                            &msg.conversation.id,
                            draft_id,
                            &response_text,
                            msg.conversation.thread_id.as_deref(),
                        )
                        .await
                    {
                        tracing::warn!("Failed to finalize draft: {e}; sending as new message");
                        let _ = channel
                            .send_typed(&text_message(
                                &msg.channel.id,
                                &msg.conversation.id,
                                &response_text,
                                msg.conversation.thread_id.clone(),
                            ))
                            .await;
                    }
                } else if let Err(e) = channel
                    .send_typed(&text_message(
                        &msg.channel.id,
                        &msg.conversation.id,
                        &response_text,
                        msg.conversation.thread_id.clone(),
                    ))
                    .await
                {
                    eprintln!("  ❌ Failed to reply on {}: {e}", channel.name());
                }
            }
            (true, response_text, resolved_provider, resolved_model)
        }
        Ok(Err(e)) => {
            if is_context_window_overflow_error(&ctx, &e)
                .await
                .unwrap_or(false)
            {
                let compacted = compact_sender_history(&ctx.conversation_histories, &history_key);
                let error_text = if compacted {
                    "⚠️ Context window exceeded for this conversation. I compacted recent history and kept the latest context. Please resend your last message."
                } else {
                    "⚠️ Context window exceeded for this conversation. Please resend your last message."
                };
                eprintln!(
                    "  ⚠️ Context window exceeded after {}ms; sender history compacted={}",
                    started_at.elapsed().as_millis(),
                    compacted
                );
                if let Some(channel) = target_channel.as_ref() {
                    if let Some(ref draft_id) = draft_message_id {
                        let _ = channel
                            .finalize_draft(
                                &msg.conversation.id,
                                draft_id,
                                error_text,
                                msg.conversation.thread_id.as_deref(),
                            )
                            .await;
                    } else {
                        let _ = channel
                            .send_typed(&text_message(
                                &msg.channel.id,
                                &msg.conversation.id,
                                error_text,
                                msg.conversation.thread_id.clone(),
                            ))
                            .await;
                    }
                }

                BUS.publish(DomainEvent::ChannelMessageProcessed {
                    channel: msg.channel.id.clone(),
                    message_id: msg.message_id.clone(),
                    sender: msg.sender.id.clone(),
                    reply_target: msg.conversation.id.clone(),
                    content: msg.text.clone(),
                    thread_ts: msg.conversation.thread_id.clone(),
                    response: error_text.to_string(),
                    provider: route.provider.clone(),
                    model: route.model.clone(),
                    elapsed_ms: started_at.elapsed().as_millis() as u64,
                    success: false,
                    workspace_dir: ctx.workspace_dir.as_ref().clone(),
                });
                return;
            }

            let error_response = format!("⚠️ Error: {e}");
            eprintln!(
                "  ❌ LLM error after {}ms: {e}",
                started_at.elapsed().as_millis()
            );
            // The typed `AgentError` is flattened to a `String` at the
            // native-bus boundary (`agent::bus` map_err → `e.to_string()`),
            // so the downcast that works in `OpenHumanSessionHost::run_single` is not an
            // option here — fall back to canonical-phrase substring match.
            // The max-tool-iterations cap is a deterministic agent-state
            // outcome and is already surfaced to the user as the
            // chat-rendered "⚠️ Error: …" message just above. Skip the
            // Sentry funnel (OPENHUMAN-TAURI-98) and emit `log::info!`
            // instead — `Err` propagation through the surrounding match
            // arm is unchanged.
            if tinyagents_harness::agent_error::is_max_iterations_error(&e.to_string()) {
                log::info!(
                    target: "channels",
                    "[channels.dispatch] suppressed Sentry emission for max-iteration cap \
                     channel={} provider={} message={}",
                    msg.channel.id.as_str(),
                    route.provider.as_str(),
                    e
                );
            } else {
                // Route through `report_error_or_expected` so
                // transient-upstream provider HTTP errors that bubbled
                // up via `agent.run_single` (`OpenHuman API error
                // (502 Bad Gateway): …`) get demoted via
                // `is_transient_upstream_http_message` — the agent
                // re-emit at the dispatch layer was previously
                // unconditionally calling `report_error`, which firehoses
                // Sentry under `domain="channels"` even though the same
                // chain was already classified at the provider + agent
                // layers (OPENHUMAN-TAURI-4F ~157ev / -1C ~87ev / -8F
                // ~39ev: provider 5xx that the reliable layer retried
                // and exhausted, then the channels layer re-reported as
                // a fresh per-attempt event). Genuine bugs (404 / 500
                // / unrelated agent failures) still surface — the
                // classifier only demotes the canonical transient
                // shapes documented in
                // `crate::core::observability::expected_error_kind`.
                crate::core::observability::report_error_or_expected(
                    &e,
                    "channels",
                    "dispatch_llm_error",
                    &[
                        ("channel", msg.channel.id.as_str()),
                        ("provider", route.provider.as_str()),
                    ],
                );
            }
            if let Some(channel) = target_channel.as_ref() {
                if let Some(ref draft_id) = draft_message_id {
                    let _ = channel
                        .finalize_draft(
                            &msg.conversation.id,
                            draft_id,
                            &error_response,
                            msg.conversation.thread_id.as_deref(),
                        )
                        .await;
                } else {
                    let _ = channel
                        .send_typed(&text_message(
                            &msg.channel.id,
                            &msg.conversation.id,
                            &error_response,
                            msg.conversation.thread_id.clone(),
                        ))
                        .await;
                }
            }
            (
                false,
                error_response,
                route.provider.clone(),
                route.model.clone(),
            )
        }
        Err(_) => {
            let timeout_msg = format!("LLM response timed out after {}s", ctx.message_timeout_secs);
            eprintln!(
                "  ❌ {} (elapsed: {}ms)",
                timeout_msg,
                started_at.elapsed().as_millis()
            );
            crate::core::observability::report_error(
                timeout_msg.as_str(),
                "channels",
                "dispatch_llm_timeout",
                &[
                    ("channel", msg.channel.id.as_str()),
                    ("timeout_secs", &ctx.message_timeout_secs.to_string()),
                ],
            );
            let error_text =
                "⚠️ Request timed out while waiting for the model. Please try again.".to_string();
            if let Some(channel) = target_channel.as_ref() {
                if let Some(ref draft_id) = draft_message_id {
                    let _ = channel
                        .finalize_draft(
                            &msg.conversation.id,
                            draft_id,
                            &error_text,
                            msg.conversation.thread_id.as_deref(),
                        )
                        .await;
                } else {
                    let _ = channel
                        .send_typed(&text_message(
                            &msg.channel.id,
                            &msg.conversation.id,
                            &error_text,
                            msg.conversation.thread_id.clone(),
                        ))
                        .await;
                }
            }
            (
                false,
                error_text,
                route.provider.clone(),
                route.model.clone(),
            )
        }
    };

    BUS.publish(DomainEvent::ChannelMessageProcessed {
        channel: msg.channel.id.clone(),
        message_id: msg.message_id.clone(),
        sender: msg.sender.id.clone(),
        reply_target: msg.conversation.id.clone(),
        content: msg.text.clone(),
        thread_ts: msg.conversation.thread_id.clone(),
        response: response_text,
        provider: response_provider,
        model: response_model,
        elapsed_ms: started_at.elapsed().as_millis() as u64,
        success,
        workspace_dir: ctx.workspace_dir.as_ref().clone(),
    });
}
