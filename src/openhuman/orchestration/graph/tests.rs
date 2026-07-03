//! Graph-mechanics tests: full-cycle walk (exactly one DM) and the
//! loop-continuity property (adversarial state combos never cycle or double-send).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use super::*;

/// Records front-end / reasoning calls and every DM sent, so tests can assert
/// call counts and single-send.
#[derive(Default)]
struct Recorder {
    instruct_calls: AtomicUsize,
    compile_calls: AtomicUsize,
    execute_calls: AtomicUsize,
    dms: Mutex<Vec<(String, String)>>,
}

struct StubFrontend(Arc<Recorder>);
#[async_trait]
impl FrontendNode for StubFrontend {
    async fn instruct(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
        self.0.instruct_calls.fetch_add(1, Ordering::SeqCst);
        Ok("do the thing".into())
    }
    async fn compile_reply(&self, s: &OrchestrationState) -> anyhow::Result<String> {
        self.0.compile_calls.fetch_add(1, Ordering::SeqCst);
        Ok(format!("reply: {}", s.agent_reply.clone().unwrap_or_default()))
    }
}

struct StubReasoning(Arc<Recorder>);
#[async_trait]
impl ReasoningNode for StubReasoning {
    async fn execute(&self, _s: &OrchestrationState) -> anyhow::Result<String> {
        self.0.execute_calls.fetch_add(1, Ordering::SeqCst);
        Ok("canned reasoning reply".into())
    }
}

struct StubSender(Arc<Recorder>);
#[async_trait]
impl ChannelSender for StubSender {
    async fn send_dm(&self, counterpart: &str, body: &str) -> anyhow::Result<()> {
        self.0
            .dms
            .lock()
            .unwrap()
            .push((counterpart.to_string(), body.to_string()));
        Ok(())
    }
}

fn run(state: OrchestrationState, rec: Arc<Recorder>) -> OrchestrationState {
    let graph = build_orchestration_graph(
        Arc::new(StubFrontend(rec.clone())),
        Arc::new(StubReasoning(rec.clone())),
        Arc::new(StubSender(rec.clone())),
        12,
    )
    .expect("graph compiles");
    // No thread id → no checkpoint persistence needed; exercises pure mechanics.
    let exec = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(graph.run(state))
        .expect("graph runs");
    exec.state
}

#[test]
fn full_cycle_walks_normalize_frontend_execute_frontend_send_guard_and_sends_one_dm() {
    let rec = Arc::new(Recorder::default());
    let state = OrchestrationState::seed("h1", "@peer", Vec::new());
    let out = run(state, rec.clone());

    // One pass-1 instruct, one reasoning execute, one pass-2 compile.
    assert_eq!(rec.instruct_calls.load(Ordering::SeqCst), 1, "one pass-1");
    assert_eq!(rec.execute_calls.load(Ordering::SeqCst), 1, "one execute");
    assert_eq!(rec.compile_calls.load(Ordering::SeqCst), 1, "one pass-2");

    // Exactly one outbound DM, to the right counterpart, carrying the compiled reply.
    let dms = rec.dms.lock().unwrap();
    assert_eq!(dms.len(), 1, "exactly one DM");
    assert_eq!(dms[0].0, "@peer");
    assert_eq!(dms[0].1, "reply: canned reasoning reply");

    // Terminal state: response compiled, latched sent, two front-end passes,
    // context utilization computed before END.
    assert_eq!(out.agent_instructions.as_deref(), Some("do the thing"));
    assert_eq!(out.agent_reply.as_deref(), Some("canned reasoning reply"));
    assert_eq!(out.channel_response.as_deref(), Some("reply: canned reasoning reply"));
    assert!(out.dm_sent);
    assert_eq!(out.pass, 2);
    assert!(out.context_utilization >= 0.0);
}

#[test]
fn loop_continuity_adversarial_state_combos_never_cycle_or_double_send() {
    // (label, seed mutation): every combination must terminate with ≤1 DM.
    let cases: Vec<(&str, Box<dyn Fn(&mut OrchestrationState)>)> = vec![
        ("cold_start", Box::new(|_s| {})),
        (
            "instructions_without_reply",
            Box::new(|s| s.agent_instructions = Some("stale".into())),
        ),
        (
            "reply_preset",
            Box::new(|s| s.agent_reply = Some("preset".into())),
        ),
        (
            "response_preset",
            Box::new(|s| s.channel_response = Some("already".into())),
        ),
        (
            "reply_and_response_preset",
            Box::new(|s| {
                s.agent_reply = Some("preset".into());
                s.channel_response = Some("already".into());
            }),
        ),
    ];

    for (label, mutate) in cases {
        let rec = Arc::new(Recorder::default());
        let mut state = OrchestrationState::seed("h1", "@peer", Vec::new());
        mutate(&mut state);
        let out = run(state, rec.clone());

        let dm_count = rec.dms.lock().unwrap().len();
        assert!(dm_count <= 1, "{label}: sent {dm_count} DMs — must never double-send");
        assert!(out.dm_sent, "{label}: cycle must reach the terminal send_dm latch");
        assert!(
            out.channel_response.is_some(),
            "{label}: cycle must terminate with a channel_response"
        );
        // Bounded front-end work: never more passes than the backstop allows.
        assert!(out.pass <= 12, "{label}: {} passes — exceeded backstop", out.pass);
        // A pre-set channel_response short-circuits the LLM entirely.
        if label == "response_preset" || label == "reply_and_response_preset" {
            assert_eq!(
                rec.instruct_calls.load(Ordering::SeqCst),
                0,
                "{label}: pre-set response must not call the front-end LLM"
            );
            assert_eq!(dm_count, 1, "{label}: still sends the pre-set response once");
        }
    }
}

#[test]
fn topology_is_structurally_valid() {
    let t = orchestration_graph_topology().expect("topology builds");
    assert!(t.validation.ok, "structural errors: {:?}", t.validation.errors);
    assert!(!t.nodes.is_empty());
}
