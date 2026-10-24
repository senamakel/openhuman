            match event {
                AgentProgress::TurnStarted => {
                    // Turn is live — start emitting liveness beats (issue #4270).
                    turn_active = true;
                    ledger_upsert_agent_run(
                        &config,
                        AgentRunUpsert {
                            id: request_id.clone(),
                            kind: AgentRunKind::BackgroundAgent,
                            parent_run_id: None,
                            parent_thread_id: Some(thread_id.clone()),
                            agent_id: Some("orchestrator".to_string()),
                            status: AgentRunStatus::Running,
                            prompt_ref: Some(format!("thread:{thread_id}:request:{request_id}")),
                            worker_thread_id: None,
                            checkpoint_path: None,
                            checkpoint: None,
                            summary: None,
                            error: None,
                            metadata: json!({
                                "clientId": client_id,
                                "source": "web_channel",
                                "schemaVersion": 1
                            }),
                            started_at: None,
                            completed_at: None,
                        },
                    );
                    ledger_append_event(
                        &config,
                        RunEventAppend {
                            run_id: request_id.clone(),
                            event_type: "turn_started".to_string(),
                            payload: json!({ "threadId": thread_id, "clientId": client_id }),
                        },
                    );
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "inference_start".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::IterationStarted {
                    iteration,
                    max_iterations,
                } => {
                    round = iteration;
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "iteration_start".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            message: Some(format!("Iteration {iteration}/{max_iterations}")),
                            round: Some(iteration),
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::ContextCompacting => {
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "context_compacting".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            round: Some(round),
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::ToolCallStarted {
                    call_id,
                    tool_name,
                    arguments,
                    iteration,
                    display_label,
                    display_detail,
                } => {
                    timing.tool_call(&tool_name, iteration, &request_id);
                    // The parent's leading narration for this round is complete
                    // once it calls a tool — flush it as an interim bubble so it
                    // persists interleaved with the tool activity.
                    flush_interim_narration(
                        &dispatch_ctx,
                        &mut pending_narration,
                        iteration,
                        &client_id,
                        &thread_id,
                        &request_id,
                        &mut emit_seq,
                    );
                    parent_tool_count += 1;
                    ledger_append_event(
                        &config,
                        RunEventAppend {
                            run_id: request_id.clone(),
                            event_type: "tool_call_started".to_string(),
                            payload: json!({
                                "callId": call_id,
                                "toolName": tool_name,
                                "iteration": iteration
                            }),
                        },
                    );
                    ledger_upsert_telemetry(
                        &config,
                        RunTelemetryUpsert {
                            run_id: request_id.clone(),
                            tool_count: Some(parent_tool_count),
                            ..Default::default()
                        },
                    );
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "tool_call".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            tool_name: Some(tool_name),
                            skill_id: Some("web_channel".to_string()),
                            args: cap_wire_args(Some(arguments)),
                            round: Some(iteration),
                            tool_call_id: Some(call_id),
                            tool_display_label: display_label,
                            tool_display_detail: display_detail,
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::ToolCallCompleted {
                    call_id,
                    tool_name,
                    success,
                    output_chars,
                    output,
                    arguments,
                    elapsed_ms,
                    iteration,
                    failure,
                    display_label,
                    display_detail,
                    structured,
                } => {
                    // Serialize the classified failure (if any) for the UI + ledger.
                    let failure_json = failure.as_ref().and_then(|f| serde_json::to_value(f).ok());
                    ledger_append_event(
                        &config,
                        RunEventAppend {
                            run_id: request_id.clone(),
                            event_type: "tool_call_completed".to_string(),
                            payload: json!({
                                "callId": call_id,
                                "toolName": tool_name,
                                "success": success,
                                "outputChars": output_chars,
                                "elapsedMs": elapsed_ms,
                                "iteration": iteration,
                                "failure": failure_json,
                            }),
                        },
                    );
                    log::debug!(
                        "[web_channel][bridge] tool_result round={} tool={} call_id={} \
                         success={} elapsed_ms={} has_structured={} request_id={}",
                        iteration,
                        tool_name,
                        call_id,
                        success,
                        elapsed_ms,
                        structured.is_some(),
                        request_id
                    );
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "tool_result".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            tool_name: Some(tool_name),
                            skill_id: Some("web_channel".to_string()),
                            // Forward the real tool result (size-capped) so the UI
                            // can render tool output — mirrors the subagent
                            // `subagent_tool_result` path. Frontends that only
                            // need size/timing read the ledger telemetry instead.
                            output: Some(cap_wire_output(output)),
                            // The call arguments the harness captured at
                            // completion (`ToolCallStarted.arguments` is
                            // always `Null` on this path). Omitted when the
                            // harness ran with payload capture off.
                            args: cap_wire_args(arguments),
                            success: Some(success),
                            round: Some(iteration),
                            tool_call_id: Some(call_id),
                            failure: failure_json,
                            elapsed_ms: Some(elapsed_ms),
                            structured,
                            // Recomputed from the real arguments (unlike the
                            // started event's args-free computation), so a
                            // completed row can pick up a detail that only
                            // became knowable once the arguments existed.
                            tool_display_label: display_label,
                            tool_display_detail: display_detail,
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::SubagentSpawned {
                    agent_id,
                    task_id,
                    mode,
                    dedicated_thread,
                    prompt_chars,
                    worker_thread_id,
                    display_name,
                    parent_call_id,
                    ..
                } => {
                    subagent_events::on_subagent_spawned(
                        &bridge_ctx,
                        &mut emit_seq,
                        &mut subagent_parent_call_ids,
                        round,
                        agent_id,
                        task_id,
                        mode,
                        dedicated_thread,
                        prompt_chars,
                        worker_thread_id,
                        display_name,
                        parent_call_id,
                    );
                }
                AgentProgress::SubagentCompleted {
                    agent_id,
                    task_id,
                    elapsed_ms,
                    iterations,
                    output_chars,
                    output,
                    usage,
                    worktree_path,
                    changed_files,
                    dirty_status,
                    ..
                } => {
                    subagent_events::on_subagent_completed(
                        &bridge_ctx,
                        &mut emit_seq,
                        &mut subagent_parent_call_ids,
                        &child_tool_counts,
                        round,
                        agent_id,
                        task_id,
                        elapsed_ms,
                        iterations,
                        output_chars,
                        output,
                        usage,
                        worktree_path,
                        changed_files,
                        dirty_status,
                    );
                }
                AgentProgress::SubagentFailed {
                    agent_id,
                    task_id,
                    error,
                } => {
                    subagent_events::on_subagent_failed(
                        &bridge_ctx,
                        &mut emit_seq,
                        &mut subagent_parent_call_ids,
                        &child_tool_counts,
                        round,
                        agent_id,
                        task_id,
                        error,
                    );
                }
                AgentProgress::SubagentAwaitingUser {
                    agent_id,
                    task_id,
                    question,
                    worker_thread_id,
                    checkpoint_path,
                } => {
                    subagent_events::on_subagent_awaiting_user(
                        &bridge_ctx,
                        &mut emit_seq,
                        &subagent_parent_call_ids,
                        round,
                        agent_id,
                        task_id,
                        question,
                        worker_thread_id,
                        checkpoint_path,
                    );
                }
                AgentProgress::SubagentIterationStarted {
                    agent_id,
                    task_id,
                    iteration,
                    max_iterations,
                    extended_policy,
                } => {
                    subagent_events::on_subagent_iteration_started(
                        &bridge_ctx,
                        &mut emit_seq,
                        round,
                        agent_id,
                        task_id,
                        iteration,
                        max_iterations,
                        extended_policy,
                    );
                }
                AgentProgress::SubagentToolCallStarted {
                    agent_id,
                    task_id,
                    call_id,
                    tool_name,
                    arguments,
                    iteration,
                    display_label,
                    display_detail,
                } => {
                    subagent_events::on_subagent_tool_call_started(
                        &bridge_ctx,
                        &mut emit_seq,
                        &mut child_tool_counts,
                        round,
                        agent_id,
                        task_id,
                        call_id,
                        tool_name,
                        arguments,
                        iteration,
                        display_label,
                        display_detail,
                    );
                }
                AgentProgress::SubagentToolCallCompleted {
                    agent_id,
                    task_id,
                    call_id,
                    tool_name,
                    success,
                    output_chars,
                    output,
                    arguments,
                    elapsed_ms,
                    iteration,
                    failure,
                    display_label,
                    display_detail,
                    structured,
                } => {
                    subagent_events::on_subagent_tool_call_completed(
                        &bridge_ctx,
                        &mut emit_seq,
                        round,
                        agent_id,
                        task_id,
                        call_id,
                        tool_name,
                        success,
                        output_chars,
                        output,
                        arguments,
                        elapsed_ms,
                        iteration,
                        failure,
                        display_label,
                        display_detail,
                        structured,
                    );
                }
                AgentProgress::SubagentTextDelta {
                    agent_id,
                    task_id,
                    delta,
                    iteration,
                } => {
                    subagent_events::on_subagent_text_delta(
                        &bridge_ctx,
                        &mut emit_seq,
                        round,
                        agent_id,
                        task_id,
                        delta,
                        iteration,
                    );
                }
                AgentProgress::SubagentThinkingDelta {
                    agent_id,
                    task_id,
                    delta,
                    iteration,
                } => {
                    subagent_events::on_subagent_thinking_delta(
                        &bridge_ctx,
                        &mut emit_seq,
                        round,
                        agent_id,
                        task_id,
                        delta,
                        iteration,
                    );
                }
                AgentProgress::TextDelta { delta, iteration } => {
                    timing.text_delta(&delta, iteration, &request_id);
                    // Buffer the round's narration so it can be flushed as an
                    // interim bubble if a tool call closes this round.
                    pending_narration.push_str(&delta);
                    if !metadata.hold_text_stream {
                        text_events::publish_text_delta(
                            &dispatch_ctx,
                            &mut emit_seq,
                            &client_id,
                            &thread_id,
                            &request_id,
                            iteration,
                            delta,
                        );
                    }
                }
                AgentProgress::ThinkingDelta { delta, iteration } => {
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "thinking_delta".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            round: Some(iteration),
                            delta: Some(delta),
                            delta_kind: Some("thinking".to_string()),
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::ToolCallArgsDelta {
                    call_id,
                    tool_name,
                    delta,
                    iteration,
                } => {
                    publish_seq_stamped(
                        &dispatch_ctx,
                        &mut emit_seq,
                        WebChannelEvent {
                            event: "tool_args_delta".to_string(),
                            client_id: client_id.clone(),
                            thread_id: thread_id.clone(),
                            request_id: request_id.clone(),
                            tool_name: if tool_name.is_empty() {
                                None
                            } else {
                                Some(tool_name)
                            },
                            skill_id: Some("web_channel".to_string()),
                            round: Some(iteration),
                            delta: Some(delta),
                            delta_kind: Some("tool_args".to_string()),
                            tool_call_id: Some(call_id),
                            ..Default::default()
                        },
                    );
                }
                AgentProgress::TurnCompleted { iterations, .. } => {
                    parent_completed = true;
                    timing.done(iterations, MIN_INTERIM_NARRATION_CHARS, &request_id);
                    if let Ok(mut guard) = timing_snapshot_for_task.lock() {
                        *guard = Some(timing.snapshot());
                    }
                    // Turn is done — stop liveness beats (issue #4270). The FE
                    // clears its silence timer on `chat_done`/`chat_error`; this
                    // also prevents a stray beat racing the channel close.
                    turn_active = false;
                    let completed_at = chrono::Utc::now();
                    ledger_upsert_agent_run(
                        &config,
                        AgentRunUpsert {
                            id: request_id.clone(),
                            kind: AgentRunKind::BackgroundAgent,
                            parent_run_id: None,
                            parent_thread_id: Some(thread_id.clone()),
                            agent_id: Some("orchestrator".to_string()),
                            status: AgentRunStatus::Completed,
                            prompt_ref: Some(format!("thread:{thread_id}:request:{request_id}")),
                            worker_thread_id: None,
                            checkpoint_path: None,
                            checkpoint: None,
                            summary: Some(format!("Completed in {iterations} iteration(s)")),
                            error: None,
                            metadata: json!({}),
                            started_at: None,
                            completed_at: Some(completed_at),
                        },
                    );
                    ledger_append_event(
                        &config,
                        RunEventAppend {
                            run_id: request_id.clone(),
                            event_type: "turn_completed".to_string(),
                            payload: json!({ "iterations": iterations }),
                        },
                    );
                    log::debug!(
                        "[web_channel] turn completed after {iterations} iteration(s) \
                         client_id={client_id} thread_id={thread_id} request_id={request_id} \
                         speak_reply={:?} source={:?} session_id={:?}",
                        metadata.speak_reply,
                        metadata.source,
                        metadata.session_id,
                    );
                    // Every event the parent queued before `TurnCompleted` has
                    // now been forwarded, in order: release a caller waiting
                    // to publish the terminal `chat_done`.
                    let _ = drained_tx.send(true);
                    log::debug!(
                        "[web_channel][bridge] drained parent events_seen={events_seen} request_id={request_id}"
                    );
                }
                AgentProgress::TurnCostUpdated {
                    model,
                    iteration,
                    input_tokens,
                    output_tokens,
                    cached_input_tokens,
                    total_usd,
                } => {
                    ledger_upsert_telemetry(
                        &config,
                        RunTelemetryUpsert {
                            run_id: request_id.clone(),
                            input_tokens: Some(input_tokens),
                            output_tokens: Some(output_tokens),
                            cached_input_tokens: Some(cached_input_tokens),
                            cost_usd: Some(total_usd),
                            model: Some(model.clone()),
                            ..Default::default()
                        },
                    );
                    log::debug!(
                        "[web_channel] turn cost update model={model} iter={iteration} \
                         in={input_tokens} out={output_tokens} cached_in={cached_input_tokens} \
                         total_usd={total_usd:.4} client_id={client_id} thread_id={thread_id}"
                    );

                    // Live cost readout: throttled so a fast multi-round turn
                    // doesn't flood the socket with one `turn_cost` per model
                    // call. `TurnCostUpdated` is the parent's cumulative
                    // rollup only — it carries no per-sub-agent breakdown, so
                    // `subagents` stays empty here; the final `chat_done.usage`
                    // (built from `LastTurnUsage` at delivery) is still where
                    // sub-agent attribution shows up.
                    if turn_cost_throttle.should_emit() {
                        publish_seq_stamped(
                            &dispatch_ctx,
                            &mut emit_seq,
                            WebChannelEvent {
                                event: "turn_cost".to_string(),
                                client_id: client_id.clone(),
                                thread_id: thread_id.clone(),
                                request_id: request_id.clone(),
                                round: Some(iteration),
                                usage: Some(crate::web_chat::TurnUsagePayload {
                                    input_tokens,
                                    output_tokens,
                                    cached_input_tokens,
                                    // A live rollup of the known spend so far;
                                    // the turn's final figure (or `null`)
                                    // arrives on `chat_done`.
                                    cost_usd: Some(total_usd),
                                    cost_source: tinyagents_harness::cost::CostSource::Charged,
                                    context_window: 0,
                                    context_tokens: 0,
                                    subagents: Vec::new(),
                                }),
                                ..Default::default()
                            },
                        );
                    }
                }
                AgentProgress::ToolCallProgress { call_id, message } => {
                    let ids = (client_id.as_str(), thread_id.as_str(), request_id.as_str());
                    tool_progress::publish(&dispatch_ctx, &mut emit_seq, ids, call_id, message);
                }
                AgentProgress::TurnContent { .. } => {
                    // Prompt/reply content is attached to the trace span by the
                    // span collector above; the ledger/telemetry bridge ignores it.
                }
                AgentProgress::ModelCallCompleted {
                    model,
                    iteration,
                    input_tokens,
                    output_tokens,
                    cost_usd,
                    ..
                } => {
                    // Per-call usage is consumed by the span collector above
                    // (per-call Langfuse generation); the socket/ledger surfaces
                    // stay on the cumulative TurnCostUpdated rollup.
                    log::debug!(
                        "[web_channel][bridge] model_call_completed model={model} iter={iteration} \
                         in={input_tokens} out={output_tokens} cost_usd={cost_usd:?} request_id={request_id}"
                    );
                }
            }
