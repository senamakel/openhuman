//! OpenHuman policy and worker adapters for the multi-stage delegation tool.
//!
//! TinyAgents' `DelegationTool` owns the durable plan→execute⇄review→finalize
//! mechanics. This module supplies the product-specific stage prompts, target
//! definition, checkpoint configuration, approval interpretation, and result
//! projection. Each host stage worker enters the shared subagent driver so its
//! lifecycle is visible to the same Jobs and Message tools as other children.
//!
//! The host retains its caller context, trust and progress policy; it does not
//! implement the generic graph state machine or job lifecycle.

use std::sync::Arc;

use crate::agent::subagent_host::{
    run_subagent_via_driver_tool, SubagentRunOptions, SubagentRunStatus,
};
use crate::agent::tinyagents::host::OpenHumanRunContext;
use crate::agent_registry::host_definition::AgentDefinition;
use crate::config::Config;
use crate::progress::AgentProgress;
use tinyagents_graph::checkpoint::Checkpointer;
use tinyagents_graph::delegation::{
    DelegationConfig, DelegationStage, DelegationStageOutput, DelegationState,
};
use tinyagents_harness::context::{RunConfig, RunContext};
use tinyagents_harness::tool::ToolDispatch;
use tinytools::{
    PermissionLevel, ToolCallOptions, ToolCategory, ToolResult, ToolScope, ToolTimeout,
    WorkspaceDescriptor,
};

const LOG_TARGET: &str = "agent_orchestration::delegation";

/// Derives one delegation stage's run context and host data from the stage
/// parent. The host carrier is pointed at the stage run's own linked child
/// token, so a graph cancel still reaches the stage through the parent while
/// cancelling the stage leaves the graph (and its siblings) running.
pub(crate) fn stage_run_contexts(
    stage_parent: &RunContext<OpenHumanRunContext>,
    workspace: Option<WorkspaceDescriptor>,
) -> tinyagents_harness::Result<(RunContext<OpenHumanRunContext>, OpenHumanRunContext)> {
    let mut stage_context = stage_parent.data.child();
    stage_context.workspace = workspace.or(stage_context.workspace);
    let mut stage_run = stage_parent.child(
        RunConfig::new(format!("delegation-stage-{}", uuid::Uuid::new_v4())),
        stage_context.clone(),
    )?;
    stage_context.cancellation = stage_run.cancellation.clone();
    stage_run.data.cancellation = stage_run.cancellation.clone();
    Ok((stage_run, stage_context))
}

/// Typed live entrypoint for the durable delegation graph.
///
/// The graph does create a durable graph id for its checkpoints, but each
/// stage retains the caller's cancellation, origin, dispatch, progress,
/// conversation thread, and workspace through the supplied carrier.
pub(crate) async fn run_subagent_delegation_with_parent_context(
    config: Arc<Config>,
    definition: AgentDefinition,
    task_prompt: String,
    max_revisions: usize,
    parent_workspace_descriptor: Option<WorkspaceDescriptor>,
    live_parent: Arc<RunContext<crate::agent::tinyagents::host::OpenHumanRunContext>>,
) -> Result<ToolResult, String> {
    let thread_id = format!("delegrun-{}", uuid::Uuid::new_v4());
    // The graph and every stage share this cancellation token. A stage gets an
    // owned host carrier before it enters `run_subagent`, rather than making a
    // fresh context after the graph has spawned work.
    // Durable graph checkpoints live in the storage backend, in the acting
    // agent's scope (`DriverCheckpointer`).
    let checkpointer = open_delegation_checkpointer(&config)?;

    tracing::info!(
        target: LOG_TARGET,
        agent_id = %definition.id,
        thread_id = %thread_id,
        max_revisions,
        "[delegation] starting durable sub-agent delegation"
    );
    if let Some(descriptor) = parent_workspace_descriptor.as_ref() {
        tracing::debug!(
            target: LOG_TARGET,
            agent_id = %definition.id,
            thread_id = %thread_id,
            workspace_root = %descriptor.root.display(),
            policy_id = %descriptor.policy_id,
            "[delegation] using ToolExecutionContext workspace root"
        );
    }

    let invocation_agent_id = definition.id.clone();
    let invocation_task_prompt = task_prompt.clone();
    let stage_parent = live_parent.clone();
    let run_stage = move |_stage_args: serde_json::Value,
                          stage: DelegationStage,
                          state: DelegationState| {
        let definition = definition.clone();
        let task = task_prompt.clone();
        let workspace_descriptor = parent_workspace_descriptor.clone();
        let stage_parent = stage_parent.clone();
        async move {
            let prompt = build_stage_prompt(stage, &task, &state);
            let (stage_parent, stage_context) =
                stage_run_contexts(&stage_parent, workspace_descriptor.clone())
                    .map_err(|error| format!("delegation stage context: {error}"))?;
            match run_subagent_via_driver_tool(
                &stage_parent,
                definition,
                prompt.clone(),
                delegation_subagent_options(workspace_descriptor, stage_context),
            )
            .await
            {
                Ok((outcome, _public_job_id)) => {
                    let should_emit_lifecycle_effects = outcome.should_emit_lifecycle_effects();
                    match outcome.status {
                        SubagentRunStatus::Completed => {}
                        SubagentRunStatus::AwaitingUser {
                            question,
                            checkpoint,
                            ..
                        } => {
                            // A durable delegation graph cannot safely advance
                            // plan/review state while one of its stages is
                            // paused. Surface the exact durable child handle so
                            // the caller can continue that stage instead of
                            // silently stranding the checkpoint behind a generic
                            // graph error.
                            if should_emit_lifecycle_effects {
                                let parent_session = stage_parent
                                    .data
                                    .parent
                                    .as_ref()
                                    .map(|parent| parent.session_id.clone())
                                    .unwrap_or_else(|| "standalone".to_string());
                                crate::agent::orchestration::subagent_events::publish_subagent_awaiting_user(
                                        parent_session,
                                        outcome.task_id.clone(),
                                        outcome.agent_id.clone(),
                                        question.clone(),
                                    );
                                if let Some(progress) = stage_parent.data.progress.clone() {
                                    let _ = progress
                                        .send(AgentProgress::SubagentAwaitingUser {
                                            agent_id: outcome.agent_id.clone(),
                                            task_id: outcome.task_id.clone(),
                                            question: question.clone(),
                                            worker_thread_id: None,
                                            checkpoint_path: checkpoint
                                                .as_ref()
                                                .map(|path| path.to_string_lossy().to_string()),
                                        })
                                        .await;
                                }
                            }
                            return Err(format!(
                                "[SUBAGENT_AWAITING_USER]\nagent_id: {}\ntask_id: {}\nquestion: {}\ncheckpointed: {}\n[/SUBAGENT_AWAITING_USER]\n\
                                     delegation stage {stage:?} is awaiting user input; relay the answer with continue_subagent using this task_id.",
                                    outcome.agent_id,
                                    outcome.task_id,
                                    serde_json::to_string(&question).unwrap_or_else(|_| {
                                        "\"<unserializable question>\"".to_string()
                                    }),
                                    checkpoint.is_some(),
                            ));
                        }
                        SubagentRunStatus::Incomplete { reason, .. } => {
                            return Err(format!(
                                "delegation stage {stage:?} stopped incomplete: {reason}"
                            ));
                        }
                        SubagentRunStatus::Cancelled => {
                            return Err(format!("delegation stage {stage:?} was cancelled"));
                        }
                    }
                    let approved = matches!(stage, DelegationStage::Review)
                        && review_approves(&outcome.output);
                    Ok(DelegationStageOutput {
                        text: outcome.output,
                        approved,
                        // Persist the exact prompt this stage sent so the
                        // execute step's `StepRecord` carries its provenance
                        // (read only for the execute stage; #3884).
                        prompt: Some(prompt),
                    })
                }
                Err(e) => Err(format!("delegation stage {stage:?} failed: {e}")),
            }
        }
    };
    let config_checkpointer = Arc::clone(&checkpointer);
    let config_thread_id = thread_id.clone();
    let config_factory =
        move |_state: &(), parent: &RunContext<OpenHumanRunContext>, _args: &serde_json::Value| {
            let mut delegation_config = DelegationConfig {
                max_revisions,
                checkpointer: Some(Arc::clone(&config_checkpointer)),
                thread_id: Some(config_thread_id.clone()),
                cancel: parent.cancellation.clone(),
                // Automated delegation: preserve the host's existing worker-output
                // review policy while the generic tool owns graph lifecycle mechanics.
                ..DelegationConfig::default()
            };
            delegation_config.event_sink = Some(Arc::new(tinyagents_graph::GraphTracingSink::new(
                "delegation:graph",
            )));
            delegation_config
        };
    let result_agent_id = invocation_agent_id.clone();
    let tool = tinyagents_orchestration::subagent::DelegationTool::new(config_factory, run_stage)
        .with_tool_name("delegate_graph")
        .with_description("Durable plan, execute, review, and finalize delegation.")
        .with_parameters(serde_json::json!({
            "type": "object",
            "required": ["task"],
            "properties": {"task": {"type": "string"}}
        }))
        .with_resumable_execution()
        .with_review_approval_policy(
            tinyagents_orchestration::subagent::ReviewApprovalPolicy::WorkerOutput,
        )
        .with_metadata(
            PermissionLevel::Execute,
            ToolScope::All,
            ToolCategory::System,
            ToolTimeout::Unbounded,
        )
        .with_result_projection(move |result| match result {
            Ok(outcome) if outcome.pending.is_some() => {
                let pending = outcome.pending.expect("checked above");
                ToolResult::json(serde_json::json!({
                    "status": "pending",
                    "state": outcome.state,
                    "pending": {
                        "interrupt_id": pending.interrupt_id,
                        "node": pending.node,
                        "payload": pending.payload,
                        "thread_id": pending.thread_id,
                    }
                }))
            }
            Ok(outcome) if outcome.state.cancelled => {
                ToolResult::error(format!("delegate_graph cancelled for '{result_agent_id}'."))
            }
            Ok(outcome) => {
                let state = outcome.state;
                let final_output = state
                    .final_output
                    .unwrap_or_else(|| "(delegation produced no final output)".to_string());
                let note = if state.revisions > 0 {
                    " (after revision)"
                } else {
                    ""
                };
                ToolResult::success(format!(
                    "[Delegated to {result_agent_id}{note}, {} review pass(es)]\n{final_output}",
                    state.reviews.len()
                ))
            }
            Err(error) => ToolResult::error(format!(
                "delegate_graph failed for '{result_agent_id}': {error}"
            )),
        });

    tool.execute(
        &(),
        tinyagents_harness::CallId::new(format!("delegation-{thread_id}")),
        serde_json::json!({
            "agent_id": invocation_agent_id,
            "task": invocation_task_prompt,
            "max_revisions": max_revisions
        }),
        ToolCallOptions::default(),
        &live_parent,
    )
    .await
    .map_err(|error| format!("delegate_graph failed: {error}"))
}

/// Per-stage prompt builder: each stage sees the task plus the accumulated state
/// (the plan for `execute`, the latest result for `review`, and the latest
/// reviewer feedback when re-executing after a revision request).
fn build_stage_prompt(stage: DelegationStage, task: &str, state: &DelegationState) -> String {
    match stage {
        DelegationStage::Plan => format!(
            "Produce a short, concrete, numbered plan to accomplish the task below. \
             Reply with the plan only.\n\n[Task]\n{task}"
        ),
        DelegationStage::Execute => {
            let plan = state.plan.as_deref().unwrap_or("(no plan produced)");
            let feedback = state
                .reviews
                .last()
                .map(|r| format!("\n\n[Reviewer feedback to address]\n{r}"))
                .unwrap_or_default();
            format!(
                "Carry out the plan below for the task and return the completed result.\n\n\
                 [Task]\n{task}\n\n[Plan]\n{plan}{feedback}"
            )
        }
        DelegationStage::Review => {
            let result = state.last_result().unwrap_or("(no execution produced)");
            format!(
                "Review the result below against the task. If it fully and correctly \
                 accomplishes the task, reply with `APPROVE` on the first line. Otherwise \
                 reply with `REVISE` on the first line followed by specific, actionable \
                 feedback.\n\n[Task]\n{task}\n\n[Result]\n{result}"
            )
        }
    }
}

/// A review stage approves when its first line begins with `APPROVE`.
fn review_approves(output: &str) -> bool {
    output
        .lines()
        .next()
        .map(|l| l.trim().to_ascii_uppercase())
        .map(|l| l.starts_with("APPROVE"))
        .unwrap_or(false)
}

/// Default sub-agent options for a delegation stage — a fresh UUID task id per
/// call (so retries/revisions don't collide), everything else inherited.
fn delegation_subagent_options(
    workspace_descriptor: Option<WorkspaceDescriptor>,
    run_context: crate::agent::tinyagents::host::OpenHumanRunContext,
) -> SubagentRunOptions {
    let worktree_action_dir = workspace_descriptor
        .as_ref()
        .map(|descriptor| descriptor.root.clone());
    SubagentRunOptions {
        skill_filter_override: None,
        context: None,
        model_override: None,
        task_id: None,
        thread_id: run_context.thread_id.clone(),
        run_context,
        worker_thread_id: None,
        initial_history: None,
        checkpoint_dir: None,
        worktree_action_dir,
        workspace_descriptor,
        run_queue: None,
        message_run_queue: None,
    }
}

/// Open the delegation graph's durable checkpointer: the acting agent's scope
/// on the storage backend ([`crate::storage::local::open`]).
pub fn open_delegation_checkpointer(
    config: &Config,
) -> Result<Arc<dyn Checkpointer<DelegationState>>, String> {
    let scoped = crate::storage::local::open("delegation", &config.workspace_dir)
        .and_then(|opened| opened.scoped())
        .map_err(|e| format!("open the storage for graph checkpoints: {e:#}"))?;
    Ok(Arc::new(
        tinyagents_graph::checkpoint::DriverCheckpointer::<DelegationState>::with_namespace(
            Arc::clone(scoped.documents()),
            "delegation",
        ),
    ))
}

#[cfg(test)]
#[path = "delegation_tests.rs"]
mod tests;
