//! Public entry points (`start`/`stop`/`resume`) and the spawned engine loop
//! that drives a run's phase DAG to completion.

use anyhow::{anyhow, Context, Result};
use serde_json::Value;

use crate::agent::orchestration::parent_context::with_root_parent;
use crate::config::Config;
use tinyagents_session::run_ledger::{
    get_workflow_run, upsert_workflow_run, WorkflowRun, WorkflowRunStatus, WorkflowRunUpsert,
};

use super::super::ops::definition_by_id;
use super::super::types::WorkflowDefinition;
use super::cancel::{clear_cancel_flag, lookup_cancel_flag, register_cancel_flag};
use super::state::{init_phase_states, persist};
use super::LOG_TARGET;

/// Start a new workflow run and return immediately.
///
/// Resolves `definition_id` to a builtin [`WorkflowDefinition`], creates a
/// `Running` ledger row with `phase_states` initialised to one `pending` entry
/// per phase, persists it, then `tokio::spawn`s the engine loop. The returned
/// [`WorkflowRun`] is the freshly-created row (status `Running`); callers poll
/// `workflow_run_get` to observe progress.
pub async fn start_workflow_run(
    config: &Config,
    definition_id: &str,
    input: Value,
    parent_thread_id: Option<String>,
) -> Result<WorkflowRun> {
    log::debug!(
        target: LOG_TARGET,
        "[workflow_run_engine] start.entry definition={definition_id} parent_thread={parent_thread_id:?}"
    );
    let definition = definition_by_id(definition_id)
        .ok_or_else(|| anyhow!("unknown workflow definition: {definition_id}"))?;

    let run_id = format!("wfrun-{}", uuid::Uuid::new_v4());
    let phase_states = init_phase_states(&definition);

    let run = upsert_workflow_run(
        &config.workspace_dir,
        WorkflowRunUpsert {
            id: run_id.clone(),
            definition_id: definition.id.clone(),
            parent_thread_id,
            input: input.clone(),
            phase_states,
            child_run_ids: Vec::new(),
            status: WorkflowRunStatus::Running,
            summary: None,
            started_at: None,
            completed_at: None,
        },
    )
    .context("persist initial workflow run")?;

    register_cancel_flag(&run_id);

    // Spawn the engine loop. Clone what the task needs (the engine reloads
    // config inside the task so it can build a real Agent without holding a
    // borrow across the spawn boundary).
    let task_run_id = run_id.clone();
    // Task-locals don't cross `tokio::spawn`, so capture the starting turn's
    // origin here and re-scope it around the engine loop — the phases spawn
    // sub-agents whose tool calls the approval gate judges by that label.
    // Inherit-only: `None` leaves the loop unlabelled and failing closed.
    let inherited_origin = crate::agent::turn_origin::capture();
    tokio::spawn(async move {
        match Config::load_or_init().await {
            Ok(task_config) => {
                crate::agent::turn_origin::with_inherited_origin(
                    inherited_origin,
                    run_engine_loop(&task_config, &task_run_id, definition),
                )
                .await;
            }
            Err(err) => {
                log::error!(
                    target: LOG_TARGET,
                    "[workflow_run_engine] start.config_load_failed run={task_run_id} err={err}"
                );
            }
        }
    });

    log::debug!(
        target: LOG_TARGET,
        "[workflow_run_engine] start.spawned run={run_id} phases={}",
        run.phase_states.as_object().map(|m| m.len()).unwrap_or(0)
    );
    Ok(run)
}

/// Signal a running workflow to stop after its current phase.
///
/// Flips the run's cancellation flag (checked by the loop between phases) and
/// eagerly marks the persisted row `Interrupted` so a poller sees the intent
/// immediately even while the in-flight phase drains. Idempotent: stopping a
/// terminal or unknown run is a no-op that returns the current row.
pub async fn stop_workflow_run(config: &Config, id: &str) -> Result<Option<WorkflowRun>> {
    log::debug!(target: LOG_TARGET, "[workflow_run_engine] stop.entry run={id}");
    let Some(run) = get_workflow_run(&config.workspace_dir, id)? else {
        log::debug!(target: LOG_TARGET, "[workflow_run_engine] stop.unknown run={id}");
        return Ok(None);
    };

    if matches!(
        run.status,
        WorkflowRunStatus::Completed | WorkflowRunStatus::Failed | WorkflowRunStatus::Cancelled
    ) {
        log::debug!(
            target: LOG_TARGET,
            "[workflow_run_engine] stop.already_terminal run={id} status={}",
            run.status.as_str()
        );
        return Ok(Some(run));
    }

    if let Some(signal) = super::cancel::lookup_cancel_signal(id) {
        signal.flag.store(true, std::sync::atomic::Ordering::SeqCst);
        signal.token.cancel();
    } else {
        // No live loop (e.g. process restart) — register a flag anyway so a
        // future resume observes the stop intent.
        let signal = super::cancel::register_cancel_signal(id);
        signal.flag.store(true, std::sync::atomic::Ordering::SeqCst);
        signal.token.cancel();
    }

    let updated = upsert_workflow_run(
        &config.workspace_dir,
        WorkflowRunUpsert {
            id: run.id.clone(),
            definition_id: run.definition_id.clone(),
            parent_thread_id: run.parent_thread_id.clone(),
            input: run.input.clone(),
            phase_states: run.phase_states.clone(),
            child_run_ids: run.child_run_ids.clone(),
            status: WorkflowRunStatus::Interrupted,
            summary: run.summary.clone(),
            started_at: Some(run.started_at),
            completed_at: None,
        },
    )
    .context("persist workflow run interrupt")?;

    log::debug!(target: LOG_TARGET, "[workflow_run_engine] stop.marked_interrupted run={id}");
    Ok(Some(updated))
}

/// Resume an interrupted (or otherwise incomplete) workflow run.
///
/// Reloads the run, clears any stale cancellation flag, flips the row back to
/// `Running`, and spawns a fresh engine loop. Phases already `completed` in
/// `phase_states` are skipped; the loop continues from the first incomplete
/// phase whose dependencies are satisfied. Returns the run row (now `Running`),
/// or an error if the run is unknown / already terminal-complete / its
/// definition no longer exists.
pub async fn resume_workflow_run(config: &Config, id: &str) -> Result<WorkflowRun> {
    log::debug!(target: LOG_TARGET, "[workflow_run_engine] resume.entry run={id}");
    let run = get_workflow_run(&config.workspace_dir, id)?
        .ok_or_else(|| anyhow!("unknown workflow run: {id}"))?;

    if matches!(run.status, WorkflowRunStatus::Completed) {
        return Err(anyhow!("workflow run {id} is already completed"));
    }

    let definition = definition_by_id(&run.definition_id)
        .ok_or_else(|| anyhow!("definition {} no longer exists", run.definition_id))?;

    // Clear any prior cancellation intent and re-register a fresh flag.
    clear_cancel_flag(id);
    register_cancel_flag(id);

    let resumed = upsert_workflow_run(
        &config.workspace_dir,
        WorkflowRunUpsert {
            id: run.id.clone(),
            definition_id: run.definition_id.clone(),
            parent_thread_id: run.parent_thread_id.clone(),
            input: run.input.clone(),
            phase_states: run.phase_states.clone(),
            child_run_ids: run.child_run_ids.clone(),
            status: WorkflowRunStatus::Running,
            summary: run.summary.clone(),
            started_at: Some(run.started_at),
            completed_at: None,
        },
    )
    .context("persist workflow run resume")?;

    let task_run_id = id.to_string();
    // Same inherit-only origin propagation as `start_workflow_run`: the resumed
    // loop runs on a fresh task, which would otherwise drop the caller's label.
    let inherited_origin = crate::agent::turn_origin::capture();
    tokio::spawn(async move {
        match Config::load_or_init().await {
            Ok(task_config) => {
                crate::agent::turn_origin::with_inherited_origin(
                    inherited_origin,
                    run_engine_loop(&task_config, &task_run_id, definition),
                )
                .await;
            }
            Err(err) => {
                log::error!(
                    target: LOG_TARGET,
                    "[workflow_run_engine] resume.config_load_failed run={task_run_id} err={err}"
                );
            }
        }
    });

    log::debug!(target: LOG_TARGET, "[workflow_run_engine] resume.spawned run={id}");
    Ok(resumed)
}

/// Build the root parent context + drive the phase DAG to completion.
///
/// Separated from [`start_workflow_run`] so it can run on the spawned task with
/// an owned [`Config`]. Errors are recorded on the run row (status `Failed`)
/// rather than propagated — there is no caller to receive them.
pub(crate) async fn run_engine_loop(config: &Config, run_id: &str, definition: WorkflowDefinition) {
    let cancel = lookup_cancel_flag(run_id).unwrap_or_else(|| register_cancel_flag(run_id));

    let outcome = with_root_parent(config, "workflow_engine", "workflow", "workflow", async {
        super::super::graph::drive_phases(config, run_id, &definition, &cancel).await
    })
    .await
    // Flatten: outer Err = root-parent build failure, inner = drive_phases result.
    .unwrap_or_else(Err);

    if let Err(err) = outcome {
        log::error!(
            target: LOG_TARGET,
            "[workflow_run_engine] loop.failed run={run_id} err={err}"
        );
        // Best-effort terminal failure write, preserving partial phase state.
        if let Ok(Some(run)) = get_workflow_run(&config.workspace_dir, run_id) {
            if !matches!(
                run.status,
                WorkflowRunStatus::Completed
                    | WorkflowRunStatus::Failed
                    | WorkflowRunStatus::Cancelled
                    | WorkflowRunStatus::Interrupted
            ) {
                let _ = persist(
                    config,
                    &run,
                    run.phase_states.clone(),
                    run.child_run_ids.clone(),
                    WorkflowRunStatus::Failed,
                    Some(format!("engine error: {err}")),
                    true,
                );
            }
        }
    }

    clear_cancel_flag(run_id);
}
