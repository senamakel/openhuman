//! The `context_scout` engine: running the sub-agent inline (blocking),
//! extracting its `[context_bundle]` envelope, classifying/logging failures,
//! and bootstrapping a thread goal from the scout's proposal.

use crate::agent::harness::definition::AgentDefinitionRegistry;
use crate::agent::harness::fork_context::{current_parent, AgentContextPreparedSource};
use crate::agent::harness::subagent_runner::{
    run_subagent, SubagentRunError, SubagentRunOptions, SubagentRunStatus,
};
use crate::agent::progress::AgentProgress;
use crate::agent::tinyagents::thread_context::current_thread_id;
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::tools::traits::ToolResult;
use tinyagents_harness::workspace::WorkspaceDescriptor;

use super::tool::AgentPrepareContextTool;

/// The sub-agent archetype this tool drives.
const SCOUT_AGENT_ID: &str = "context_scout";

/// Extract exactly one `[context_bundle] … [/context_bundle]` envelope from
/// `output`, tolerating surrounding prose, and return only the envelope
/// substring (tags included). Returns `None` when no usable envelope is
/// present.
///
/// The `context_scout` contract is "emit the single envelope and nothing
/// outside it", but the fast model may wrap the envelope in a preamble (`Sure,
/// here's what I found:\n[context_bundle]…`) or a closing line
/// (`…[/context_bundle]\nHope that helps!`). Requiring the *whole* trimmed
/// output to be the envelope would discard an otherwise-good bundle.
///
/// Pulling the envelope substring out of the surrounding text keeps the safety
/// property intact: callers receive only the bracketed envelope, never the
/// model's free-form prose. We still reject genuinely unusable output —
/// absent, unterminated/reversed, or duplicated (where we can't tell which
/// envelope is authoritative) — by returning `None`.
pub(super) fn extract_context_bundle(output: &str) -> Option<String> {
    const OPEN: &str = "[context_bundle]";
    const CLOSE: &str = "[/context_bundle]";
    // Exactly one open + one close tag. Duplicates are a contract violation we
    // reject rather than guess which envelope is authoritative.
    if output.matches(OPEN).count() != 1 || output.matches(CLOSE).count() != 1 {
        return None;
    }
    let open_idx = output.find(OPEN)?;
    let close_idx = output.find(CLOSE)?;
    // Tags must appear in order (open before close) and not overlap.
    if close_idx < open_idx + OPEN.len() {
        return None;
    }
    let end = close_idx + CLOSE.len();
    Some(output[open_idx..end].trim().to_string())
}

pub(super) fn already_prepared_context_bundle(sources: &[AgentContextPreparedSource]) -> String {
    let source_names = if sources.is_empty() {
        "the OpenHuman harness".to_string()
    } else {
        sources
            .iter()
            .map(|source| source.source.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let has_enough_context = if sources
        .iter()
        .any(|source| source.has_enough_context == Some(false))
    {
        false
    } else {
        sources
            .iter()
            .any(|source| source.has_enough_context == Some(true))
    };
    let sufficiency_note = if has_enough_context {
        "The earlier prepared context reported enough context."
    } else {
        "This no-op result does not assert that enough context is available; \
         inspect the earlier prepared-context blocks for sufficiency and recommended follow-up tools."
    };
    format!(
        "[context_bundle]\nhas_enough_context: {has_enough_context}\nproposed_goal: none\n\
         summary: Agent context has already been prepared once for this turn by {source_names}. \
         {sufficiency_note} Use the existing prepared-context blocks in the current user message; do not run \
         another context_scout pass.\nrecommended_tool_calls:\n[/context_bundle]"
    )
}

/// Run the `context_scout` sub-agent inline (blocking) for `question` and
/// return its bounded `[context_bundle]` envelope as a [`ToolResult`].
///
/// This is the engine behind [`AgentPrepareContextTool`], invoked autonomously
/// by the LLM when it decides to scout context mid-turn.
///
/// Must be called from within an active agent turn (i.e. with the
/// [`crate::agent::harness::fork_context::PARENT_CONTEXT`]
/// task-local installed) — it reads the parent's visible tool catalogue
/// and runs the scout against the parent's provider. Outside a turn the
/// `run_subagent` call surfaces a no-parent error as a [`ToolResult::error`].
pub async fn run_context_scout(question: &str, focus: Option<&str>) -> anyhow::Result<ToolResult> {
    let tool_catalog = AgentPrepareContextTool::render_parent_tool_catalog();
    run_context_scout_with_catalog(question, focus, &tool_catalog).await
}

/// Same as [`run_context_scout`] but with an **explicitly-supplied** tool
/// catalogue — for callers *outside* an agent turn that can't auto-derive the
/// parent's visible tool set from `current_parent()` (e.g. the subconscious
/// engine's structured tick).
///
/// The caller passes the catalogue of tools the eventual decision agent can
/// actually call (one `- name: description` per line), so the bundle's
/// `recommended_tool_calls` stay grounded in callable tools.
///
/// **A parent execution context is still required.** Like [`run_context_scout`]
/// this spawns `context_scout` via `run_subagent`, which resolves its provider /
/// tools / model from the `PARENT_CONTEXT` task-local and returns
/// `NoParentContext` when it is unset. A background surface with no enclosing
/// turn MUST establish a root parent first — call this *inside*
/// [`with_root_parent`](crate::agent::orchestration::parent_context::with_root_parent).
/// Skipping that is exactly the TAURI-RUST-HMW failure (#4337): every spawn
/// died with `NoParentContext` and the tick ran un-grounded. Only the
/// progress / subagent-lifecycle telemetry degrades gracefully without a
/// parent — `parent_session` falls back to `standalone` and the absent progress
/// sink no-ops — but the spawn itself does not.
pub async fn run_context_scout_with_catalog(
    question: &str,
    focus: Option<&str>,
    tool_catalog: &str,
) -> anyhow::Result<ToolResult> {
    run_context_scout_with_catalog_and_workspace(question, focus, tool_catalog, None).await
}

/// The text [`log_scout_failure`] classifies: a failed run flattened into one
/// string, `source` chain included.
///
/// The billing classifiers key on the **provider wire body**
/// (`… "errorCode":"USER_INSUFFICIENT_CREDITS"`), which arrives inside
/// [`SubagentRunError::Provider`]'s `anyhow` error — often as a *cause* under a
/// context layer, so `to_string()` alone renders only `"provider call failed:
/// <top-level context>"` and would miss the very failure this exists to demote.
/// Walk `anyhow`'s chain for that variant; every other variant is a flat
/// `thiserror` message already. Used for classification only — the
/// user-visible `message` stays `err.to_string()`, unchanged.
pub(super) fn scout_failure_signal(err: &SubagentRunError) -> String {
    let SubagentRunError::Provider(inner) = err else {
        return err.to_string();
    };
    let mut chain = inner.to_string();
    for cause in inner.chain().skip(1) {
        chain.push_str(": ");
        chain.push_str(&cause.to_string());
    }
    chain
}

/// Is a failed `context_scout` run just the user being out of credits?
///
/// Two billing shapes reach here, both **user-state, not defects**:
/// * the managed OpenHuman backend's budget-exhausted 400
///   (`{"error":"Insufficient budget","errorCode":"USER_INSUFFICIENT_CREDITS"}`),
///   matched by [`crate::inference::provider::is_budget_exhausted_message`];
/// * a BYO provider's insufficient-credits 402, matched by
///   [`crate::core::observability::is_insufficient_credits_message`].
///
/// Both delegate to the crate's single-source classifiers so the phrase sets
/// can't drift from the cron halt / `before_send` nets that share them.
pub(super) fn is_expected_billing_failure(message: &str) -> bool {
    crate::inference::provider::is_budget_exhausted_message(message)
        || crate::core::observability::is_insufficient_credits_message(message)
}

/// Log a failed `context_scout` run at the severity its cause deserves.
///
/// The scout is a **background/best-effort** pass: every caller already
/// degrades to the un-augmented message on failure. When the cause is the user
/// being out of credits (`USER_INSUFFICIENT_CREDITS`) that is a preventable
/// billing state OpenHuman has no lever over, yet `tracing::error!` maps to
/// `EventFilter::Event` in `core::logging::sentry_tracing_layer` — so every
/// tick of every out-of-credits user paged Sentry (TAURI-RUST-HMW: 8314 events
/// / 13 users, #5308). The existing `is_budget_event` `before_send` net cannot
/// catch it: that filter is tag-gated (`failure=non_2xx` + `status=400`) and
/// keys on the event *message*, but this event's message is the static
/// `"context_scout run failed"` with the wire body only in a breadcrumb.
///
/// So demote at the emit site: `warn!` maps to `EventFilter::Breadcrumb`, which
/// keeps the local log line (and the Sentry breadcrumb trail for any *real*
/// error that follows) without raising an issue. Every other cause still
/// `error!`s and keeps paging.
pub(super) fn log_scout_failure(error_kind: &str, message: &str) {
    if is_expected_billing_failure(message) {
        // Metadata-only — never log the raw provider body (see CLAUDE.md).
        tracing::warn!(
            target: "agent_prepare_context",
            error_kind = %error_kind,
            "[agent_prepare_context] context_scout run skipped — account is out of credits (expected user-state, not reported)"
        );
        return;
    }
    tracing::error!(
        target: "agent_prepare_context",
        error_kind = %error_kind,
        "[agent_prepare_context] context_scout run failed"
    );
}

pub(super) async fn run_context_scout_with_catalog_and_workspace(
    question: &str,
    focus: Option<&str>,
    tool_catalog: &str,
    parent_workspace_descriptor: Option<WorkspaceDescriptor>,
) -> anyhow::Result<ToolResult> {
    let question = question.trim().to_string();
    let focus = focus.map(|s| s.to_string());

    tracing::info!(
        target: "agent_prepare_context",
        question_chars = question.chars().count(),
        has_focus = focus.as_deref().map(|f| !f.trim().is_empty()).unwrap_or(false),
        "[agent_prepare_context] invoked"
    );

    if question.is_empty() {
        return Ok(ToolResult::error(
            "agent_prepare_context: `question` is required",
        ));
    }

    let registry = match AgentDefinitionRegistry::global() {
        Some(reg) => reg,
        None => {
            return Ok(ToolResult::error(
                "agent_prepare_context: AgentDefinitionRegistry has not been initialised.",
            ));
        }
    };
    let definition = match registry.get(SCOUT_AGENT_ID) {
        Some(def) => def,
        None => {
            return Ok(ToolResult::error(format!(
                "agent_prepare_context: built-in agent `{SCOUT_AGENT_ID}` is not registered.",
            )));
        }
    };

    let catalog_tool_count = tool_catalog.lines().filter(|l| !l.is_empty()).count();
    let scout_prompt =
        AgentPrepareContextTool::build_scout_prompt(&question, focus.as_deref(), tool_catalog);

    tracing::debug!(
        target: "agent_prepare_context",
        catalog_tool_count,
        scout_prompt_chars = scout_prompt.chars().count(),
        "[agent_prepare_context] spawning context_scout (blocking)"
    );

    let task_id = format!("ctx-{}", uuid::Uuid::new_v4());
    let parent_session = current_parent()
        .map(|p| p.session_id.clone())
        .unwrap_or_else(|| "standalone".into());
    let progress_sink = current_parent().and_then(|p| p.on_progress.clone());

    // Surface the scout as a live subagent row in the parent thread. The
    // child's own iterations/tool-calls already stream to this sink from
    // inside run_subagent; we bookend them with spawned/completed so the
    // UI opens and closes the card. Best-effort — a closed sink is fine.
    crate::agent::orchestration::subagent_events::publish_subagent_spawned(
        parent_session.clone(),
        definition.id.clone(),
        "typed".to_string(),
        task_id.clone(),
        scout_prompt.chars().count(),
    );
    if let Some(ref tx) = progress_sink {
        let _ = tx
            .send(AgentProgress::SubagentSpawned {
                agent_id: definition.id.clone(),
                task_id: task_id.clone(),
                mode: "typed".to_string(),
                dedicated_thread: false,
                prompt_chars: scout_prompt.chars().count(),
                prompt: scout_prompt.clone(),
                worker_thread_id: None,
                display_name: Some(definition.display_name().to_string()),
            })
            .await;
    }

    let worktree_action_dir = parent_workspace_descriptor
        .as_ref()
        .map(|descriptor| descriptor.root.clone());
    if let Some(descriptor) = parent_workspace_descriptor.as_ref() {
        tracing::debug!(
            target: "agent_prepare_context",
            task_id = %task_id,
            workspace_root = %descriptor.root.display(),
            policy_id = %descriptor.policy_id,
            "[agent_prepare_context] using ToolExecutionContext workspace root"
        );
    }
    let options = SubagentRunOptions {
        task_id: Some(task_id.clone()),
        worktree_action_dir,
        workspace_descriptor: parent_workspace_descriptor,
        ..Default::default()
    };

    match run_subagent(definition, &scout_prompt, options).await {
        Ok(outcome) => match &outcome.status {
            SubagentRunStatus::Completed => {
                // Guard the contract: the scout MUST return exactly one
                // `[context_bundle] … [/context_bundle]` envelope. We tolerate
                // surrounding prose by extracting just the envelope (the harness
                // prepends any non-error result to turn 1 as "Prepared context",
                // so we still inject only the bracketed envelope, never the
                // model's free-form text). Genuinely unusable output — absent,
                // unterminated, or duplicated — is rejected so the caller falls
                // back to the un-augmented message.
                let Some(bundle) = extract_context_bundle(&outcome.output) else {
                    tracing::warn!(
                        target: "agent_prepare_context",
                        task_id = %outcome.task_id,
                        output_chars = outcome.output.chars().count(),
                        "[agent_prepare_context] scout returned a malformed/absent context_bundle — rejecting"
                    );
                    crate::agent::orchestration::subagent_events::publish_subagent_completed(
                        parent_session.clone(),
                        outcome.task_id.clone(),
                        outcome.agent_id.clone(),
                        outcome.elapsed.as_millis() as u64,
                        0,
                        outcome.iterations,
                    );
                    if let Some(ref tx) = progress_sink {
                        let _ = tx
                            .send(AgentProgress::SubagentCompleted {
                                agent_id: outcome.agent_id.clone(),
                                task_id: outcome.task_id.clone(),
                                elapsed_ms: outcome.elapsed.as_millis() as u64,
                                iterations: outcome.iterations as u32,
                                output_chars: 0,
                                output: String::new(),
                                worktree_path: None,
                                changed_files: Vec::new(),
                                dirty_status: None,
                            })
                            .await;
                    }
                    return Ok(ToolResult::error(
                        "agent_prepare_context: context_scout did not return a well-formed \
                         [context_bundle] envelope",
                    ));
                };
                // From here on use the extracted `bundle`, not the raw
                // `outcome.output`, so any prose the scout wrapped around the
                // envelope never reaches the parent's context.
                tracing::info!(
                    target: "agent_prepare_context",
                    task_id = %outcome.task_id,
                    elapsed_ms = outcome.elapsed.as_millis() as u64,
                    iterations = outcome.iterations,
                    output_chars = bundle.chars().count(),
                    raw_output_chars = outcome.output.chars().count(),
                    "[agent_prepare_context] context bundle ready"
                );
                crate::agent::orchestration::subagent_events::publish_subagent_completed(
                    parent_session.clone(),
                    outcome.task_id.clone(),
                    outcome.agent_id.clone(),
                    outcome.elapsed.as_millis() as u64,
                    bundle.chars().count(),
                    outcome.iterations,
                );
                if let Some(ref tx) = progress_sink {
                    let _ = tx
                        .send(AgentProgress::SubagentCompleted {
                            agent_id: outcome.agent_id.clone(),
                            task_id: outcome.task_id.clone(),
                            elapsed_ms: outcome.elapsed.as_millis() as u64,
                            iterations: outcome.iterations as u32,
                            output_chars: bundle.chars().count(),
                            output: bundle.clone(),
                            worktree_path: None,
                            changed_files: Vec::new(),
                            dirty_status: None,
                        })
                        .await;
                }

                // Bootstrap this thread's goal from the scout's proposal — but
                // ONLY when the thread has none yet. The orchestrator stays
                // authoritative (it sets/replaces via `goal_set`); the
                // context-gathering path just seeds a goal on the first scout of
                // a fresh chat so the harness has something to steer toward.
                // Best-effort — never fails the call.
                if let (Some(parent), Some(thread_id)) = (current_parent(), current_thread_id()) {
                    if let Some(objective) = AgentPrepareContextTool::parse_proposed_goal(&bundle) {
                        match crate::threads::goals::store::set_if_absent(
                            &parent.workspace_dir,
                            &thread_id,
                            &objective,
                            None,
                        )
                        .await
                        {
                            Ok(Some(goal)) => {
                                tracing::info!(
                                    target: "agent_prepare_context",
                                    thread_id = %thread_id,
                                    goal_id = %goal.goal_id,
                                    "[agent_prepare_context] bootstrapped thread goal from scout proposal"
                                );
                                BUS.publish(DomainEvent::ThreadGoalUpdated {
                                    thread_id: goal.thread_id.clone(),
                                    goal_id: goal.goal_id.clone(),
                                    status: goal.status.as_str().to_string(),
                                });
                            }
                            Ok(None) => {
                                tracing::debug!(
                                    target: "agent_prepare_context",
                                    thread_id = %thread_id,
                                    "[agent_prepare_context] thread already has a goal — scout proposal not applied"
                                );
                            }
                            Err(e) => {
                                tracing::debug!(
                                    target: "agent_prepare_context",
                                    error = %e,
                                    "[agent_prepare_context] failed to persist scout-proposed goal"
                                );
                            }
                        }
                    }
                }

                Ok(ToolResult::success(bundle))
            }
            // The scout has no `ask_user_clarification` tool, so this
            // branch should not fire — handle defensively rather than
            // leaking a confusing checkpoint envelope to the parent.
            SubagentRunStatus::AwaitingUser { question, .. } => {
                tracing::warn!(
                    target: "agent_prepare_context",
                    task_id = %outcome.task_id,
                    "[agent_prepare_context] scout unexpectedly awaited user input"
                );
                // Close the domain-event lifecycle too — a SubagentSpawned
                // was already published, so emit Completed to avoid a
                // dangling spawned state for event-bus consumers.
                crate::agent::orchestration::subagent_events::publish_subagent_completed(
                    parent_session.clone(),
                    outcome.task_id.clone(),
                    outcome.agent_id.clone(),
                    outcome.elapsed.as_millis() as u64,
                    0,
                    outcome.iterations,
                );
                if let Some(ref tx) = progress_sink {
                    let _ = tx
                        .send(AgentProgress::SubagentCompleted {
                            agent_id: outcome.agent_id.clone(),
                            task_id: outcome.task_id.clone(),
                            elapsed_ms: outcome.elapsed.as_millis() as u64,
                            iterations: outcome.iterations as u32,
                            output_chars: 0,
                            output: String::new(),
                            worktree_path: None,
                            changed_files: Vec::new(),
                            dirty_status: None,
                        })
                        .await;
                }
                Ok(ToolResult::success(format!(
                    "[context_bundle]\nhas_enough_context: false\n\
                     summary: The context scout could not complete without clarification: {question}\n\
                     recommended_tool_calls:\n[/context_bundle]"
                )))
            }
            SubagentRunStatus::Incomplete { reason } => {
                // The scout stopped short (stuck halt / iteration cap) without a
                // well-formed bundle. Don't inject partial context — return a
                // has_enough_context:false bundle and close the lifecycle.
                tracing::warn!(
                    target: "agent_prepare_context",
                    task_id = %outcome.task_id,
                    reason = %reason,
                    "[agent_prepare_context] scout stopped incomplete — returning empty bundle"
                );
                crate::agent::orchestration::subagent_events::publish_subagent_completed(
                    parent_session.clone(),
                    outcome.task_id.clone(),
                    outcome.agent_id.clone(),
                    outcome.elapsed.as_millis() as u64,
                    0,
                    outcome.iterations,
                );
                if let Some(ref tx) = progress_sink {
                    let _ = tx
                        .send(AgentProgress::SubagentCompleted {
                            agent_id: outcome.agent_id.clone(),
                            task_id: outcome.task_id.clone(),
                            elapsed_ms: outcome.elapsed.as_millis() as u64,
                            iterations: outcome.iterations as u32,
                            output_chars: 0,
                            output: String::new(),
                            worktree_path: None,
                            changed_files: Vec::new(),
                            dirty_status: None,
                        })
                        .await;
                }
                Ok(ToolResult::success(format!(
                    "[context_bundle]\nhas_enough_context: false\n\
                     summary: The context scout stopped before finishing ({reason}).\n\
                     recommended_tool_calls:\n[/context_bundle]"
                )))
            }
        },
        Err(err) => {
            let message = err.to_string();
            let error_kind = message
                .split(':')
                .next()
                .map(str::trim)
                .unwrap_or("unknown");
            log_scout_failure(error_kind, &scout_failure_signal(&err));
            crate::agent::orchestration::subagent_events::publish_subagent_failed(
                parent_session.clone(),
                task_id.clone(),
                definition.id.clone(),
                message.clone(),
            );
            if let Some(ref tx) = progress_sink {
                let _ = tx
                    .send(AgentProgress::SubagentFailed {
                        agent_id: definition.id.clone(),
                        task_id: task_id.clone(),
                        error: message.clone(),
                    })
                    .await;
            }
            Ok(ToolResult::error(format!(
                "agent_prepare_context failed: {message}"
            )))
        }
    }
}
