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

use super::graph::compress::{compression_budget, count_tokens, enforce_budget};
use super::graph::{
    run_orchestration_graph, world_diff, CompressedEntry, EvictionOutcome, ExecuteOutcome,
    OrchestrationRuntime, OrchestrationState, WorldDiffEntry,
};
use super::store;
use super::types::ChatKind;

/// Assumed model context window (tokens) for the `context_guard` utilization
/// estimate until per-model resolution is wired. Sized to the reasoning tier.
const ASSUMED_CONTEXT_WINDOW: u64 = 200_000;

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
    let runtime: Arc<dyn OrchestrationRuntime> = Arc::new(ProductionRuntime {
        config: config.clone(),
        agent_id: agent_id.to_string(),
        session_id: session_id.to_string(),
    });

    let out = run_orchestration_graph(config.clone(), runtime, state)
        .await
        .map_err(|e| format!("graph run: {e}"))?;

    if out.dm_sent {
        advance_cursor(&config, agent_id, session_id, latest);
    }
    Ok(())
}

// ── Production runtime ──────────────────────────────────────────────────────

/// Render the windowed transcript for a node prompt. Roles are the harness roles
/// (`user` / `agent`); the agents read them like a chat log.
fn render_transcript(state: &OrchestrationState) -> String {
    let mut out = String::with_capacity(1024);
    for m in &state.messages {
        out.push_str(&format!("[{}] {}\n", m.role, m.body));
    }
    out
}

/// The production wiring for every wake-graph node: the front-end + reasoning
/// agents, the compression summarizer, the world-diff + compressed-history store
/// writes, the memory-RAG eviction, and the Signal DM reply.
struct ProductionRuntime {
    config: Arc<Config>,
    agent_id: String,
    session_id: String,
}

impl ProductionRuntime {
    /// Run a built-in agent for one turn under a background origin, forcing the
    /// given model hint (`hint:chat` for the front end, `hint:reasoning` for the
    /// core). Returns the final assistant text.
    async fn run_agent_turn(
        &self,
        agent_id: &str,
        model_hint: &str,
        channel: &str,
        user_message: String,
    ) -> anyhow::Result<String> {
        use crate::openhuman::agent::turn_origin::{
            with_origin, AgentTurnOrigin, TrustedAutomationSource,
        };
        use crate::openhuman::agent::Agent;

        let mut effective = (*self.config).clone();
        effective.default_model = Some(model_hint.to_string());

        let mut agent = Agent::from_config_for_agent(&effective, agent_id)
            .map_err(|e| anyhow::anyhow!("{agent_id} init: {e}"))?;
        agent.set_event_context(
            format!("orchestration:{channel}:{}", self.session_id),
            "orchestration",
        );

        // Background origin: no interactive approval parking.
        let origin = AgentTurnOrigin::TrustedAutomation {
            job_id: format!("orchestration:{channel}:{}", self.session_id),
            source: TrustedAutomationSource::Cron,
        };
        with_origin(origin, agent.run_single(&user_message))
            .await
            .map_err(|e| anyhow::anyhow!("{agent_id} run: {e}"))
    }
}

#[async_trait]
impl OrchestrationRuntime for ProductionRuntime {
    async fn frontend_instruct(&self, state: &OrchestrationState) -> anyhow::Result<String> {
        let prompt = format!(
            "Session transcript:\n\n{}\n\n## Pass 1\n\nTriage this. If a complete answer is \
             obvious, call `reply_to_channel`. Otherwise call `defer_to_orchestrator` with concise \
             macro-instructions for the reasoning core.",
            render_transcript(state),
        );
        self.run_agent_turn("frontend_agent", "hint:chat", "frontend", prompt)
            .await
    }

    async fn frontend_compile(&self, state: &OrchestrationState) -> anyhow::Result<String> {
        let reply = state.agent_reply.clone().unwrap_or_default();
        let prompt = format!(
            "Session transcript:\n\n{}\n\n## Pass 2\n\nThe reasoning core produced this result:\n\n\
             {}\n\nCompile it into the finished message to send back to the session, then call \
             `reply_to_channel` with that text.",
            render_transcript(state),
            reply,
        );
        self.run_agent_turn("frontend_agent", "hint:chat", "frontend", prompt)
            .await
    }

    async fn execute(&self, state: &OrchestrationState) -> anyhow::Result<ExecuteOutcome> {
        let instructions = state.agent_instructions.as_deref().unwrap_or("(none)");
        let prompt = format!(
            "Macro-instructions from the front end:\n\n{instructions}\n\nSession transcript:\n\n{}\n\n\
             Do the work (delegating to worker sub-agents where appropriate) and return the result.",
            render_transcript(state),
        );
        // Scope the current steering directive so the reasoning agent's prompt
        // builder weaves it into the system prompt (spec §3.2).
        let steering = state.subconscious_steering.clone().unwrap_or_default();
        let reply = super::reasoning_agent::with_steering(
            steering,
            self.run_agent_turn("reasoning_agent", "hint:reasoning", "reasoning", prompt),
        )
        .await?;
        // The trace the compression node condenses. `run_single` surfaces the
        // final assistant text; the richer per-tool/sub-agent trace lands when
        // the lower-level runner is wired (follow-up). Frame it with the
        // instructions so the compressed record is self-describing.
        let trace = format!("Instructions: {instructions}\n\nResult:\n{reply}");
        Ok(ExecuteOutcome { reply, trace })
    }

    async fn compress(&self, state: &OrchestrationState) -> anyhow::Result<CompressedEntry> {
        let trace = &state.execution_trace;
        let input_tokens = count_tokens(trace);
        if input_tokens == 0 {
            return Ok(CompressedEntry::default());
        }
        let budget = compression_budget(input_tokens);

        // Summarize via a cheap tier, then enforce the 20:1 budget: retry once if
        // the summary exceeds 1.5× budget, then hard-truncate.
        let summarize_prompt = format!(
            "Compress the following execution trace into at most ~{budget} tokens. Keep only the \
             decisions, outcomes, and facts needed to continue. No preamble.\n\n{trace}",
        );
        let raw = self
            .run_agent_turn(
                "summarizer",
                "hint:burst",
                "compress",
                summarize_prompt.clone(),
            )
            .await
            .unwrap_or_else(|_| trace.clone());
        let (mut summary, mut truncated) = enforce_budget(&raw, budget);
        if truncated {
            if let Ok(retry) = self
                .run_agent_turn("summarizer", "hint:burst", "compress", summarize_prompt)
                .await
            {
                let (s2, t2) = enforce_budget(&retry, budget);
                summary = s2;
                truncated = t2;
            }
        }
        let output_tokens = count_tokens(&summary);
        let now = chrono::Utc::now().to_rfc3339();

        // Persist idempotently by cycle_id (a resumed cycle re-writes the same row).
        let cycle_id = state.cycle_id.clone();
        let session_id = state.session_id.clone();
        let agent_id = self.agent_id.clone();
        let text = summary.clone();
        if let Err(e) = store::with_connection(&self.config.workspace_dir, |conn| {
            store::insert_compressed(
                conn,
                &cycle_id,
                &session_id,
                &agent_id,
                input_tokens as i64,
                output_tokens as i64,
                &text,
                &now,
            )
        }) {
            log::warn!(target: LOG, "[orchestration] compress.persist_failed cycle={cycle_id}: {e}");
        }
        log::debug!(
            target: LOG,
            "[orchestration] compress cycle={} input={input_tokens} output={output_tokens} budget={budget} truncated={truncated}",
            state.cycle_id,
        );
        Ok(CompressedEntry {
            summary,
            covered_messages: state.messages.len() as u32,
        })
    }

    async fn world_diff(&self, state: &OrchestrationState) -> anyhow::Result<WorldDiffEntry> {
        let signature = world_diff::event_signature(state);
        let mutation = world_diff::world_mutation(state);
        let delta = world_diff::delta(state);
        let now = chrono::Utc::now().to_rfc3339();

        let cycle_id = state.cycle_id.clone();
        let session_id = state.session_id.clone();
        let agent_id = self.agent_id.clone();
        let seq = store::with_connection(&self.config.workspace_dir, |conn| {
            store::append_world_diff(
                conn,
                &cycle_id,
                &session_id,
                &agent_id,
                &signature,
                &mutation,
                &delta,
                &now,
            )
        })
        .map_err(|e| anyhow::anyhow!("world_diff persist: {e}"))?;

        Ok(WorldDiffEntry {
            seq: seq as u64,
            note: mutation,
        })
    }

    async fn context_utilization(&self, state: &OrchestrationState) -> anyhow::Result<f32> {
        // Estimate accumulated tokens: the message window + execution trace +
        // retained compressed-history summaries, over the assumed window.
        let mut tokens = count_tokens(&render_transcript(state));
        tokens += count_tokens(&state.execution_trace);
        for entry in &state.compressed_history {
            tokens += count_tokens(&entry.summary);
        }
        let util = (tokens as f32 / ASSUMED_CONTEXT_WINDOW as f32).min(1.0);
        Ok(util)
    }

    async fn evict(&self, state: &OrchestrationState) -> anyhow::Result<EvictionOutcome> {
        // Keep the most recent two compressed entries live; evict the older head
        // to memory RAG under a session-scoped path so it stays retrievable.
        let total = state.compressed_history.len();
        let keep = 2usize.min(total);
        let evict_count = total.saturating_sub(keep);
        let path_scope = format!("orchestration/{}", state.session_id);

        for (i, entry) in state
            .compressed_history
            .iter()
            .take(evict_count)
            .enumerate()
        {
            let doc = crate::openhuman::memory_sync::canonicalize::document::DocumentInput {
                provider: "orchestration".to_string(),
                title: format!("orchestration session {} — cycle summary", state.session_id),
                body: entry.summary.clone(),
                modified_at: chrono::Utc::now(),
                source_ref: None,
            };
            let source_id = format!("orchestration/{}/{}#{i}", state.session_id, state.cycle_id);
            if let Err(e) = crate::openhuman::memory::ingest_pipeline::ingest_document_with_scope(
                &self.config,
                &source_id,
                &self.agent_id,
                vec!["orchestration".to_string()],
                doc,
                Some(path_scope.clone()),
            )
            .await
            {
                log::warn!(target: LOG, "[orchestration] evict.memory_write_failed: {e}");
            }
        }

        // Utilization after dropping the evicted head from live state.
        let mut retained_tokens = count_tokens(&render_transcript(state));
        retained_tokens += count_tokens(&state.execution_trace);
        for entry in state.compressed_history.iter().skip(evict_count) {
            retained_tokens += count_tokens(&entry.summary);
        }
        let new_utilization = (retained_tokens as f32 / ASSUMED_CONTEXT_WINDOW as f32).min(1.0);
        log::debug!(
            target: LOG,
            "[orchestration] evict session={} evicted={evict_count} new_util={new_utilization}",
            state.session_id,
        );
        Ok(EvictionOutcome {
            evicted: evict_count,
            new_utilization,
        })
    }

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

    // A hermetic stub runtime for the integration run (no LLM, no real Signal,
    // no memory writes) that records DMs + world-diff/compress store rows.
    // (`CompressedEntry`, `ExecuteOutcome`, etc. are in scope via `use super::*`.)
    struct StubRuntime {
        config: Arc<Config>,
        agent_id: String,
        sends: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl OrchestrationRuntime for StubRuntime {
        async fn frontend_instruct(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok("instructions".into())
        }
        async fn frontend_compile(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
            Ok("compiled reply".into())
        }
        async fn execute(&self, _s: &OrchestrationState) -> anyhow::Result<ExecuteOutcome> {
            Ok(ExecuteOutcome {
                reply: "reasoning reply".into(),
                trace: "trace line one\ntrace line two".into(),
            })
        }
        async fn compress(&self, s: &OrchestrationState) -> anyhow::Result<CompressedEntry> {
            // Persist a real compressed row so the e2e can assert exactly one.
            store::with_connection(&self.config.workspace_dir, |conn| {
                store::insert_compressed(
                    conn,
                    &s.cycle_id,
                    &s.session_id,
                    &self.agent_id,
                    100,
                    5,
                    "compact",
                    "now",
                )
            })
            .ok();
            Ok(CompressedEntry {
                summary: "compact".into(),
                covered_messages: s.messages.len() as u32,
            })
        }
        async fn world_diff(&self, s: &OrchestrationState) -> anyhow::Result<WorldDiffEntry> {
            let seq = store::with_connection(&self.config.workspace_dir, |conn| {
                store::append_world_diff(
                    conn,
                    &s.cycle_id,
                    &s.session_id,
                    &self.agent_id,
                    "sig",
                    "mutation",
                    "delta",
                    "now",
                )
            })
            .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(WorldDiffEntry {
                seq: seq as u64,
                note: "mutation".into(),
            })
        }
        async fn context_utilization(&self, _s: &OrchestrationState) -> anyhow::Result<f32> {
            Ok(0.1)
        }
        async fn evict(&self, _s: &OrchestrationState) -> anyhow::Result<EvictionOutcome> {
            Ok(EvictionOutcome {
                evicted: 0,
                new_utilization: 0.1,
            })
        }
        async fn send_dm(&self, _c: &str, _b: &str) -> anyhow::Result<()> {
            self.sends.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn full_cycle_persists_one_dm_one_compressed_one_diff_and_checkpoints() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Arc::new(test_config(&tmp));
        let sends = Arc::new(AtomicUsize::new(0));

        let state = OrchestrationState::seed("h1", "@peer", vec![msg("h1", 1)]);
        let runtime = Arc::new(StubRuntime {
            config: config.clone(),
            agent_id: "@me".into(),
            sends: sends.clone(),
        });
        let out = run_orchestration_graph(config.clone(), runtime, state)
            .await
            .expect("graph runs");

        assert!(out.dm_sent, "cycle latches dm_sent");
        assert_eq!(sends.load(Ordering::SeqCst), 1, "exactly one DM");
        assert_eq!(out.channel_response.as_deref(), Some("compiled reply"));

        // Exactly one compressed row + one world-diff entry landed in the store.
        store::with_connection(&config.workspace_dir, |conn| {
            assert_eq!(store::count_compressed(conn, "@me", "h1")?, 1);
            assert_eq!(store::world_diff_seqs(conn, "@me", "h1")?, vec![1]);
            Ok(())
        })
        .unwrap();

        // Checkpoints persisted → kill/restart could resume without re-sending.
        let cp = SqlRunLedgerCheckpointer::<OrchestrationState>::new(config);
        let list = cp.list("orchestration:h1").await.expect("list checkpoints");
        assert!(!list.is_empty(), "wake cycle persisted checkpoints");
    }
}
