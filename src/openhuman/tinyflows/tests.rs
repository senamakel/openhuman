//! Seam tests for `src/openhuman/tinyflows/`.
//!
//! **Deviation from the original test plan** (see
//! `my_docs/ohxtf/b1-engine-seam-domain/09-testing-and-verification.md` item 2
//! and commons/11): the plan called for pointing `HttpRequestTool` at a local
//! mock HTTP server and asserting a success round-trip. That is not possible
//! against the REAL `HttpRequestTool` — unlike `tinyflows`' own mock
//! `HttpClient`, OpenHuman's `url_guard` unconditionally blocks
//! loopback/private hosts as an SSRF guard (`is_private_or_local_host`),
//! before the allowlist is even consulted, and any locally-hosted mock server
//! is necessarily loopback. So instead:
//! - the HTTP adapter tests assert the SSRF guard and the strict-allowlist
//!   rejection both surface as `EngineError::Capability` (proving the adapter
//!   correctly propagates `HttpRequestTool`'s real security behavior), and
//! - the engine smoke test drives `trigger -> http_request` against a
//!   deterministically-blocked loopback URL with `on_error: continue`, which
//!   exercises the full real stack (build_capabilities -> engine -> compiled
//!   graph -> `OpenHumanHttp` -> real `HttpRequestTool` -> SSRF guard ->
//!   `EngineError::Capability` -> the crate's `on_error: continue` policy ->
//!   error item) without any network dependency.

use std::sync::Arc;

use serde_json::json;
use tempfile::TempDir;
use tinyflows::caps::{CodeLanguage, CodeRunner, HttpClient, StateStore, ToolInvoker};
use tinyflows::model::{Edge, Node, NodeKind, WorkflowGraph};

use crate::openhuman::config::Config;
use crate::openhuman::security::SecurityPolicy;

use super::build_capabilities;
use super::caps::{FlowStateStore, OpenHumanCode, OpenHumanHttp, OpenHumanTools};

fn test_config(tmp: &TempDir) -> Arc<Config> {
    let config = Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        ..Config::default()
    };
    std::fs::create_dir_all(&config.workspace_dir).unwrap();
    Arc::new(config)
}

fn node(id: &str, kind: NodeKind, config: serde_json::Value) -> Node {
    Node {
        id: id.to_string(),
        kind,
        type_version: 1,
        name: id.to_string(),
        config,
        ports: Vec::new(),
        position: None,
    }
}

fn edge(from: &str, to: &str) -> Edge {
    Edge {
        from_node: from.to_string(),
        from_port: "main".to_string(),
        to_node: to.to_string(),
        to_port: "main".to_string(),
    }
}

// ── build_capabilities smoke ────────────────────────────────────────────

#[test]
fn build_capabilities_constructs_every_slot_without_panicking() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    // Purely a construction smoke test — no capability is invoked here.
    let _caps = build_capabilities(config, "test:build");
}

// ── HTTP adapter ─────────────────────────────────────────────────────────

fn http_adapter(allowed_domains: Vec<String>) -> OpenHumanHttp {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
        &config.action_dir,
    ));
    OpenHumanHttp {
        security,
        http_config: crate::openhuman::config::HttpRequestConfig {
            allowed_domains,
            ..Default::default()
        },
        http_creds: Arc::new(
            crate::openhuman::credentials::HttpCredentialsStore::from_config(&config),
        ),
    }
}

#[tokio::test]
async fn http_adapter_blocks_loopback_host_as_capability_error() {
    let adapter = http_adapter(vec![]); // open allowlist mode
    let err = adapter
        .request(
            json!({ "method": "GET", "url": "http://127.0.0.1:1/" }),
            None,
        )
        .await
        .expect_err("loopback host must be blocked by the SSRF guard");
    let msg = err.to_string();
    assert!(
        msg.to_lowercase().contains("private") || msg.to_lowercase().contains("local"),
        "expected an SSRF-guard message, got: {msg}"
    );
}

#[tokio::test]
async fn http_adapter_rejects_host_outside_strict_allowlist() {
    let adapter = http_adapter(vec!["example.com".to_string()]);
    let err = adapter
        .request(
            json!({ "method": "GET", "url": "https://not-allowed.test/" }),
            None,
        )
        .await
        .expect_err("host outside the strict allowlist must be rejected");
    assert!(
        err.to_string().contains("not-allowed.test")
            || err.to_string().to_lowercase().contains("allowed"),
        "expected an allowlist rejection message, got: {err}"
    );
}

// ── StateStore adapter ───────────────────────────────────────────────────

#[tokio::test]
async fn flow_state_store_round_trips_and_is_namespace_scoped() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);

    let ns1 = FlowStateStore {
        config: config.clone(),
        namespace: "ns1".to_string(),
    };
    let ns2 = FlowStateStore {
        config: config.clone(),
        namespace: "ns2".to_string(),
    };

    assert!(ns1.load("k").await.unwrap().is_none());

    ns1.store("k", json!({ "v": 1 })).await.unwrap();
    assert_eq!(ns1.load("k").await.unwrap(), Some(json!({ "v": 1 })));

    // A different namespace never sees ns1's value.
    assert!(ns2.load("k").await.unwrap().is_none());

    // Overwrite.
    ns1.store("k", json!(2)).await.unwrap();
    assert_eq!(ns1.load("k").await.unwrap(), Some(json!(2)));
}

// ── Engine smoke: real seam end to end ───────────────────────────────────

#[tokio::test]
async fn engine_run_drives_trigger_to_http_request_through_the_real_seam() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let caps = build_capabilities(config, "test:smoke");

    // A deterministically-blocked loopback URL with `on_error: continue` so
    // the run completes even though the (real, SSRF-guarded) HTTP adapter
    // necessarily rejects it — see the module doc for why a real network
    // round-trip isn't testable here.
    let graph = WorkflowGraph {
        nodes: vec![
            node("t", NodeKind::Trigger, serde_json::Value::Null),
            node(
                "http",
                NodeKind::HttpRequest,
                json!({ "method": "GET", "url": "http://127.0.0.1:1/", "on_error": "continue" }),
            ),
        ],
        edges: vec![edge("t", "http")],
        ..Default::default()
    };
    let compiled = tinyflows::compiler::compile(&graph).expect("compile");

    let outcome = tinyflows::engine::run(&compiled, json!({ "seed": 1 }), &caps)
        .await
        .expect("run should complete (on_error: continue)");

    assert!(outcome.pending_approvals.is_empty());
    assert_eq!(
        outcome.output["nodes"]["http"]["items"][0]["json"]["error"]["node"],
        json!("http")
    );
}

// ── Code adapter ──────────────────────────────────────────────────────────

/// Requires `node` on `PATH`. Ignored by default (per the B1 test plan);
/// run explicitly with `cargo test -- --ignored` on a host with Node
/// installed.
#[tokio::test]
#[ignore = "requires a `node` binary on PATH"]
async fn code_adapter_javascript_passthrough_round_trips_json() {
    let tmp = TempDir::new().unwrap();
    let config = test_config(&tmp);
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
        &config.action_dir,
    ));
    let runner = OpenHumanCode { config, security };

    let input = json!([{ "json": { "n": 7 } }]);
    let result = runner
        .run(CodeLanguage::JavaScript, "return input;", input.clone())
        .await
        .expect("javascript passthrough should succeed when node is present");
    assert_eq!(result, input);
}

// ── Tool curation / scope + connection_ref (issue B2) ─────────────────────
//
// No `ApprovalGate` is installed in this test binary (see the module doc on
// `flows::bus`'s tests and the trust-model tests in `approval::gate` for the
// gate-level behavior) — these tests exercise the *curation* gate, which is
// independent of the approval gate and runs first, so they stay deterministic
// without any global state.

fn tools_adapter(config: Arc<Config>) -> OpenHumanTools {
    OpenHumanTools { config }
}

#[tokio::test]
async fn tools_invoke_rejects_a_non_curated_slug_for_a_known_toolkit() {
    let tmp = TempDir::new().unwrap();
    let tools = tools_adapter(test_config(&tmp));

    // "gmail" has a curated catalog; this action is not in it, so curation
    // must reject regardless of the user's read/write/admin scope prefs.
    let err = tools
        .invoke("GMAIL_NOT_A_REAL_CURATED_ACTION", json!({}), None)
        .await
        .expect_err("a non-curated action for a curated toolkit must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("tool not permitted"),
        "expected a curation rejection message, got: {msg}"
    );
    assert!(msg.contains("GMAIL_NOT_A_REAL_CURATED_ACTION"));
}

#[tokio::test]
async fn tools_invoke_rejects_an_unrecognized_toolkit_slug() {
    // Issue B2 finding #2 (deny-by-default): a made-up toolkit prefix that
    // isn't in any curated catalog must be rejected — not passed through on
    // a permissive "unknown toolkit" heuristic. Live testing confirmed this
    // used to reach Composio (and only failed there for lack of a signed-in
    // session), which is not a hard allowlist.
    let tmp = TempDir::new().unwrap();
    let tools = tools_adapter(test_config(&tmp));

    let err = tools
        .invoke("madeupkit_dostuff", json!({}), None)
        .await
        .expect_err("an unrecognized toolkit slug must be rejected by curation");
    let msg = err.to_string();
    assert!(
        msg.contains("tool not permitted"),
        "expected a curation rejection message, got: {msg}"
    );
    assert!(msg.contains("madeupkit_dostuff"));
}

#[tokio::test]
async fn tools_invoke_rejects_a_prefix_less_slug() {
    // "noop" has no curated catalog (`catalog_for_toolkit` returns `None`
    // for the single-segment "toolkit" `toolkit_from_slug` degrades it to),
    // so the hard allowlist in `is_curated_flow_tool` rejects it outright —
    // unlike the general agent tool-call path's `is_action_visible_with_pref`,
    // which falls back to the permissive `classify_unknown` heuristic and
    // would let this slug through.
    let tmp = TempDir::new().unwrap();
    let tools = tools_adapter(test_config(&tmp));

    let err = tools
        .invoke("noop", json!({}), None)
        .await
        .expect_err("a prefix-less/unrecognized slug must be rejected by curation");
    assert!(
        err.to_string().contains("tool not permitted"),
        "expected a curation rejection message, got: {err}"
    );
}

#[tokio::test]
async fn tools_invoke_does_not_reject_a_known_curated_slug_at_the_curation_gate() {
    // A real curated action for a known toolkit must clear the curation
    // gate — it may still fail further downstream (no composio client
    // configured in this test environment), but that failure must NOT be
    // the "tool not permitted" curation-rejection message.
    let tmp = TempDir::new().unwrap();
    let tools = tools_adapter(test_config(&tmp));

    let err = tools
        .invoke("GMAIL_SEND_EMAIL", json!({}), None)
        .await
        .expect_err("no composio client is configured in the test environment");
    assert!(
        !err.to_string().contains("tool not permitted"),
        "a known curated slug must not be rejected by curation, got: {err}"
    );
}

#[test]
fn composio_connection_id_parses_toolkit_prefixed_ref() {
    assert_eq!(
        super::caps::composio_connection_id("composio:slack:acct_123"),
        Some("acct_123")
    );
    // Trailing segment only — works even without a toolkit segment present.
    assert_eq!(
        super::caps::composio_connection_id("composio::acct_1"),
        Some("acct_1")
    );
}

#[test]
fn composio_connection_id_returns_none_for_non_composio_ref_or_empty_id() {
    assert_eq!(
        super::caps::composio_connection_id("http_cred:my-secret"),
        None
    );
    assert_eq!(super::caps::composio_connection_id("composio:"), None);
    assert_eq!(super::caps::composio_connection_id("composio:slack:"), None);
}

#[test]
fn http_cred_name_parses_and_trims() {
    assert_eq!(
        super::caps::http_cred_name("http_cred:my-secret"),
        Some("my-secret")
    );
    assert_eq!(
        super::caps::http_cred_name("http_cred: spaced "),
        Some("spaced")
    );
}

#[test]
fn http_cred_name_returns_none_for_non_http_cred_ref_or_empty_name() {
    assert_eq!(super::caps::http_cred_name("composio:slack:acct_1"), None);
    assert_eq!(super::caps::http_cred_name("http_cred:"), None);
}
