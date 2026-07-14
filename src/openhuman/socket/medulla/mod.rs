//! Medulla "harness plane" — binds the backend's `medulla:task_*` Socket.IO
//! protocol to an OpenHuman agent session so a medulla operator (running in the
//! backend) can drive an openhuman agent as a delegated sub-agent.
//!
//! This rides the *existing* authenticated backend socket owned by
//! [`crate::openhuman::socket::SocketManager`] — the transport, handshake auth
//! (`socket.handshake.auth.token`), and reconnection are already handled there,
//! so this module only adds the task/envelope binding on top:
//!
//! Down (backend → openhuman), handled in [`crate::openhuman::socket::event_handlers`]:
//! - `medulla:task_run`   → [`MedullaTaskManager::start_task`]
//! - `medulla:task_send`  → [`MedullaTaskManager::steer_task`]
//! - `medulla:task_abort` → [`MedullaTaskManager::abort_task`]
//!
//! Up (openhuman → backend):
//! - `medulla:task_envelope` — the live session stream, as
//!   `tinyplace.harness.session.v2` envelopes (see [`envelope`]).
//! - `medulla:task_result`   — explicit completion.
//! - `medulla:register_agents` — roster advertised on connect
//!   ([`emit_register_agents`]); the backend clears it on disconnect.

pub mod envelope;
pub mod payloads;

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use tokio::sync::{mpsc, Notify};
use tokio::time::Duration;

use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::agent::turn_origin::{with_origin, AgentTurnOrigin};
use crate::openhuman::agent::Agent;

use payloads::{
    AgentDescriptor, RegisterAgents, TaskResult, EVENT_REGISTER_AGENTS, EVENT_TASK_ENVELOPE,
    EVENT_TASK_RESULT,
};

/// Default agent an unspecified `medulla:task_run` runs as.
const DEFAULT_AGENT_ID: &str = "orchestrator";

/// How long we wait, after a turn settles, for a `medulla:task_send` follow-up
/// that arrived *during* the turn to be drained before declaring the task done.
/// Steering is inherently best-effort; this only catches input already queued.
const STEER_DRAIN_GRACE: Duration = Duration::from_millis(50);

// ─────────────────────────────────────────────────────────────────────────────
// Global manager
// ─────────────────────────────────────────────────────────────────────────────

static GLOBAL: OnceLock<Arc<MedullaTaskManager>> = OnceLock::new();

/// The process-wide medulla task manager (lazily created).
pub fn manager() -> &'static Arc<MedullaTaskManager> {
    GLOBAL.get_or_init(|| Arc::new(MedullaTaskManager::new()))
}

/// One in-flight task: a cooperative abort signal and a steering input channel.
struct RunningTask {
    /// Fired by `medulla:task_abort` to cancel the in-flight turn.
    abort: Arc<Notify>,
    /// Mid-task steering input (`medulla:task_send`) delivered as follow-up
    /// turns on the same agent session.
    steer_tx: mpsc::UnboundedSender<String>,
}

/// Tracks the openhuman side of every medulla-driven task.
pub struct MedullaTaskManager {
    tasks: Mutex<HashMap<String, RunningTask>>,
}

impl Default for MedullaTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MedullaTaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
        }
    }

    /// Handle `medulla:task_run`: register the task and spawn its driver.
    pub fn start_task(self: &Arc<Self>, run: payloads::TaskRun) {
        let task_id = run.task_id.clone();
        if self.tasks.lock().contains_key(&task_id) {
            log::warn!("[medulla] task_run for already-running task_id={task_id} — ignoring");
            return;
        }

        let abort = Arc::new(Notify::new());
        let (steer_tx, steer_rx) = mpsc::unbounded_channel::<String>();
        self.tasks.lock().insert(
            task_id.clone(),
            RunningTask {
                abort: Arc::clone(&abort),
                steer_tx,
            },
        );

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            manager.drive(run, abort, steer_rx).await;
        });
    }

    /// Handle `medulla:task_send`: deliver steering input into the session.
    pub fn steer_task(&self, send: payloads::TaskSend) {
        match self.tasks.lock().get(&send.task_id) {
            Some(task) => {
                if task.steer_tx.send(send.input).is_err() {
                    log::warn!(
                        "[medulla] task_send for task_id={} whose driver has exited",
                        send.task_id
                    );
                }
            }
            None => log::warn!(
                "[medulla] task_send for unknown task_id={} — dropping",
                send.task_id
            ),
        }
    }

    /// Handle `medulla:task_abort`: cancel the in-flight turn.
    pub fn abort_task(&self, abort: payloads::TaskAbort) {
        match self.tasks.lock().get(&abort.task_id) {
            Some(task) => {
                log::info!("[medulla] aborting task_id={}", abort.task_id);
                task.abort.notify_waiters();
            }
            None => log::warn!(
                "[medulla] task_abort for unknown task_id={} — dropping",
                abort.task_id
            ),
        }
    }

    /// Abort every in-flight task (used when the backend socket drops).
    pub fn abort_all(&self) {
        let tasks = self.tasks.lock();
        for (task_id, task) in tasks.iter() {
            log::debug!("[medulla] socket down — aborting task_id={task_id}");
            task.abort.notify_waiters();
        }
    }

    fn finish(&self, task_id: &str) {
        self.tasks.lock().remove(task_id);
    }

    /// Drive a task to completion: build/resume an agent session, run the
    /// instruction (plus any queued steering follow-ups) as turns, stream the
    /// progress as `medulla:task_envelope` frames, and emit a terminal
    /// `medulla:task_result`.
    async fn drive(
        &self,
        run: payloads::TaskRun,
        abort: Arc<Notify>,
        mut steer_rx: mpsc::UnboundedReceiver<String>,
    ) {
        let task_id = run.task_id.clone();
        // Session key: reuse the caller-supplied session id when resuming, else
        // fall back to the task id so the envelope stream is still anchored.
        let session_id = run.session_id.clone().unwrap_or_else(|| task_id.clone());
        let agent_id = run
            .agent_id
            .clone()
            .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string());
        let seq = Arc::new(AtomicI64::new(0));

        let mut agent = match build_agent(&agent_id, &task_id).await {
            Ok(agent) => agent,
            Err(err) => {
                log::error!("[medulla] task_id={task_id} failed to build agent: {err}");
                emit_envelope(
                    &task_id,
                    envelope::error_envelope(&session_id, next_seq(&seq), &err, true),
                );
                emit_result(TaskResult {
                    task_id: task_id.clone(),
                    ok: false,
                    reply: String::new(),
                    usage: None,
                    error: Some(err),
                });
                self.finish(&task_id);
                return;
            }
        };

        let deadline = (run.timeout_ms > 0).then(|| Duration::from_millis(run.timeout_ms));
        let mut next_input = run.instruction.clone();
        let result;

        'outer: loop {
            let (progress_tx, progress_rx) = mpsc::channel::<AgentProgress>(256);
            agent.set_on_progress(Some(progress_tx));
            let forwarder = spawn_forwarder(
                task_id.clone(),
                session_id.clone(),
                Arc::clone(&seq),
                progress_rx,
            );

            let origin = AgentTurnOrigin::ExternalChannel {
                channel: "medulla_harness".to_string(),
                sender: None,
                reply_target: task_id.clone(),
                message_id: uuid::Uuid::new_v4().to_string(),
            };
            let turn = Box::pin(with_origin(origin, agent.run_single(&next_input)));

            let turn_result = run_with_optional_timeout(deadline, &abort, turn).await;
            // The forwarder ends when `progress_tx` drops; make sure it's flushed.
            agent.set_on_progress(None);
            let _ = forwarder.await;

            match turn_result {
                TurnOutcome::Aborted => {
                    result = TaskResult {
                        task_id: task_id.clone(),
                        ok: false,
                        reply: String::new(),
                        usage: None,
                        error: Some("aborted".to_string()),
                    };
                    break 'outer;
                }
                TurnOutcome::TimedOut => {
                    emit_envelope(
                        &task_id,
                        envelope::error_envelope(
                            &session_id,
                            next_seq(&seq),
                            "task timed out",
                            true,
                        ),
                    );
                    result = TaskResult {
                        task_id: task_id.clone(),
                        ok: false,
                        reply: String::new(),
                        usage: None,
                        error: Some("timeout".to_string()),
                    };
                    break 'outer;
                }
                TurnOutcome::Errored(err) => {
                    emit_envelope(
                        &task_id,
                        envelope::error_envelope(&session_id, next_seq(&seq), &err, true),
                    );
                    result = TaskResult {
                        task_id: task_id.clone(),
                        ok: false,
                        reply: String::new(),
                        usage: None,
                        error: Some(err),
                    };
                    break 'outer;
                }
                TurnOutcome::Completed(reply) => {
                    // Drain any steering input that arrived during the turn and,
                    // if present, run it as a follow-up turn on the same session.
                    match drain_steer(&mut steer_rx).await {
                        Some(next) => {
                            next_input = next;
                            continue 'outer;
                        }
                        None => {
                            let usage = agent.take_last_turn_usage_totals().map(usage_to_json);
                            result = TaskResult {
                                task_id: task_id.clone(),
                                ok: true,
                                reply,
                                usage,
                                error: None,
                            };
                            break 'outer;
                        }
                    }
                }
            }
        }

        emit_result(result);
        self.finish(&task_id);
    }
}

/// Outcome of a single driven turn.
enum TurnOutcome {
    Completed(String),
    Errored(String),
    Aborted,
    TimedOut,
}

/// Race the agent turn against the cooperative abort signal and an optional
/// wall-clock deadline.
async fn run_with_optional_timeout(
    deadline: Option<Duration>,
    abort: &Arc<Notify>,
    turn: std::pin::Pin<Box<impl std::future::Future<Output = anyhow::Result<String>>>>,
) -> TurnOutcome {
    let run = async {
        tokio::select! {
            biased;
            _ = abort.notified() => TurnOutcome::Aborted,
            res = turn => match res {
                Ok(reply) => TurnOutcome::Completed(reply),
                Err(err) => TurnOutcome::Errored(err.to_string()),
            },
        }
    };

    match deadline {
        Some(d) => match tokio::time::timeout(d, run).await {
            Ok(outcome) => outcome,
            Err(_) => TurnOutcome::TimedOut,
        },
        None => run.await,
    }
}

/// Return queued steering input (if any) after briefly waiting for input that
/// was sent while the turn was still in flight.
async fn drain_steer(steer_rx: &mut mpsc::UnboundedReceiver<String>) -> Option<String> {
    if let Ok(msg) = steer_rx.try_recv() {
        return Some(msg);
    }
    match tokio::time::timeout(STEER_DRAIN_GRACE, steer_rx.recv()).await {
        Ok(msg) => msg,
        Err(_) => None,
    }
}

/// Build (or resume) an agent session for a medulla task.
async fn build_agent(agent_id: &str, task_id: &str) -> Result<Agent, String> {
    let config = crate::openhuman::config::rpc::load_config_with_timeout().await?;
    crate::openhuman::agent::harness::AgentDefinitionRegistry::init_global(&config.workspace_dir)
        .map_err(|err| format!("failed to init agent definition registry: {err}"))?;
    let mut agent = Agent::from_config_for_agent(&config, agent_id)
        .map_err(|err| format!("failed to build agent `{agent_id}`: {err}"))?;
    agent.set_event_context(format!("medulla:{task_id}"), "medulla_harness");
    agent.fetch_connected_integrations().await;
    let _ = agent.refresh_delegation_tools();
    Ok(agent)
}

/// Spawn the per-turn progress → `medulla:task_envelope` forwarder. Returns its
/// join handle so the driver can flush it before the next turn.
fn spawn_forwarder(
    task_id: String,
    session_id: String,
    seq: Arc<AtomicI64>,
    mut progress_rx: mpsc::Receiver<AgentProgress>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(progress) = progress_rx.recv().await {
            if let Some(kind) = envelope::progress_to_event_kind(&progress) {
                let env = envelope::envelope_for_kind(&session_id, next_seq(&seq), &kind);
                emit_envelope(&task_id, env);
            }
        }
    })
}

fn next_seq(seq: &AtomicI64) -> i64 {
    seq.fetch_add(1, Ordering::Relaxed)
}

/// Project the crate-private [`LastTurnUsage`] into a compact JSON usage block
/// for `medulla:task_result` (the type itself isn't `Serialize`).
fn usage_to_json(
    usage: crate::openhuman::agent::harness::turn_subagent_usage::LastTurnUsage,
) -> serde_json::Value {
    serde_json::json!({
        "inputTokens": usage.input_tokens,
        "outputTokens": usage.output_tokens,
        "cachedInputTokens": usage.cached_input_tokens,
        "costUsd": usage.cost_usd,
        "contextWindow": usage.context_window,
    })
}

/// Emit a `medulla:task_envelope` frame up the backend socket.
fn emit_envelope(task_id: &str, env: tinyplace::types::SessionEnvelopeV2) {
    let envelope = match serde_json::to_value(&env) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("[medulla] failed to serialize envelope for task_id={task_id}: {err}");
            return;
        }
    };
    let frame = payloads::TaskEnvelope {
        task_id: task_id.to_string(),
        envelope,
    };
    emit(EVENT_TASK_ENVELOPE, frame);
}

/// Emit a terminal `medulla:task_result`.
fn emit_result(result: TaskResult) {
    emit(EVENT_TASK_RESULT, result);
}

/// Emit `medulla:register_agents` — the roster advertised on (re)connect.
///
/// Built from the shipped default agent definitions. The backend clears the
/// roster on socket disconnect.
pub fn emit_register_agents() {
    let agents: Vec<AgentDescriptor> = crate::openhuman::agent_registry::default_agents()
        .into_iter()
        .map(|entry| AgentDescriptor {
            agent_id: entry.id,
            name: entry.name,
            description: entry.description,
        })
        .collect();
    log::info!("[medulla] advertising {} agents to backend", agents.len());
    emit(EVENT_REGISTER_AGENTS, RegisterAgents { agents });
}

/// Serialize `payload` and emit it as a Socket.IO event on the global backend
/// socket. Best-effort: a missing/disconnected socket is logged, not fatal.
fn emit<T: serde::Serialize>(event: &str, payload: T) {
    let data = match serde_json::to_value(&payload) {
        Ok(v) => v,
        Err(err) => {
            log::warn!("[medulla] failed to serialize payload for {event}: {err}");
            return;
        }
    };
    let Some(mgr) = crate::openhuman::socket::global_socket_manager() else {
        log::debug!("[medulla] no socket manager — dropping {event}");
        return;
    };
    let mgr = Arc::clone(mgr);
    let event = event.to_string();
    tokio::spawn(async move {
        if let Err(err) = mgr.emit(&event, data).await {
            log::warn!("[medulla] failed to emit {event}: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_task_send_and_abort_are_noops() {
        let mgr = MedullaTaskManager::new();
        // Neither should panic when the task id is unknown.
        mgr.steer_task(payloads::TaskSend {
            task_id: "nope".into(),
            input: "hi".into(),
        });
        mgr.abort_task(payloads::TaskAbort {
            task_id: "nope".into(),
        });
    }

    #[test]
    fn duplicate_task_registration_is_rejected() {
        let mgr = Arc::new(MedullaTaskManager::new());
        // Manually seed a running task to simulate an in-flight run, then prove
        // a second registration under the same id is ignored.
        let abort = Arc::new(Notify::new());
        let (steer_tx, _rx) = mpsc::unbounded_channel();
        mgr.tasks
            .lock()
            .insert("dup".to_string(), RunningTask { abort, steer_tx });
        assert!(mgr.tasks.lock().contains_key("dup"));
        // A second start_task for "dup" must not overwrite / spawn.
        mgr.start_task(payloads::TaskRun {
            task_id: "dup".into(),
            cycle_id: "c".into(),
            session_id: None,
            instruction: "x".into(),
            agent_id: None,
            timeout_ms: 0,
        });
        assert_eq!(mgr.tasks.lock().len(), 1);
    }
}
