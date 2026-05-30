//! The unified turn loop.
//!
//! [`run_turn_engine`] is the single agentic loop the harness runs: announce the
//! turn, then per iteration run the stop-hook + context guards, send the
//! provider request (streaming deltas when a sink exists), parse the response,
//! either return the final text or execute every requested tool through the
//! [`ToolSource`] and loop again — bailing early via the shared repeated-failure
//! circuit breaker, or returning `MaxIterationsExceeded` at the cap.
//!
//! This body was moved verbatim (behavior-preserving) out of the canonical
//! `run_tool_call_loop`; the only seam introduced so far is [`ToolSource`],
//! which owns tool advertisement + per-call execution. Progress events, history
//! shaping, context management and the max-iteration outcome are still the
//! channel loop's behavior inline; later phases lift those behind their own
//! seams as the subagent and `Agent` paths need them.

use anyhow::Result;
use std::fmt::Write as _;
use std::io::Write as _;

use crate::openhuman::agent::cost::TurnCost;
use crate::openhuman::agent::multimodal;
use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::agent::stop_hooks::{current_stop_hooks, StopDecision, TurnState};
use crate::openhuman::context::guard::{ContextCheckResult, ContextGuard};
use crate::openhuman::inference::model_context::context_window_for_model;
use crate::openhuman::inference::provider::{ChatMessage, ChatRequest, Provider, ProviderCapabilityError};

use super::super::parse::{build_native_assistant_history, parse_structured_tool_calls, parse_tool_calls};
use super::super::token_budget::trim_chat_messages_to_budget;
use super::super::tool_loop::{RepeatFailureGuard, STREAM_CHUNK_MIN_CHARS};
use super::tool_source::ToolSource;

/// Run the agent loop over `history` using `tools`. `max_iterations` must be
/// pre-normalized (callers map `0` to a sane default). See the module docs for
/// the per-iteration flow.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_turn_engine(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools: &mut dyn ToolSource,
    provider_name: &str,
    model: &str,
    temperature: f64,
    silent: bool,
    multimodal_config: &crate::openhuman::config::MultimodalConfig,
    max_iterations: usize,
    on_delta: Option<tokio::sync::mpsc::Sender<String>>,
    on_progress: Option<tokio::sync::mpsc::Sender<AgentProgress>>,
) -> Result<String> {
    let use_native_tools = provider.supports_native_tools() && !tools.request_specs().is_empty();

    let mut context_guard = context_window_for_model(model)
        .map(ContextGuard::with_context_window)
        .unwrap_or_else(ContextGuard::new);
    let mut turn_cost = TurnCost::new();

    // Announce turn start to progress subscribers (if any). We use
    // `send().await` for lifecycle (turn/iteration) events so they survive
    // downstream backpressure — dropping one of these would desync the
    // web-channel progress bridge. High-volume delta events use the same
    // backpressure discipline (see below).
    if let Some(ref sink) = on_progress {
        if let Err(e) = sink.send(AgentProgress::TurnStarted).await {
            log::warn!("[agent_loop] progress sink closed at TurnStarted: {e}");
        }
    }

    let stop_hooks = current_stop_hooks();
    // Repeated-failure circuit breaker — halts with a root cause rather than
    // grinding to `max_iterations` (shared with the subagent loop).
    let mut failure_guard = RepeatFailureGuard::new();
    let mut halt_reason: Option<String> = None;
    for iteration in 0..max_iterations {
        if let Some(ref sink) = on_progress {
            if let Err(e) = sink
                .send(AgentProgress::IterationStarted {
                    iteration: (iteration + 1) as u32,
                    max_iterations: max_iterations as u32,
                })
                .await
            {
                log::warn!("[agent_loop] progress sink closed at IterationStarted: {e}");
            }
        }

        // ── Stop hooks: policy check before the next LLM call ──
        if !stop_hooks.is_empty() {
            let state = TurnState {
                iteration: (iteration + 1) as u32,
                max_iterations: max_iterations as u32,
                cost: &turn_cost,
                model,
            };
            for hook in &stop_hooks {
                match hook.check(&state).await {
                    StopDecision::Continue => {}
                    StopDecision::Stop { reason } => {
                        tracing::warn!(
                            iteration = (iteration + 1),
                            hook = hook.name(),
                            reason = %reason,
                            "[agent_loop] stop hook triggered — aborting turn"
                        );
                        anyhow::bail!("Agent turn stopped by hook '{}': {reason}", hook.name());
                    }
                }
            }
        }

        // ── Context guard: check utilization before each LLM call ──
        match context_guard.check() {
            ContextCheckResult::Ok => {}
            ContextCheckResult::CompactionNeeded => {
                tracing::warn!(
                    iteration,
                    "[agent_loop] context guard: compaction needed (>{:.0}% full)",
                    crate::openhuman::context::guard::COMPACTION_TRIGGER_THRESHOLD * 100.0
                );
                // Compaction is handled by history management upstream;
                // log and continue so the caller can act on it.
            }
            ContextCheckResult::ContextExhausted {
                utilization_pct,
                reason,
            } => {
                let msg = format!("Context window exhausted ({utilization_pct}% full): {reason}");
                crate::core::observability::report_error(
                    msg.as_str(),
                    "agent",
                    "context_exhausted",
                    &[
                        ("provider", provider_name),
                        ("model", model),
                        ("utilization_pct", &utilization_pct.to_string()),
                    ],
                );
                anyhow::bail!(msg);
            }
        }

        if let Some(context_window) = context_window_for_model(model) {
            let budget_outcome = trim_chat_messages_to_budget(history, context_window);
            if budget_outcome.trimmed {
                log::warn!(
                    "[agent_loop] pre-dispatch history trimmed model={} context_window={} original_tokens={} final_tokens={} messages_removed={}",
                    model,
                    context_window,
                    budget_outcome.original_tokens,
                    budget_outcome.final_tokens,
                    budget_outcome.messages_removed
                );
            } else {
                tracing::debug!(
                    iteration,
                    model,
                    context_window,
                    estimated_tokens = budget_outcome.final_tokens,
                    "[agent_loop] pre-dispatch token budget ok"
                );
            }
        }

        tracing::debug!(iteration, "[agent_loop] sending LLM request");
        let image_marker_count = multimodal::count_image_markers(history);
        if image_marker_count > 0 && !provider.supports_vision() {
            let cap_err = ProviderCapabilityError {
                provider: provider_name.to_string(),
                capability: "vision".to_string(),
                message: format!(
                    "received {image_marker_count} image marker(s), but this provider does not support vision input"
                ),
            };
            crate::core::observability::report_error(
                &cap_err,
                "agent",
                "provider_capability",
                &[
                    ("provider", provider_name),
                    ("capability", "vision"),
                    ("model", model),
                ],
            );
            return Err(cap_err.into());
        }

        let prepared_messages =
            multimodal::prepare_messages_for_provider(history, multimodal_config).await?;

        // Unified path via Provider::chat so provider-specific native tool logic
        // (OpenAI/Anthropic/OpenRouter/compatible adapters) is honored.
        let request_tools = if use_native_tools {
            Some(tools.request_specs())
        } else {
            None
        };

        // Wire up a ProviderDelta → AgentProgress forwarder for this iteration
        // when a progress sink exists. Senders dropped after the chat call so
        // the forwarder task exits cleanly.
        let (delta_tx_opt, delta_forwarder) =
            super::spawn_delta_forwarder(on_progress.clone(), (iteration + 1) as u32);

        let chat_result = provider
            .chat(
                ChatRequest {
                    messages: &prepared_messages.messages,
                    tools: request_tools,
                    stream: delta_tx_opt.as_ref(),
                },
                model,
                temperature,
            )
            .await;

        drop(delta_tx_opt);
        if let Some(handle) = delta_forwarder {
            let _ = handle.await;
        }

        let (response_text, parsed_text, tool_calls, assistant_history_content, native_tool_calls) =
            match chat_result {
                Ok(resp) => {
                    // Update context guard with token usage from this response.
                    if let Some(ref usage) = resp.usage {
                        context_guard.update_usage(usage);
                        turn_cost.add_call(model, usage);
                        tracing::debug!(
                            iteration,
                            input_tokens = usage.input_tokens,
                            output_tokens = usage.output_tokens,
                            context_window = usage.context_window,
                            cumulative_usd = turn_cost.total_usd(),
                            "[agent_loop] LLM response received"
                        );
                        if let Some(ref sink) = on_progress {
                            let event = AgentProgress::TurnCostUpdated {
                                model: model.to_string(),
                                iteration: (iteration + 1) as u32,
                                input_tokens: turn_cost.input_tokens,
                                output_tokens: turn_cost.output_tokens,
                                cached_input_tokens: turn_cost.cached_input_tokens,
                                total_usd: turn_cost.total_usd(),
                            };
                            if let Err(e) = sink.send(event).await {
                                log::warn!("[agent_loop] progress sink closed at TurnCostUpdated: {e}");
                            }
                        }
                    } else {
                        tracing::debug!(iteration, "[agent_loop] LLM response received (no usage info)");
                    }

                    let response_text = resp.text_or_empty().to_string();
                    let mut calls = parse_structured_tool_calls(&resp.tool_calls);
                    let mut parsed_text = String::new();

                    if calls.is_empty() {
                        let (fallback_text, fallback_calls) = parse_tool_calls(&response_text);
                        if !fallback_text.is_empty() {
                            parsed_text = fallback_text;
                        }
                        calls = fallback_calls;
                    }

                    tracing::debug!(
                        iteration,
                        native_tool_calls = resp.tool_calls.len(),
                        parsed_tool_calls = calls.len(),
                        "[agent_loop] tool calls parsed"
                    );

                    // Preserve native tool call IDs in assistant history so role=tool
                    // follow-up messages can reference the exact call id.
                    let assistant_history_content = if resp.tool_calls.is_empty() {
                        response_text.clone()
                    } else {
                        build_native_assistant_history(
                            &response_text,
                            resp.reasoning_content.as_deref(),
                            &resp.tool_calls,
                        )
                    };

                    let native_calls = resp.tool_calls;
                    (
                        response_text,
                        parsed_text,
                        calls,
                        assistant_history_content,
                        native_calls,
                    )
                }
                Err(e) => {
                    // Transient upstream failures (rate-limit, gateway 5xx, "no
                    // healthy upstream", etc.) are already classified + retried
                    // by reliable.rs and produce an aggregate Sentry event only
                    // when every provider/model is exhausted. Reporting each
                    // per-iteration provider_chat error here duplicates the
                    // signal and floods Sentry — see OPENHUMAN-TAURI-3Y/3Z.
                    let transient = crate::openhuman::inference::provider::reliable::is_rate_limited(&e)
                        || crate::openhuman::inference::provider::reliable::is_upstream_unhealthy(&e);
                    if transient {
                        tracing::warn!(
                            domain = "agent",
                            operation = "provider_chat",
                            provider = provider_name,
                            model = model,
                            iteration = iteration + 1,
                            error = %format!("{e:#}"),
                            "[agent] transient provider_chat failure — retried upstream; \
                             aggregated all-providers-exhausted will report if applicable"
                        );
                    } else {
                        crate::core::observability::report_error_or_expected(
                            &e,
                            "agent",
                            "provider_chat",
                            &[
                                ("provider", provider_name),
                                ("model", model),
                                ("iteration", &(iteration + 1).to_string()),
                            ],
                        );
                    }
                    return Err(e);
                }
            };

        let display_text = if parsed_text.is_empty() {
            response_text.clone()
        } else {
            parsed_text
        };

        if tool_calls.is_empty() {
            tracing::debug!(iteration, "[agent_loop] no tool calls — returning final response");
            // No tool calls — this is the final response. If a streaming sender
            // is provided, relay the text in small chunks so the channel can
            // progressively update the draft message.
            if let Some(ref tx) = on_delta {
                // Split on whitespace boundaries, accumulating chunks of at
                // least STREAM_CHUNK_MIN_CHARS characters for progressive
                // draft updates.
                let mut chunk = String::new();
                for word in display_text.split_inclusive(char::is_whitespace) {
                    chunk.push_str(word);
                    if chunk.len() >= STREAM_CHUNK_MIN_CHARS
                        && tx.send(std::mem::take(&mut chunk)).await.is_err()
                    {
                        break; // receiver dropped
                    }
                }
                if !chunk.is_empty() {
                    let _ = tx.send(chunk).await;
                }
            }
            history.push(ChatMessage::assistant(response_text.clone()));
            log::info!(
                "[agent_loop] turn complete: iters={} provider_calls={} tokens_in={} tokens_out={} cached_in={} usd={:.4}",
                (iteration + 1),
                turn_cost.call_count,
                turn_cost.input_tokens,
                turn_cost.output_tokens,
                turn_cost.cached_input_tokens,
                turn_cost.total_usd(),
            );
            if let Some(ref sink) = on_progress {
                if let Err(e) = sink
                    .send(AgentProgress::TurnCompleted {
                        iterations: (iteration + 1) as u32,
                    })
                    .await
                {
                    log::warn!("[agent_loop] progress sink closed at TurnCompleted: {e}");
                }
            }
            return Ok(display_text);
        }

        // Print any text the LLM produced alongside tool calls (unless silent)
        if !silent && !display_text.is_empty() {
            print!("{display_text}");
            let _ = std::io::stdout().flush();
        }

        // Execute each tool call and build results.
        // `individual_results` tracks per-call output so that native-mode
        // history can emit one `role: tool` message per tool call with the
        // correct ID.
        let mut tool_results = String::new();
        let mut individual_results: Vec<String> = Vec::new();
        for (call_idx, call) in tool_calls.iter().enumerate() {
            // Stable id threaded through the start/complete pair (and any
            // preceding args-delta events) so consumers can reconcile tool rows
            // by id. The fallback includes `call_idx` to stay unique when the
            // same tool name appears multiple times in one iteration.
            let progress_call_id = call
                .id
                .clone()
                .unwrap_or_else(|| format!("loop-{iteration}-{call_idx}-{}", call.name));

            // The full per-call lifecycle (start event, policy gate, scope
            // guard, approval gate, execute + timeout, scrub/tokenjuice/cap/
            // summarize, audit, completion event) is owned by the ToolSource.
            let outcome = tools
                .execute_call(call, iteration, &on_progress, &progress_call_id)
                .await;

            individual_results.push(outcome.text.clone());
            let _ = writeln!(
                tool_results,
                "<tool_result name=\"{}\">\n{}\n</tool_result>",
                call.name, outcome.text
            );

            // Repeated-failure circuit breaker (shared guard) — halt with a root
            // cause instead of grinding to `max_iterations` on a doomed action.
            if let Some(reason) = failure_guard.record(
                &call.name,
                &call.arguments.to_string(),
                outcome.success,
                &outcome.text,
            ) {
                tracing::warn!(
                    iteration,
                    tool = call.name.as_str(),
                    "[agent_loop] circuit breaker tripped — halting with root cause"
                );
                halt_reason = Some(reason);
            }
        }

        // Add assistant message with tool calls + tool results to history.
        // Native mode: use JSON-structured messages so convert_messages() can
        // reconstruct proper OpenAI-format tool_calls and tool result messages.
        // Prompt mode: use XML-based text format as before.
        history.push(ChatMessage::assistant(assistant_history_content));
        if native_tool_calls.is_empty() {
            history.push(ChatMessage::user(format!("[Tool results]\n{tool_results}")));
        } else {
            for (native_call, result) in native_tool_calls.iter().zip(individual_results.iter()) {
                let tool_msg = serde_json::json!({
                    "tool_call_id": native_call.id,
                    "content": result,
                });
                history.push(ChatMessage::tool(tool_msg.to_string()));
            }
        }

        // Circuit breaker tripped this iteration: return the root-cause summary
        // as the agent's result instead of looping to `max_iterations`. The
        // tool results are already in `history` above, so the caller still has
        // full context if it wants it.
        if let Some(reason) = halt_reason.take() {
            // Mirror the normal-completion path: emit TurnCompleted before the
            // early return, otherwise progress consumers stay "in-flight"
            // indefinitely when the circuit breaker trips.
            if let Some(ref sink) = on_progress {
                if let Err(e) = sink
                    .send(AgentProgress::TurnCompleted {
                        iterations: (iteration + 1) as u32,
                    })
                    .await
                {
                    log::warn!("[agent_loop] progress sink closed at TurnCompleted: {e}");
                }
            }
            return Ok(reason);
        }
    }

    // Return the typed `AgentError::MaxIterationsExceeded` variant (boxed
    // through `anyhow::Error`) so downstream wrappers — notably
    // `Agent::run_single` in `harness/session/runtime.rs` — can downcast and
    // suppress Sentry emission for this deterministic agent-state outcome
    // (OPENHUMAN-TAURI-99 / -98). The `Display` text is preserved verbatim so
    // any caller that already inspects the string (UI chat surface, tests)
    // continues to work.
    Err(anyhow::Error::new(
        crate::openhuman::agent::error::AgentError::MaxIterationsExceeded { max: max_iterations },
    ))
}
