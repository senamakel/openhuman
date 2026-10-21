//! Live execution runtime for agent-team members (#3374 PR4).
//!
//! PR1–PR3 shipped the durable team model, race-safe claiming, quality-gated
//! completion, and the read-only board. They were entirely *store-only*: a
//! claim flipped a row, nothing ran. This module adapts the owner
//! `TeamService::start_member_with_started` to OpenHuman's actual sub-agent
//! executor. The owner atomically claims and activates a member, returns an
//! admission receipt, then keeps the same future through evidence validation,
//! completion, and cleanup.
//!
//! ## Root parent context
//!
//! Like the workflow engine (#3375), the worker is spawned from a controller
//! background task with no enclosing agent turn, so it builds a root
//! [`ParentExecutionContext`] (shared [`build_root_parent`]) and passes it to
//! `spawn_agent` — every spawned worker then inherits a real provider / tools /
//! memory.
//!
//! ## Message delivery boundary
//!
//! Pending lead/teammate messages addressed to the member are injected into the
//! worker's prompt **at spawn** (a well-defined boundary), and are always
//! visible in the team timeline. Mid-turn injection into an already-running
//! harness loop is intentionally **not** supported — the orchestration layer has
//! no live inbox. Boundary delivery satisfies the issue's "see the message in the team
//! timeline or worker thread" criterion via both paths; a live inbox would be a
//! separate orchestration-layer change.

use anyhow::{anyhow, Result};
use serde_json::json;

use crate::agent::harness::ParentExecutionContext;
use crate::agent::orchestration::parent_context::with_root_parent;
use crate::config::Config;
use tinyagents_session::run_ledger::{
    AgentTeamMember, AgentTeamMemberStatus, AgentTeamTask, RunEventAppend,
};
use {
    crate::agent::orchestration::AgentOrchestrationSession,
    crate::agent::orchestration::SpawnAgentRequest, crate::agent::orchestration::WaitAgentOptions,
    tinyagents_tasks::OrchestrationTaskStatus,
};

use tinyagents_orchestration::teams::{
    build_member_prompt, claimable_task, deliver_pending_messages, truncate_chars,
    SessionTeamLedger, TeamError, TeamMemberExecutor, TeamMemberStartError,
};
const LOG_TARGET: &str = "agent_team_runtime";
/// Fallback archetype when a member carries no explicit `agent_id`: the
/// orchestrator, the one general-purpose agent with the full direct belt plus
/// `tool_search` and skills. Same fallback a cron job with an unknown agent
/// uses (`cron/scheduler/agent_run.rs`).
const DEFAULT_TEAMMATE_AGENT_ID: &str = "orchestrator";
/// Event recorded when a worker run ends without completing its task.
const MEMBER_FAILED_EVENT: &str = "team_member_failed";
/// Cap on how much worker output is captured as evidence (UTF-8 safe).
const EVIDENCE_MAX_CHARS: usize = 280;

/// Host-side result of accepting a request to run a durable team member.
///
/// Actual worker execution remains OpenHuman-specific because it creates a
/// root parent context, selects a model and tools, and emits host progress.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum StartMemberOutcome {
    Started {
        run_id: String,
        task: Box<AgentTeamTask>,
    },
    Blocked {
        unmet: Vec<String>,
    },
    AlreadyClaimed,
    AlreadyActive,
    NoClaimableTask,
    UnknownTask,
}

/// Start a live run for a team member. **Non-blocking.**
///
/// Resolves a target task (the explicit `task_id`, else the member's next
/// claimable ready task), atomically claims it, marks the member `active` with
/// the new `run_id`, and `tokio::spawn`s the worker loop — then returns
/// immediately. The UI observes progress by polling `agent_team_get`.
///
/// Returns a non-`Started` [`StartMemberOutcome`] (no side effects) when no work
/// could be dispatched: the task is already claimed, blocked on dependencies,
/// unknown, or the member has nothing claimable. An unknown member surfaces as
/// [`TeamError::UnknownMember`].
pub async fn start_member_run(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    team_id: &str,
    member_id: &str,
    task_id: Option<&str>,
    model_override: Option<String>,
) -> Result<StartMemberOutcome> {
    log::debug!(
        target: LOG_TARGET,
        "[agent_team_runtime] start.entry team={team_id} member={member_id} task={task_id:?}"
    );

    let member = run_ledger::get_agent_team_member(&config.workspace_dir, member_id)
        .await?
        .filter(|m| m.team_id == team_id)
        .ok_or_else(|| {
            anyhow!(TeamError::UnknownMember {
                member_id: member_id.to_string(),
            })
        })?;

    // Reject a start on an already-active member before any claim or state
    // mutation. The UI hides the control for active members, but this entry
    // point is reachable directly over RPC; without this guard two near-
    // simultaneous calls would each claim a task and the second
    // `mark_agent_team_member_running` would clobber the first's task/run
    // pointer, leaving two workers for one member.
    if member.member_status == AgentTeamMemberStatus::Active {
        log::debug!(
            target: LOG_TARGET,
            "[agent_team_runtime] start.reject_active team={team_id} member={member_id}"
        );
        return Ok(StartMemberOutcome::AlreadyActive);
    }

    // Resolve the target task: an explicit id, or the member's next claimable
    // ready task (unowned or owned-by-this-member, dependencies all done).
    let tasks = run_ledger::list_agent_team_tasks(&config.workspace_dir, team_id).await?;
    let target = match task_id {
        Some(tid) => match tasks.iter().find(|t| t.id == tid) {
            Some(t) => t.clone(),
            None => return Ok(StartMemberOutcome::UnknownTask),
        },
        None => match claimable_task(&tasks, member_id) {
            Some(t) => t.clone(),
            None => return Ok(StartMemberOutcome::NoClaimableTask),
        },
    };

    let executor = HostTeamMemberExecutor {
        ctx: ctx.clone(),
        config: config.clone(),
        team_id: team_id.to_string(),
        member_id: member_id.to_string(),
        model_override,
        parent_override: None,
    };
    start_member_with_executor(ctx, config, team_id, member_id, target, executor).await
}

/// Run the owner admission path behind an immediate RPC receipt. Kept generic
/// so tests can hold execution after the receipt and exercise the real host
/// boundary without a model backend.
async fn start_member_with_executor<E: TeamMemberExecutor + 'static>(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    team_id: &str,
    member_id: &str,
    target: AgentTeamTask,
    executor: E,
) -> Result<StartMemberOutcome> {
    // TinyAgents owns the claim, active transition, cleanup and completion.
    // The receipt crosses back to this RPC as soon as the owner has durably
    // admitted the run; the same owner future then continues executing it.
    let ledger = crate::storage::sessions::require_ledger(&config.workspace_dir)
        .map_err(|error| anyhow!("{error}"))?;
    let service =
        SessionTeamLedger::new(ledger).map(tinyagents_orchestration::teams::TeamService::new)?;
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let mut started_tx = Some(started_tx);
    let team = team_id.to_string();
    let mem = member_id.to_string();
    let task_id = target.id.clone();
    let task_config = config.clone();
    let service_task = service.clone();
    // The member loop drives real agent work on a fresh task, and task-locals
    // don't cross `tokio::spawn` — capture the caller's turn origin here so the
    // worker keeps the label the approval gate needs. Inherit-only: no origin
    // in scope means the worker stays unlabelled and fails closed as before.
    let inherited_origin = crate::security::turn_origin::capture();
    ctx.spawn(async move {
        let owner_result = crate::security::turn_origin::with_inherited_origin(
            inherited_origin,
            service_task.start_member_with_started(
                &team,
                &task_id,
                &mem,
                &executor,
                false,
                |started| {
                    if let Some(sender) = started_tx.take() {
                        let _ = sender.send(Ok(started));
                    }
                },
            ),
        )
        .await;
        if let Err(error) = owner_result {
            if let Some(sender) = started_tx.take() {
                let _ = sender.send(Err(error));
            } else {
                log::error!(target: LOG_TARGET, "[agent_team_runtime] owner execution failed team={team} member={mem} task={task_id}: {error:#}");
                record_failure_event(&task_config, &team, &mem, &task_id, &error.to_string()).await;
            }
        }
    });

    let started = match started_rx.await {
        Ok(Ok(started)) => started,
        Ok(Err(error)) => {
            if let Some(error) = error.downcast_ref::<TeamMemberStartError>() {
                return Ok(match error {
                    TeamMemberStartError::AlreadyClaimed { .. } => {
                        StartMemberOutcome::AlreadyClaimed
                    }
                    TeamMemberStartError::AlreadyActive { .. } => StartMemberOutcome::AlreadyActive,
                    TeamMemberStartError::Blocked { unmet, .. } => StartMemberOutcome::Blocked {
                        unmet: unmet.clone(),
                    },
                    TeamMemberStartError::UnknownTask { .. } => StartMemberOutcome::UnknownTask,
                    TeamMemberStartError::Other(_) => return Err(anyhow!("{error}")),
                });
            }
            return Err(error);
        }
        Err(_) => return Err(anyhow!("team member start task exited before admission")),
    };
    let run_id = started.run_id;
    let claimed = started.task;
    log::debug!(target: LOG_TARGET, "[agent_team_runtime] start.spawned team={team_id} member={member_id} task={} run={run_id}", claimed.id);
    Ok(StartMemberOutcome::Started {
        run_id,
        task: Box::new(claimed),
    })
}

/// Owner-provided executor: build the root parent context, then run the
/// member's OpenHuman worker inside it. Returned errors let TinyAgents perform
/// token-scoped release and idle cleanup.
struct HostTeamMemberExecutor {
    ctx: crate::core::runtime::DispatchCtx,
    config: Config,
    team_id: String,
    member_id: String,
    model_override: Option<String>,
    parent_override: Option<ParentExecutionContext>,
}

#[async_trait::async_trait]
impl TeamMemberExecutor for HostTeamMemberExecutor {
    async fn execute(&self, member: &AgentTeamMember, task: &AgentTeamTask) -> Result<Vec<String>> {
        let agent_id = member
            .agent_id
            .as_deref()
            .unwrap_or(DEFAULT_TEAMMATE_AGENT_ID);
        let run_id = member
            .run_id
            .as_deref()
            .ok_or_else(|| anyhow!("admitted team member has no run id"))?;
        let worker_thread_id = member
            .worker_thread_id
            .as_deref()
            .ok_or_else(|| anyhow!("admitted team member has no worker thread id"))?;
        run_member_loop(
            &self.ctx,
            &self.config,
            &self.team_id,
            &self.member_id,
            agent_id,
            task.clone(),
            run_id,
            worker_thread_id,
            self.model_override.clone(),
            self.parent_override.clone(),
        )
        .await
    }
}

async fn run_member_loop(
    ctx: &crate::core::runtime::DispatchCtx,
    config: &Config,
    team_id: &str,
    member_id: &str,
    agent_id: &str,
    task: AgentTeamTask,
    run_id: &str,
    worker_thread_id: &str,
    model_override: Option<String>,
    parent_override: Option<ParentExecutionContext>,
) -> Result<Vec<String>> {
    // The member acts on memory as itself within the team: its node is
    // `team:<team>/agent:<agent>`, so members share the team's node and
    // never read each other's.
    // The member acts on memory as itself, in its team: it rides the parent's
    // host environment, so every run started from that parent keeps it.
    let identity = tinymemory_tools::layout::identity::Identity::team_member(team_id, agent_id);
    let member_turn = with_root_parent(
        ctx,
        parent_override,
        config,
        "agent_team_runtime",
        "team",
        "teamrun",
        |mut parent| async {
            parent.host_env.memory_identity = Some(identity);
            drive_member(
                config,
                team_id,
                member_id,
                agent_id,
                &task,
                run_id,
                worker_thread_id,
                model_override,
                parent,
            )
            .await
        },
    );
    Box::pin(member_turn)
        .await
        // Flatten: outer Err = root-parent build failure, inner = drive_member result.
        .unwrap_or_else(Err)
}

/// Spawn the actual OpenHuman worker sub-agent and return its evidence to the
/// TinyAgents team owner. Claiming, completion, and failed-run cleanup stay in
/// `TeamService::start_member_with_started`.
async fn drive_member(
    config: &Config,
    team_id: &str,
    member_id: &str,
    agent_id: &str,
    task: &AgentTeamTask,
    run_id: &str,
    worker_thread_id: &str,
    model_override: Option<String>,
    parent: ParentExecutionContext,
) -> Result<Vec<String>> {
    let ledger = SessionTeamLedger::new(
        crate::storage::sessions::require_ledger(&config.workspace_dir)
            .map_err(|e| anyhow!("{e}"))?,
    )?;
    let delivered = deliver_pending_messages(&ledger, team_id, member_id)?;
    let prompt = build_member_prompt(task, &delivered.messages);

    let session = AgentOrchestrationSession::new(format!("team-{team_id}-{member_id}"));

    let resp = session
        .spawn_agent(
            parent,
            SpawnAgentRequest {
                agent_id: agent_id.to_string(),
                prompt,
                model: model_override,
                metadata: [
                    ("teamId".to_string(), team_id.to_string()),
                    ("memberId".to_string(), member_id.to_string()),
                    ("taskId".to_string(), task.id.clone()),
                    ("teamRunId".to_string(), run_id.to_string()),
                    (
                        "teamWorkerThreadId".to_string(),
                        worker_thread_id.to_string(),
                    ),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| anyhow!("spawn teammate worker failed: {e}"))?;

    let wait = session
        .wait_agents(WaitAgentOptions {
            orchestration_ids: vec![resp.orchestration_id.clone()],
            timeout_ms: None,
        })
        .await
        .map_err(|e| anyhow!("wait teammate worker failed: {e}"))?;

    let snapshot = wait
        .agents
        .into_iter()
        .find(|a| a.orchestration_id == resp.orchestration_id)
        .ok_or_else(|| anyhow!("worker snapshot missing after wait"))?;
    if snapshot.status != OrchestrationTaskStatus::Completed {
        return Err(anyhow!(
            "worker ended with status {:?}: {}",
            snapshot.status,
            snapshot
                .error
                .unwrap_or_else(|| "worker ended without completing".to_string())
        ));
    }
    let output = snapshot.result_summary.unwrap_or_default();
    Ok(if output.trim().is_empty() {
        Vec::new()
    } else {
        vec![format!(
            "run:{run_id} — {}",
            truncate_chars(output.trim(), EVIDENCE_MAX_CHARS)
        )]
    })
}

async fn record_failure_event(
    config: &Config,
    team_id: &str,
    member_id: &str,
    task_id: &str,
    reason: &str,
) {
    let _ = run_ledger::append_run_event(
        &config.workspace_dir,
        RunEventAppend {
            run_id: team_id.to_string(),
            event_type: MEMBER_FAILED_EVENT.to_string(),
            payload: json!({
                "memberId": member_id,
                "taskId": task_id,
                "reason": truncate_chars(reason, EVIDENCE_MAX_CHARS),
            }),
        },
    )
    .await;
}

/// Host-owned team-ledger reads and failure-event projection. Claiming,
/// active-state transitions, completion and cleanup are owned by TinyAgents'
/// `TeamService`.
mod run_ledger {
    use std::path::Path;

    use anyhow::{anyhow, Result};

    use super::{AgentTeamMember, AgentTeamTask, RunEventAppend};

    fn ledger(
        workspace_dir: &Path,
    ) -> Result<std::sync::Arc<dyn tinyagents_session::run_ledger::RunLedgerStore>> {
        crate::storage::sessions::require_ledger(workspace_dir).map_err(|e| anyhow!("{e}"))
    }

    pub(super) async fn get_agent_team_member(
        workspace_dir: &Path,
        member_id: &str,
    ) -> Result<Option<AgentTeamMember>> {
        ledger(workspace_dir)?
            .get_agent_team_member(member_id)
            .await
            .map_err(|e| anyhow!("{e}"))
    }

    pub(super) async fn list_agent_team_tasks(
        workspace_dir: &Path,
        team_id: &str,
    ) -> Result<Vec<AgentTeamTask>> {
        ledger(workspace_dir)?
            .list_agent_team_tasks(team_id)
            .await
            .map_err(|e| anyhow!("{e}"))
    }

    pub(super) async fn append_run_event(
        workspace_dir: &Path,
        event: RunEventAppend,
    ) -> Result<()> {
        ledger(workspace_dir)?
            .append_run_event(event)
            .await
            .map(|_| ())
            .map_err(|e| anyhow!("{e}"))
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod runtime_tests;
