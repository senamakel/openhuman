//! Orchestration wake-graph invocation (stage 4).
//!
//! This is the one thing that lives *outside* the graph on the transport side:
//! DMs arrive asynchronously, the stage-3 ingest subscriber persists them and
//! then asks us to wake the graph for that session. We:
//!
//! 1. **debounce** per session so a burst of DMs produces one graph run,
//! 2. **guard idempotence** via a per-session cursor so a re-trigger with no new
//!    messages does no LLM work and sends no DM,
//! 3. **seed** [`OrchestrationState`] from the stage-3 store (windowed messages +
//!    the counterpart to reply to), and
//! 4. drive [`run_orchestration_graph`] with the production nodes: the front-end
//!    agent (`hint:chat`), a stubbed reasoning core, and the Signal DM sender.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use serde_json::{Map, Value};

use crate::openhuman::config::Config;

use super::graph::{
    run_orchestration_graph, ChannelSender, FrontendNode, OrchestrationState, ReasoningNode,
};
use super::store;
use super::types::ChatKind;

const LOG: &str = "orchestration";

/// The per-session idempotence cursor key: the highest message seq that has been
/// carried through a completed wake cycle.
fn cursor_key(agent_id: &str, session_id: &str) -> String {
    format!("cursor:{agent_id}:{session_id}")
}

/// Per-session debounce generation counter. Each trigger bumps its session's
/// generation; the delayed task only proceeds if it is still the latest.
fn wake_generations() -> &'static Mutex<HashMap<String, u64>> {
    static GENS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    GENS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Bump the generation for `key` and return the new value.
fn bump_generation(key: &str) -> u64 {
    let mut map = wake_generations().lock().unwrap();
    let gen = map.entry(key.to_string()).or_insert(0);
    *gen += 1;
    *gen
}

/// True if `gen` is still the latest recorded generation for `key`.
fn is_latest_generation(key: &str, gen: u64) -> bool {
    wake_generations()
        .lock()
        .unwrap()
        .get(key)
        .is_some_and(|latest| *latest == gen)
}

/// Debounced entry point called by the stage-3 ingest subscriber on
/// `OrchestrationSessionMessage`. Coalesces a DM burst for one session into a
/// single graph run: the last trigger within `debounce_ms` wins.
pub async fn schedule_wake(agent_id: String, session_id: String, chat_kind: String) {
    let config = match Config::load_or_init().await {
        Ok(c) => c,
        Err(e) => {
            log::warn!(target: LOG, "[orchestration] wake.config_load_failed: {e}");
            return;
        }
    };
    if !config.orchestration.enabled {
        return;
    }
    // The subconscious window is not a wake trigger — it feeds steering (stage 6),
    // not the front-end channel loop.
    if ChatKind::from_str(&chat_kind) == ChatKind::Subconscious {
        return;
    }

    let key = format!("{agent_id}:{session_id}");
    let gen = bump_generation(&key);
    let debounce = config.orchestration.debounce_ms;
    log::debug!(
        target: LOG,
        "[orchestration] wake.scheduled agent={agent_id} session={session_id} gen={gen} debounce_ms={debounce}",
    );

    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(debounce)).await;
        if !is_latest_generation(&key, gen) {
            log::debug!(target: LOG, "[orchestration] wake.coalesced key={key} gen={gen}");
            return;
        }
        if let Err(e) = invoke_orchestration_graph(&config, &agent_id, &session_id).await {
            log::warn!(target: LOG, "[orchestration] wake.run_failed session={session_id}: {e}");
        }
    });
}

/// Seed a wake-cycle [`OrchestrationState`] from the store: the counterpart to
/// reply to plus the recent-message window. Returns `None` when the session has
/// no persisted messages (nothing to wake for).
pub fn seed_state(
    config: &Config,
    agent_id: &str,
    session_id: &str,
) -> Result<Option<OrchestrationState>, String> {
    let window = config.orchestration.message_window;
    store::with_connection(&config.workspace_dir, |conn| {
        let messages = store::list_recent_messages(conn, agent_id, session_id, window)?;
        if messages.is_empty() {
            return Ok(None);
        }
        Ok(Some(OrchestrationState::seed(
            session_id.to_string(),
            agent_id.to_string(),
            messages,
        )))
    })
    .map_err(|e| format!("seed_state: {e}"))
}

/// The highest message seq currently persisted for the session.
fn latest_seq(state: &OrchestrationState) -> i64 {
    state.messages.iter().map(|m| m.seq).max().unwrap_or(0)
}

/// Idempotence guard: has anything newer than the recorded cursor arrived?
fn has_new_work(config: &Config, agent_id: &str, session_id: &str, latest: i64) -> bool {
    let key = cursor_key(agent_id, session_id);
    let cursor = store::with_connection(&config.workspace_dir, |conn| store::kv_get(conn, &key))
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(i64::MIN);
    latest > cursor
}

/// Advance the idempotence cursor after a completed cycle.
fn advance_cursor(config: &Config, agent_id: &str, session_id: &str, latest: i64) {
    let key = cursor_key(agent_id, session_id);
    if let Err(e) = store::with_connection(&config.workspace_dir, |conn| {
        store::kv_set(conn, &key, &latest.to_string())
    }) {
        log::warn!(target: LOG, "[orchestration] cursor.advance_failed session={session_id}: {e}");
    }
}

/// Build the production node set and drive one wake cycle. Skips (no LLM, no DM)
/// when the idempotence cursor shows no new messages since the last cycle.
pub async fn invoke_orchestration_graph(
    config: &Config,
    agent_id: &str,
    session_id: &str,
) -> Result<(), String> {
    let Some(state) = seed_state(config, agent_id, session_id)? else {
        log::debug!(target: LOG, "[orchestration] wake.skip_empty session={session_id}");
        return Ok(());
    };
    let latest = latest_seq(&state);
    if !has_new_work(config, agent_id, session_id, latest) {
        log::debug!(
            target: LOG,
            "[orchestration] wake.skip_idempotent session={session_id} latest_seq={latest}",
        );
        return Ok(());
    }

    let config = Arc::new(config.clone());
    let frontend: Arc<dyn FrontendNode> = Arc::new(AgentFrontendRunner {
        config: config.clone(),
        session_id: session_id.to_string(),
    });
    let reasoning: Arc<dyn ReasoningNode> = Arc::new(StubReasoningCore);
    let sender: Arc<dyn ChannelSender> = Arc::new(SignalDmSender);

    let out = run_orchestration_graph(config.clone(), frontend, reasoning, sender, state)
        .await
        .map_err(|e| format!("graph run: {e}"))?;

    if out.dm_sent {
        advance_cursor(&config, agent_id, session_id, latest);
    }
    Ok(())
}

// ── Production nodes ────────────────────────────────────────────────────────

/// Render the windowed transcript for the front-end prompt. Roles are the
/// harness roles (`user` / `agent`); the front end reads them like a chat log.
fn render_transcript(state: &OrchestrationState) -> String {
    let mut out = String::with_capacity(1024);
    for m in &state.messages {
        out.push_str(&format!("[{}] {}\n", m.role, m.body));
    }
    if let Some(steer) = &state.subconscious_steering {
        out.push_str(&format!("\n[subconscious steering]: {steer}\n"));
    }
    out
}

/// Production front end: runs the `frontend_agent` built-in for one turn on the
/// Quick (`hint:chat`) tier. Pass 1 frames macro-instructions; pass 2 compiles
/// the reasoning reply into the finished channel text.
struct AgentFrontendRunner {
    config: Arc<Config>,
    session_id: String,
}

impl AgentFrontendRunner {
    async fn run_turn(&self, user_message: String) -> anyhow::Result<String> {
        use crate::openhuman::agent::turn_origin::{
            with_origin, AgentTurnOrigin, TrustedAutomationSource,
        };
        use crate::openhuman::agent::Agent;

        // Force the Quick tier — verified `hint:chat` (TTFT-optimized, remote).
        let mut effective = (*self.config).clone();
        effective.default_model = Some("hint:chat".to_string());

        let mut agent = Agent::from_config_for_agent(&effective, "frontend_agent")
            .map_err(|e| anyhow::anyhow!("frontend agent init: {e}"))?;
        agent.set_event_context(
            format!("orchestration:frontend:{}", self.session_id),
            "orchestration",
        );

        // Background origin: no interactive approval parking (stage-4 gating).
        let origin = AgentTurnOrigin::TrustedAutomation {
            job_id: format!("orchestration:frontend:{}", self.session_id),
            source: TrustedAutomationSource::Cron,
        };
        with_origin(origin, agent.run_single(&user_message))
            .await
            .map_err(|e| anyhow::anyhow!("frontend agent run: {e}"))
    }
}

#[async_trait]
impl FrontendNode for AgentFrontendRunner {
    async fn instruct(&self, state: &OrchestrationState) -> anyhow::Result<String> {
        let prompt = format!(
            "Session transcript:\n\n{}\n\n## Pass 1\n\nTriage this. If a complete answer is \
             obvious, call `reply_to_channel`. Otherwise call `defer_to_orchestrator` with concise \
             macro-instructions for the reasoning core.",
            render_transcript(state),
        );
        self.run_turn(prompt).await
    }

    async fn compile_reply(&self, state: &OrchestrationState) -> anyhow::Result<String> {
        let reply = state.agent_reply.clone().unwrap_or_default();
        let prompt = format!(
            "Session transcript:\n\n{}\n\n## Pass 2\n\nThe reasoning core produced this result:\n\n\
             {}\n\nCompile it into the finished message to send back to the session, then call \
             `reply_to_channel` with that text.",
            render_transcript(state),
            reply,
        );
        self.run_turn(prompt).await
    }
}

/// Stubbed reasoning core (stage 4). Replaced by the real sub-agent-spawning
/// `execute` node in stage 5.
struct StubReasoningCore;

#[async_trait]
impl ReasoningNode for StubReasoningCore {
    async fn execute(&self, state: &OrchestrationState) -> anyhow::Result<String> {
        let instructions = state.agent_instructions.as_deref().unwrap_or("(none)");
        Ok(format!(
            "[stubbed reasoning core] acknowledged instructions: {instructions}"
        ))
    }
}

/// Production DM sender: the finished `channel_response` back over the tiny.place
/// Signal channel, reusing the same reply seam the messaging UI uses.
struct SignalDmSender;

#[async_trait]
impl ChannelSender for SignalDmSender {
    async fn send_dm(&self, counterpart_agent_id: &str, body: &str) -> anyhow::Result<()> {
        let mut params = Map::new();
        params.insert("recipient".to_string(), Value::from(counterpart_agent_id));
        params.insert("plaintext".to_string(), Value::from(body));
        crate::openhuman::tinyplace::handle_tinyplace_signal_send_message(params)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("signal send: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::orchestration::types::OrchestrationMessage;
    use crate::openhuman::tinyagents::SqlRunLedgerCheckpointer;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tinyagents::graph::checkpoint::Checkpointer;

    fn test_config(tmp: &tempfile::TempDir) -> Config {
        Config {
            workspace_dir: tmp.path().to_path_buf(),
            ..Config::default()
        }
    }

    fn msg(session: &str, seq: i64) -> OrchestrationMessage {
        OrchestrationMessage {
            id: format!("m{seq}"),
            agent_id: "@peer".into(),
            session_id: session.into(),
            chat_kind: ChatKind::Session,
            role: "user".into(),
            body: "hello".into(),
            timestamp: format!("2026-07-02T00:00:{seq:02}Z"),
            seq,
        }
    }

    #[test]
    fn cursor_gates_reprocessing() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(&tmp);
        // No cursor yet → any message is new work.
        assert!(has_new_work(&config, "@peer", "h1", 3));
        advance_cursor(&config, "@peer", "h1", 3);
        // Nothing newer than seq 3 → no work (idempotent re-trigger).
        assert!(!has_new_work(&config, "@peer", "h1", 3));
        // A newer message reopens work.
        assert!(has_new_work(&config, "@peer", "h1", 4));
    }

    #[test]
    fn debounce_generation_coalesces_bursts() {
        let key = "@peer:burst-session";
        let g1 = bump_generation(key);
        let g2 = bump_generation(key);
        let g3 = bump_generation(key);
        assert!(g2 > g1 && g3 > g2);
        // Only the latest trigger survives the debounce window.
        assert!(!is_latest_generation(key, g1));
        assert!(!is_latest_generation(key, g2));
        assert!(is_latest_generation(key, g3));
    }

    #[test]
    fn seed_state_windows_messages_and_skips_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let config = test_config(&tmp);
        // Empty session → nothing to wake for.
        assert!(seed_state(&config, "@peer", "h1").unwrap().is_none());

        // Persist two messages, then seed reads them in order.
        store::with_connection(&config.workspace_dir, |conn| {
            store::insert_message(conn, &msg("h1", 1))?;
            store::insert_message(conn, &msg("h1", 2))?;
            Ok(())
        })
        .unwrap();
        let state = seed_state(&config, "@peer", "h1").unwrap().expect("seeded");
        assert_eq!(state.session_id, "h1");
        assert_eq!(state.counterpart_agent_id, "@peer");
        assert_eq!(state.messages.len(), 2);
        assert_eq!(latest_seq(&state), 2);
    }

    // Stub nodes for the integration run (no LLM, no real Signal).
    struct StubFe;
    #[async_trait]
    impl FrontendNode for StubFe {
        async fn instruct(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok("instructions".into())
        }
        async fn compile_reply(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok("compiled reply".into())
        }
    }
    struct StubReasoning;
    #[async_trait]
    impl ReasoningNode for StubReasoning {
        async fn execute(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok("reasoning reply".into())
        }
    }
    struct CountingSender(Arc<AtomicUsize>);
    #[async_trait]
    impl ChannelSender for CountingSender {
        async fn send_dm(&self, _c: &str, _b: &str) -> anyhow::Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn graph_run_persists_checkpoints_and_sends_one_dm() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Arc::new(test_config(&tmp));
        let sends = Arc::new(AtomicUsize::new(0));

        let state = OrchestrationState::seed("h1", "@peer", vec![msg("h1", 1)]);
        let out = run_orchestration_graph(
            config.clone(),
            Arc::new(StubFe),
            Arc::new(StubReasoning),
            Arc::new(CountingSender(sends.clone())),
            state,
        )
        .await
        .expect("graph runs");

        assert!(out.dm_sent, "cycle latches dm_sent");
        assert_eq!(sends.load(Ordering::SeqCst), 1, "exactly one DM");
        assert_eq!(out.channel_response.as_deref(), Some("compiled reply"));

        // Checkpoints were persisted for the thread — kill/restart could resume.
        let cp = SqlRunLedgerCheckpointer::<OrchestrationState>::new(config);
        let list = cp.list("orchestration:h1").await.expect("list checkpoints");
        assert!(!list.is_empty(), "wake cycle persisted checkpoints");
    }
}
