//! Integration test: owner-discovery agent runner.
//!
//! Verifies that `run_discovery` with stubbed LLM + Apify clients:
//!   1. Writes `Identity` + `Role` facets to the `user_profile` table.
//!   2. Writes a rich document to the `owner` namespace.
//!   3. Surfaces the Identity facets via `build_owner_section`.
//!
//! No network calls — both external dependencies are stubbed behind
//! the `LlmClient` / `ApifyClient` traits.
//!
//! Run:
//!   cargo test --test owner_discovery_test -- --nocapture

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use tempfile::tempdir;

use openhuman_core::openhuman::agent::discovery::apify::{ApifyError, ApifyRunResult};
use openhuman_core::openhuman::agent::discovery::{
    run_discovery, ApifyClient, DiscoveryDeps, DiscoveryJob, DiscoveryTrigger, LlmClient, LlmError,
    LlmMessage, LlmResponse, LlmToolCall, SeedFact,
};
use openhuman_core::openhuman::channels::build_owner_section;
use openhuman_core::openhuman::config::DiscoveryConfig;
use openhuman_core::openhuman::memory::MemoryClient;

// ── Stub LLM ────────────────────────────────────────────────────────────

/// An LLM stub that replays a fixed script of canned responses. Each
/// call to `chat` pops the next scripted reply. The test asserts on
/// the number of calls and the messages the runner built along the way.
struct ScriptedLlm {
    script: Mutex<Vec<LlmResponse>>,
    seen: Mutex<Vec<Vec<LlmMessage>>>,
}

impl ScriptedLlm {
    fn new(script: Vec<LlmResponse>) -> Self {
        Self {
            script: Mutex::new(script),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn call_count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn call_history(&self) -> Vec<Vec<LlmMessage>> {
        self.seen.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmClient for ScriptedLlm {
    async fn chat(
        &self,
        messages: &[LlmMessage],
        _tools: &[Value],
        _model: &str,
    ) -> Result<LlmResponse, LlmError> {
        self.seen.lock().unwrap().push(messages.to_vec());
        let next = self
            .script
            .lock()
            .unwrap()
            .drain(..1)
            .next()
            .ok_or_else(|| LlmError::Request("script exhausted".into()))?;
        Ok(next)
    }
}

// ── Stub Apify ──────────────────────────────────────────────────────────

/// An Apify stub that returns canned dataset items keyed by actor id.
struct StubApify {
    items: std::collections::HashMap<String, Vec<Value>>,
    calls: Mutex<Vec<(String, Value)>>,
}

impl StubApify {
    fn new() -> Self {
        let mut items = std::collections::HashMap::new();
        items.insert(
            "apify/linkedin-profile-scraper".into(),
            vec![json!({
                "name": "Ada Lovelace",
                "headline": "Principal Engineer at Analytical Engines",
                "url": "https://linkedin.example/in/ada",
            })],
        );
        items.insert(
            "apify/website-content-crawler".into(),
            vec![json!({
                "url": "https://linkedin.example/in/ada",
                "text": "Ada Lovelace is a principal engineer who works on computing systems.",
            })],
        );
        Self {
            items,
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ApifyClient for StubApify {
    async fn run_actor(
        &self,
        actor_id: &str,
        input: Value,
        _timeout_secs: u32,
    ) -> Result<ApifyRunResult, ApifyError> {
        self.calls
            .lock()
            .unwrap()
            .push((actor_id.to_string(), input));
        let items = self.items.get(actor_id).cloned().unwrap_or_default();
        Ok(ApifyRunResult {
            run_id: "run_stub_1".into(),
            actor_id: actor_id.to_string(),
            status: "SUCCEEDED".into(),
            items,
        })
    }
}

// ── Canned LLM responses ────────────────────────────────────────────────

fn tool_call(id: &str, name: &str, args: Value) -> LlmResponse {
    LlmResponse {
        content: String::new(),
        tool_calls: vec![LlmToolCall {
            id: id.into(),
            name: name.into(),
            arguments: args,
        }],
        finish_reason: "tool_calls".into(),
    }
}

fn final_reply(text: &str) -> LlmResponse {
    LlmResponse {
        content: text.into(),
        tool_calls: Vec::new(),
        finish_reason: "stop".into(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[tokio::test]
async fn discovery_runner_writes_owner_facts_and_doc() {
    let _ = env_logger::builder()
        .is_test(true)
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    let tmp = tempdir().expect("tempdir");
    let workspace_dir = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir).unwrap();

    let memory =
        Arc::new(MemoryClient::from_workspace_dir(workspace_dir.clone()).expect("MemoryClient"));

    // Script:
    //   round 1 → assistant calls apify_search_person(query="ada@example.com")
    //   round 2 → assistant calls apify_fetch_url(url=...)
    //   round 3 → assistant calls owner_write({facts, document})
    //   round 4 → assistant replies "done" and stops
    let script = vec![
        tool_call(
            "call_1",
            "apify_search_person",
            json!({ "query": "ada@example.com" }),
        ),
        tool_call(
            "call_2",
            "apify_fetch_url",
            json!({ "url": "https://linkedin.example/in/ada" }),
        ),
        tool_call(
            "call_3",
            "owner_write",
            json!({
                "facts": [
                    {"type": "identity", "key": "full_name", "value": "Ada Lovelace", "confidence": 0.95},
                    {"type": "identity", "key": "email",     "value": "ada@example.com", "confidence": 0.99},
                    {"type": "identity", "key": "company",   "value": "Analytical Engines"},
                    {"type": "role",     "key": "title",     "value": "Principal Engineer"}
                ],
                "document": {
                    "title": "LinkedIn bio",
                    "content": "Ada Lovelace is a principal engineer who works on computing systems."
                }
            }),
        ),
        final_reply("Wrote 4 facts + 1 document."),
    ];

    let llm = Arc::new(ScriptedLlm::new(script));
    let apify: Arc<dyn ApifyClient> = Arc::new(StubApify::new());

    let job = DiscoveryJob {
        trigger: DiscoveryTrigger::Test,
        seed_facts: vec![SeedFact::new("email", "ada@example.com")],
    };
    let deps = DiscoveryDeps {
        memory: memory.clone(),
        llm: llm.clone(),
        apify,
        config: DiscoveryConfig::default(),
        model: "test-model".into(),
    };

    let report = run_discovery(job, deps).await.expect("run_discovery ok");

    // ── Report assertions ────────────────────────────────────────────
    assert_eq!(
        report.facts_written, 4,
        "expected 4 facts, errors={:?}",
        report.errors
    );
    assert_eq!(report.docs_written, 1);
    assert_eq!(report.rounds_used, 4);
    assert!(
        report.errors.is_empty(),
        "expected no errors, got {:?}",
        report.errors
    );
    assert_eq!(
        llm.call_count(),
        4,
        "LLM should be called exactly 4 times (3 tool rounds + 1 final)"
    );

    // History on round 4 should include tool replies for all three
    // prior calls.
    let final_round_msgs = &llm.call_history()[3];
    let tool_replies: Vec<&LlmMessage> = final_round_msgs
        .iter()
        .filter(|m| {
            matches!(
                m.role,
                openhuman_core::openhuman::agent::discovery::llm::LlmRole::Tool
            )
        })
        .collect();
    assert_eq!(tool_replies.len(), 3);

    // ── Profile table assertions ─────────────────────────────────────
    let facets = memory.profile_load_all().expect("profile_load_all ok");
    let identity_full_name = facets
        .iter()
        .find(|f| f.key == "full_name")
        .expect("full_name facet present");
    assert_eq!(identity_full_name.value, "Ada Lovelace");
    assert!(
        identity_full_name
            .source_segment_ids
            .as_deref()
            .unwrap_or("")
            .contains("discovery-"),
        "expected discovery-test origin tag in segment ids"
    );

    // ── build_owner_section surfaces the Identity facets ─────────────
    // Note: the live owner section is scoped to Identity facets only
    // (per Phase 2 design). Role facets land in the separate user-
    // profile section, so we don't assert Principal Engineer here.
    let rendered = build_owner_section(&memory).await;
    assert!(
        rendered.contains("## Owner"),
        "owner section should contain heading"
    );
    assert!(
        rendered.contains("Ada Lovelace"),
        "owner section should contain full name"
    );
    assert!(
        rendered.contains("Analytical Engines"),
        "owner section should contain company"
    );

    // Role facet exists in the profile table even though it's not
    // rendered into the owner section.
    let role_facet = facets
        .iter()
        .find(|f| f.key == "title")
        .expect("role/title facet present in profile table");
    assert_eq!(role_facet.value, "Principal Engineer");
}

#[tokio::test]
async fn discovery_runner_caps_at_max_rounds() {
    let _ = env_logger::builder().is_test(true).try_init();

    let tmp = tempdir().expect("tempdir");
    let workspace_dir = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir).unwrap();
    let memory = Arc::new(MemoryClient::from_workspace_dir(workspace_dir).expect("MemoryClient"));

    // Script always returns tool_calls — runner should bail out
    // after max_rounds without infinite-looping.
    let mut script = Vec::new();
    for i in 0..10 {
        script.push(tool_call(
            &format!("call_{i}"),
            "apify_search_person",
            json!({ "query": "loop" }),
        ));
    }
    let llm = Arc::new(ScriptedLlm::new(script));

    let job = DiscoveryJob {
        trigger: DiscoveryTrigger::Test,
        seed_facts: vec![],
    };
    let config = DiscoveryConfig {
        max_rounds: 3,
        ..DiscoveryConfig::default()
    };
    let deps = DiscoveryDeps {
        memory: memory.clone(),
        llm: llm.clone(),
        apify: Arc::new(StubApify::new()),
        config,
        model: "test-model".into(),
    };

    let report = run_discovery(job, deps).await.expect("run_discovery ok");
    assert_eq!(report.rounds_used, 3);
    assert_eq!(llm.call_count(), 3);
    assert!(
        report.errors.iter().any(|e| e.contains("max_rounds")),
        "expected max_rounds error, got {:?}",
        report.errors
    );
}

#[tokio::test]
async fn discovery_runner_continues_past_tool_errors() {
    let _ = env_logger::builder().is_test(true).try_init();

    let tmp = tempdir().expect("tempdir");
    let workspace_dir = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace_dir).unwrap();
    let memory = Arc::new(MemoryClient::from_workspace_dir(workspace_dir).expect("MemoryClient"));

    // First call is an `owner_write` with a BAD facet type — should
    // produce a soft error in the report. Second call is valid.
    // Third is the final reply.
    let script = vec![
        tool_call(
            "bad",
            "owner_write",
            json!({
                "facts": [{"type": "quantum", "key": "spin", "value": "up"}]
            }),
        ),
        tool_call(
            "good",
            "owner_write",
            json!({
                "facts": [{"type": "identity", "key": "email", "value": "ada@example.com"}]
            }),
        ),
        final_reply("done"),
    ];
    let llm = Arc::new(ScriptedLlm::new(script));

    let deps = DiscoveryDeps {
        memory: memory.clone(),
        llm,
        apify: Arc::new(StubApify::new()),
        config: DiscoveryConfig::default(),
        model: "test-model".into(),
    };
    let job = DiscoveryJob {
        trigger: DiscoveryTrigger::Test,
        seed_facts: vec![],
    };

    let report = run_discovery(job, deps).await.expect("run_discovery ok");
    assert_eq!(report.facts_written, 1);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("unknown type 'quantum'")),
        "expected unknown-type error, got {:?}",
        report.errors
    );
}
