//! `dispatch` step of the phase loop: reload the run, honour cancellation,
//! and pick the next runnable phase.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};

use tinyagents_session::run_ledger::{get_workflow_run, WorkflowRunStatus};

use super::super::types::{WorkflowDefinition, WorkflowPhase};
use super::state::{all_phases_completed, next_runnable_phase, persist, synthesize_summary};
use super::LOG_TARGET;
use crate::config::Config;

/// What the scheduler's `dispatch` step decided.
pub(crate) enum PhaseSelection {
    /// Execute this phase next.
    Run(WorkflowPhase),
    /// The run reached a terminal status (already persisted) — route to `done`.
    Terminated,
}

/// `dispatch` step: reload the run, honour cancellation, and pick the next
/// runnable phase (pending, all deps `completed`). When none remains, persist the
/// terminal status (Completed / Failed) and return [`PhaseSelection::Terminated`].
pub(crate) async fn select_next_phase(
    config: &Config,
    run_id: &str,
    definition: &WorkflowDefinition,
    cancel: &Arc<AtomicBool>,
    session: &crate::agent::orchestration::AgentOrchestrationSession,
) -> Result<PhaseSelection> {
    // Reload so we read the latest phase_states (and a resume picks up persisted
    // progress).
    let run = get_workflow_run(&config.workspace_dir, run_id)?
        .ok_or_else(|| anyhow!("workflow run {run_id} vanished mid-loop"))?;
    let phase_states = run.phase_states.clone();
    let child_run_ids = run.child_run_ids.clone();

    // Cancellation check between phases.
    if cancel.load(Ordering::SeqCst) {
        log::debug!(
            target: LOG_TARGET,
            "[workflow_run_engine] loop.cancelled run={run_id}"
        );
        session.abort_all().await;
        persist(
            config,
            &run,
            phase_states,
            child_run_ids,
            WorkflowRunStatus::Interrupted,
            None,
            false,
        )?;
        return Ok(PhaseSelection::Terminated);
    }

    // Find the next runnable phase: pending, with all deps completed.
    let Some(phase) = next_runnable_phase(definition, &phase_states) else {
        // No runnable phase left. Either everything is done, or we're blocked
        // (which a validated DAG shouldn't be).
        if all_phases_completed(definition, &phase_states) {
            let summary = synthesize_summary(definition, &phase_states);
            log::debug!(
                target: LOG_TARGET,
                "[workflow_run_engine] loop.completed run={run_id} summary_chars={}",
                summary.as_deref().map(str::len).unwrap_or(0)
            );
            persist(
                config,
                &run,
                phase_states,
                child_run_ids,
                WorkflowRunStatus::Completed,
                summary,
                true,
            )?;
        } else {
            log::warn!(
                target: LOG_TARGET,
                "[workflow_run_engine] loop.stuck run={run_id} no_runnable_phase"
            );
            persist(
                config,
                &run,
                phase_states,
                child_run_ids,
                WorkflowRunStatus::Failed,
                Some("no runnable phase (dependency deadlock)".to_string()),
                true,
            )?;
        }
        return Ok(PhaseSelection::Terminated);
    };

    Ok(PhaseSelection::Run(phase.clone()))
}
