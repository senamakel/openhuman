use super::*;
use crate::agent::orchestration::background_completions::CompletionTarget;
use crate::agent::orchestration::subagent_sessions::SubagentSessionStore;
use crate::agent::subagent_host::{SubagentRunOutcome, SubagentRunStatus};
use crate::core::runtime::{CoreContext, DispatchCtx};
use crate::progress::AgentProgress;
use crate::security::turn_origin::AgentTurnOrigin;
use std::path::PathBuf;
use std::sync::Arc;

pub(super) fn make_completion(
    completion_core: Option<Arc<CoreContext>>,
    completion_origin: Option<AgentTurnOrigin>,
    progress_sink: Option<tokio::sync::mpsc::Sender<AgentProgress>>,
    background_workspace_dir: PathBuf,
    background_dispatch: DispatchCtx,
    background_agent_id: String,
    background_task_id: String,
    background_parent_session: String,
    completion_target: CompletionTarget,
    completion_worker_thread_id: Option<String>,
    background_store: SubagentSessionStore,
    background_subagent_session_id: String,
    background_parent_thread_id: Option<String>,
) -> impl Fn(
    String,
    Result<SubagentRunOutcome, String>,
    bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
       + Send
       + Sync
       + 'static {
    move |_target: String,
          result: Result<SubagentRunOutcome, String>,
          emit_lifecycle_effects: bool|
          -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        let core = completion_core.clone();
        let origin = completion_origin.clone();
        let progress_sink = progress_sink.clone();
        let background_workspace_dir = background_workspace_dir.clone();
        let background_dispatch = background_dispatch.clone();
        let background_agent_id = background_agent_id.clone();
        let background_task_id = background_task_id.clone();
        let background_parent_session = background_parent_session.clone();
        let completion_target = completion_target.clone();
        let background_worker_thread_id = completion_worker_thread_id.clone();
        let background_store = background_store.clone();
        let background_subagent_session_id = background_subagent_session_id.clone();
        let background_parent_thread_id = background_parent_thread_id.clone();
        Box::pin(async move {
            let finish = async move {
                let mut abort_report =
                    AbortReport::arm(progress_sink, &background_agent_id, &background_task_id);
                let result = result.map_err(anyhow::Error::msg);
                match result {
                    Ok(outcome) => {
                        match outcome.status {
                            SubagentRunStatus::Completed => {
                                if let Err(err) = subagent_sessions::mark_finished(
                                    &background_store,
                                    &background_subagent_session_id,
                                    &outcome.task_id,
                                    &outcome.status,
                                    outcome.final_history.clone(),
                                ) {
                                    log::warn!(
                                        "[subagent_reuse] mark_completed failed subagent_session_id={} task_id={} agent_id={} error={}",
                                        background_subagent_session_id,
                                        outcome.task_id,
                                        outcome.agent_id,
                                        err
                                    );
                                }
                                // A workflow proposal from the child's tool history is
                                // durable state, not prose: persist it into the parent chat
                                // thread (survives reload / reconnect) and carry the full
                                // payload in the delivery notice for the follow-up turn.
                                if emit_lifecycle_effects {
                                    let delivery_summary = attach_workflow_proposal(
                                        &background_dispatch,
                                        &background_workspace_dir,
                                        background_parent_thread_id.as_deref(),
                                        &outcome.task_id,
                                        &outcome.agent_id,
                                        &outcome.final_history,
                                        outcome.output.clone(),
                                    );
                                    completion_target
                                        .completed(
                                            &outcome.task_id,
                                            &outcome.agent_id,
                                            delivery_summary,
                                        )
                                        .await;
                                    crate::agent::orchestration::subagent_events::publish_subagent_completed(
                            background_parent_session,
                            outcome.task_id.clone(),
                            outcome.agent_id.clone(),
                            outcome.elapsed.as_millis() as u64,
                            outcome.output.chars().count(),
                            outcome.iterations,
                        );
                                    abort_report
                                        .deliver(AgentProgress::SubagentCompleted {
                                            agent_id: outcome.agent_id,
                                            task_id: outcome.task_id,
                                            elapsed_ms: outcome.elapsed.as_millis() as u64,
                                            iterations: outcome.iterations as u32,
                                            output_chars: outcome.output.chars().count(),
                                            output: outcome.output.clone(),
                                            // Detached by construction: this tool
                                            // takes `detached_child()`, so this child's
                                            // spend never reached the parent turn's
                                            // ledger or `chat_done`. See the field docs.
                                            usage: Some(outcome.usage),
                                            worktree_path: None,
                                            changed_files: Vec::new(),
                                            dirty_status: None,
                                            stop: None,
                                        })
                                        .await;
                                }
                            }
                            SubagentRunStatus::Incomplete { ref reason, .. } => {
                                // Async sub-agent stopped short (stuck halt / iteration
                                // cap). Mark the session finished and deliver the PARTIAL
                                // progress back to the parent, framed so it is not
                                // mistaken for a completed result (#4096).
                                if let Err(err) = subagent_sessions::mark_finished(
                                    &background_store,
                                    &background_subagent_session_id,
                                    &outcome.task_id,
                                    &outcome.status,
                                    outcome.final_history.clone(),
                                ) {
                                    log::warn!(
                                        "[subagent_reuse] mark_incomplete failed subagent_session_id={} task_id={} agent_id={} error={}",
                                        background_subagent_session_id,
                                        outcome.task_id,
                                        outcome.agent_id,
                                        err
                                    );
                                }
                                let framed = format!(
                                    "[SUBAGENT_INCOMPLETE] the sub-agent {reason} and did not finish. \
                             Partial progress:\n{}",
                                    outcome.output
                                );
                                // An incomplete run may still have produced a full
                                // proposal before stalling — preserve it durably too.
                                if emit_lifecycle_effects {
                                    let framed = attach_workflow_proposal(
                                        &background_dispatch,
                                        &background_workspace_dir,
                                        background_parent_thread_id.as_deref(),
                                        &outcome.task_id,
                                        &outcome.agent_id,
                                        &outcome.final_history,
                                        framed,
                                    );
                                    completion_target
                                        .completed(&outcome.task_id, &outcome.agent_id, framed)
                                        .await;
                                    crate::agent::orchestration::subagent_events::publish_subagent_completed(
                            background_parent_session,
                            outcome.task_id.clone(),
                            outcome.agent_id.clone(),
                            outcome.elapsed.as_millis() as u64,
                            outcome.output.chars().count(),
                            outcome.iterations,
                        );
                                    abort_report
                                        .deliver(AgentProgress::SubagentCompleted {
                                            agent_id: outcome.agent_id,
                                            task_id: outcome.task_id,
                                            elapsed_ms: outcome.elapsed.as_millis() as u64,
                                            iterations: outcome.iterations as u32,
                                            output_chars: outcome.output.chars().count(),
                                            output: outcome.output.clone(),
                                            // Detached by construction: this tool
                                            // takes `detached_child()`, so this child's
                                            // spend never reached the parent turn's
                                            // ledger or `chat_done`. See the field docs.
                                            usage: Some(outcome.usage),
                                            worktree_path: None,
                                            changed_files: Vec::new(),
                                            dirty_status: None,
                                            stop: incomplete_stop(
                                                reason,
                                                outcome.failure_cause.clone(),
                                            ),
                                        })
                                        .await;
                                }
                            }
                            SubagentRunStatus::Cancelled => {
                                let reason = match background_store.load() {
                                    Ok(rows) => rows
                                        .into_iter()
                                        .find(|row| {
                                            row.subagent_session_id
                                                == background_subagent_session_id
                                                && row.current_task_id.as_deref()
                                                    == Some(outcome.task_id.as_str())
                                        })
                                        .and_then(|row| row.latest_error),
                                    Err(error) => {
                                        log::warn!(
                                            "[subagent_reuse] cancellation reason lookup failed subagent_session_id={} task_id={} error={error}",
                                            background_subagent_session_id,
                                            outcome.task_id
                                        );
                                        None
                                    }
                                };
                                let error = match reason
                                    .as_deref()
                                    .map(str::trim)
                                    .filter(|reason| !reason.is_empty())
                                {
                                    Some(reason) => {
                                        format!("async sub-agent was cancelled. Reason: {reason}")
                                    }
                                    None => "async sub-agent was cancelled".to_string(),
                                };
                                if let Err(err) = subagent_sessions::mark_finished(
                                    &background_store,
                                    &background_subagent_session_id,
                                    &outcome.task_id,
                                    &outcome.status,
                                    outcome.final_history.clone(),
                                ) {
                                    log::warn!(
                                        "[subagent_reuse] mark_cancelled failed subagent_session_id={} task_id={} agent_id={} error={}",
                                        background_subagent_session_id,
                                        outcome.task_id,
                                        outcome.agent_id,
                                        err
                                    );
                                }
                                if emit_lifecycle_effects {
                                    completion_target
                                        .failed(&outcome.task_id, &outcome.agent_id, &error)
                                        .await;
                                    crate::agent::orchestration::subagent_events::publish_subagent_failed(
                                        background_parent_session,
                                        outcome.task_id.clone(),
                                        outcome.agent_id.clone(),
                                        error.clone(),
                                    );
                                    abort_report
                                        .deliver(AgentProgress::SubagentFailed {
                                            agent_id: outcome.agent_id,
                                            task_id: outcome.task_id,
                                            error,
                                        })
                                        .await;
                                }
                            }
                            SubagentRunStatus::AwaitingUser {
                                ref question,
                                ref checkpoint,
                                ..
                            } => {
                                if let Err(err) = subagent_sessions::mark_finished(
                                    &background_store,
                                    &background_subagent_session_id,
                                    &outcome.task_id,
                                    &outcome.status,
                                    outcome.final_history.clone(),
                                ) {
                                    log::warn!(
                                        "[subagent_reuse] mark_awaiting_user failed subagent_session_id={} task_id={} agent_id={} error={}",
                                        background_subagent_session_id,
                                        outcome.task_id,
                                        outcome.agent_id,
                                        err
                                    );
                                }
                                // #4896: a detached child that pauses for input won't continue
                                // on its own — queue a framed notice so the parent chat learns it
                                // needs input, not "Accepted". Same idle-gated path as a success.
                                if emit_lifecycle_effects {
                                    completion_target
                                        .awaiting_input(
                                            &outcome.task_id,
                                            &outcome.agent_id,
                                            question,
                                            checkpoint.is_some(),
                                        )
                                        .await;
                                    crate::agent::orchestration::subagent_events::publish_subagent_awaiting_user(
                                        background_parent_session,
                                        outcome.task_id.clone(),
                                        outcome.agent_id.clone(),
                                        question.clone(),
                                    );
                                    abort_report
                                        .deliver(AgentProgress::SubagentAwaitingUser {
                                            agent_id: outcome.agent_id,
                                            task_id: outcome.task_id,
                                            question: question.clone(),
                                            worker_thread_id: background_worker_thread_id.clone(),
                                            checkpoint_path: checkpoint
                                                .as_ref()
                                                .map(|path| path.to_string_lossy().to_string()),
                                        })
                                        .await;
                                }
                            }
                        }
                    }
                    Err(err) => {
                        let error = err.to_string();
                        if let Err(store_err) = subagent_sessions::mark_failed(
                            &background_store,
                            &background_subagent_session_id,
                            &background_task_id,
                            error.clone(),
                        ) {
                            log::warn!(
                                "[subagent_reuse] mark_failed failed subagent_session_id={} task_id={} agent_id={} error={}",
                                background_subagent_session_id,
                                background_task_id,
                                background_agent_id,
                                store_err
                            );
                        }
                        // #4896: a detached child that errors previously only
                        // published an event — nothing reached chat, so the parent
                        // turn finalized on "Accepted" and the failure was lost.
                        // Queue a framed failure notice so background_delivery
                        // surfaces it as a follow-up chat turn.
                        if emit_lifecycle_effects {
                            completion_target
                                .failed(&background_task_id, &background_agent_id, &error)
                                .await;
                            crate::agent::orchestration::subagent_events::publish_subagent_failed(
                                background_parent_session,
                                background_task_id.clone(),
                                background_agent_id.clone(),
                                error.clone(),
                            );
                            abort_report
                                .deliver(AgentProgress::SubagentFailed {
                                    agent_id: background_agent_id,
                                    task_id: background_task_id,
                                    error,
                                })
                                .await;
                        }
                    }
                }
                abort_report.disarm();
            };
            match core {
                Some(core) => {
                    crate::core::runtime::CoreContext::scope_with_turn_origin(
                        core,
                        origin,
                        Box::pin(finish),
                    )
                    .await
                }
                None => finish.await,
            }
        })
    }
}
