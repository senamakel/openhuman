//! Supervisor tests against a mock NDJSON serve server on a unix socket.
//!
//! Mirrors the `runtime_python_server` mock-JSONL tests: a mock listener plays
//! `serve`, the supervisor plays `host`. Covers the handshake, an `instruct`
//! round trip, `inference` port-callback routing, and restart-on-death.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use super::*;
use crate::openhuman::medulla_local::ports::{HostPorts, PortError};
use crate::openhuman::medulla_local::types::{InferenceCall, InferenceResult, ToolSpec, Usage};

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_socket_path(tag: &str) -> PathBuf {
    let n = SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "oh-medulla-{}-{}-{}.sock",
        tag,
        std::process::id(),
        n
    ))
}

/// Records what the host ports were asked to do, and answers with canned data.
#[derive(Default)]
struct RecordingState {
    inference_tiers: Mutex<Vec<String>>,
    tool_names: Mutex<Vec<String>>,
}

struct RecordingPorts {
    state: Arc<RecordingState>,
}

/// The curated read-only tool the recording ports advertise and answer. A real
/// [`OpenhumanHostPorts`] derives this list from the runtime tool surface; the
/// test uses one fixed spec so both the `hello` advertisement and the
/// `tools.invoke` dispatch can be asserted end to end.
fn recording_tool_spec() -> ToolSpec {
    ToolSpec {
        name: "file_read".to_string(),
        description: "Read a file from the workspace".to_string(),
        parameters: json!({ "type": "object", "properties": { "path": { "type": "string" } } }),
    }
}

#[async_trait]
impl HostPorts for RecordingPorts {
    fn tool_specs(&self) -> Vec<ToolSpec> {
        vec![recording_tool_spec()]
    }

    async fn invoke_inference(&self, call: InferenceCall) -> Result<InferenceResult, PortError> {
        self.state
            .inference_tiers
            .lock()
            .unwrap()
            .push(call.tier.clone());
        Ok(InferenceResult {
            content: "canned-answer".to_string(),
            reasoning_content: None,
            model: "test-model".to_string(),
            tool_calls: Vec::new(),
            usage: Usage {
                input_tokens: 5,
                output_tokens: 2,
            },
        })
    }

    async fn invoke_tool(&self, name: &str, _args: Value) -> Result<Value, PortError> {
        self.state.tool_names.lock().unwrap().push(name.to_string());
        Ok(json!({ "content": [{ "type": "text", "text": "ok" }], "isError": false }))
    }
}

/// Behaviour knobs for one mock serve run.
#[derive(Clone, Default)]
struct MockOpts {
    /// Issue an `inference.invoke` port call before answering the first
    /// `instruct`, asserting the returned `ret` content.
    issue_inference: bool,
    /// Issue a `tools.invoke` port call before answering the first `instruct`,
    /// asserting the returned `ret` content.
    issue_tool_call: bool,
    /// Drop the connection on the first `instruct` of the first connection.
    /// `instruct` is non-idempotent, so the supervisor must fail fast with
    /// `MaybeApplied` instead of restart-and-retry.
    die_first_instruct: bool,
    /// Drop the connection on the first `status` of the first connection,
    /// forcing the supervisor to restart-and-retry (status is idempotent).
    die_first_status: bool,
    /// Counts `instruct` requests that actually reached the mock, so a test
    /// can assert a transport break did not cause a duplicate submission.
    instruct_count: Option<Arc<AtomicU64>>,
    /// Answer every `instruct` with `ok=false` (`bad_request`): a healthy
    /// connection issuing an application-level rejection, which must NOT
    /// trigger restart-and-retry.
    reject_instruct: bool,
    /// Counts accepted connections, so a test can assert whether the
    /// supervisor restarted (2) or failed fast on the live child (1).
    connection_count: Option<Arc<AtomicU64>>,
    /// Sink recording the tool names advertised in the `hello` request, so a
    /// test can assert the host advertised its curated surface.
    observed_hello_tools: Option<Arc<Mutex<Vec<String>>>>,
}

/// Spawn a mock serve loop on `listener`, one accepted connection at a time.
fn spawn_mock_serve(listener: UnixListener, opts: MockOpts) {
    tokio::spawn(async move {
        let mut conn_index = 0u64;
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => break,
            };
            if let Some(counter) = &opts.connection_count {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            serve_connection(stream, conn_index, &opts).await;
            conn_index += 1;
        }
    });
}

async fn serve_connection(stream: UnixStream, conn_index: u64, opts: &MockOpts) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half).lines();

    // Unprompted ready banner (§3).
    write_line(
        &mut write_half,
        &json!({
            "t": "ready", "protocol": 1, "serve": "3.12.0-test",
            "sessionId": "agent", "capabilities": ["inference", "tools"], "error": null
        }),
    )
    .await;

    while let Ok(Some(line)) = reader.next_line().await {
        let frame: Value = match serde_json::from_str(&line) {
            Ok(frame) => frame,
            Err(_) => continue,
        };
        // Host→serve frames we handle: req and ret. We only initiate ret reads
        // inline (below), so here we only see `req`.
        if frame.get("t").and_then(Value::as_str) != Some("req") {
            continue;
        }
        let id = frame
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let op = frame.get("op").and_then(Value::as_str).unwrap_or("");
        match op {
            "hello" => {
                if let Some(sink) = &opts.observed_hello_tools {
                    let names: Vec<String> = frame
                        .get("params")
                        .and_then(|params| params.get("tools"))
                        .and_then(Value::as_array)
                        .map(|tools| {
                            tools
                                .iter()
                                .filter_map(|tool| tool.get("name").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    *sink.lock().unwrap() = names;
                }
                write_line(
                    &mut write_half,
                    &json!({
                        "t": "res", "id": id, "ok": true,
                        "result": { "protocol": 1, "sessionId": "agent", "ports": ["inference", "tools"] }
                    }),
                )
                .await;
            }
            "status" => {
                if opts.die_first_status && conn_index == 0 {
                    // Drop the connection mid-request: status is idempotent,
                    // so the host must restart and retry.
                    return;
                }
                write_line(
                    &mut write_half,
                    &json!({
                        "t": "res", "id": id, "ok": true,
                        "result": { "state": "running", "queued": 0 }
                    }),
                )
                .await;
            }
            "instruct" => {
                if let Some(counter) = &opts.instruct_count {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
                if opts.die_first_instruct && conn_index == 0 {
                    // Drop the connection mid-request: instruct is
                    // non-idempotent, so the host must NOT replay it.
                    return;
                }
                if opts.reject_instruct {
                    // Application-level rejection over a healthy connection:
                    // the host must fail fast, not kill and respawn us.
                    write_line(
                        &mut write_half,
                        &json!({
                            "t": "res", "id": id, "ok": false,
                            "error": { "code": "bad_request", "message": "instruct refused by mock" }
                        }),
                    )
                    .await;
                    continue;
                }
                if opts.issue_inference {
                    // Reverse-RPC into the host inference port, then read its ret.
                    write_line(
                        &mut write_half,
                        &json!({
                            "t": "call", "id": "c1", "port": "inference", "method": "invoke",
                            "params": {
                                "tier": "orchestrator", "op": "orchestrate", "cycleId": "cyc:1",
                                "messages": [{ "role": "user", "content": "reconcile" }]
                            }
                        }),
                    )
                    .await;
                    let ret = read_frame(&mut reader)
                        .await
                        .expect("host must answer the call");
                    assert_eq!(ret["t"], "ret");
                    assert_eq!(ret["id"], "c1");
                    assert_eq!(ret["ok"], true);
                    assert_eq!(ret["result"]["content"], "canned-answer");
                }
                if opts.issue_tool_call {
                    // Reverse-RPC into the host tools port, then read its ret.
                    write_line(
                        &mut write_half,
                        &json!({
                            "t": "call", "id": "c2", "port": "tools", "method": "invoke",
                            "params": {
                                "name": "file_read",
                                "args": { "path": "README.md" },
                                "callId": "cyc:1:tool_call:0", "cycleId": "cyc:1"
                            }
                        }),
                    )
                    .await;
                    let ret = read_frame(&mut reader)
                        .await
                        .expect("host must answer the tools call");
                    assert_eq!(ret["t"], "ret");
                    assert_eq!(ret["id"], "c2");
                    assert_eq!(ret["ok"], true);
                    assert_eq!(ret["result"]["isError"], false);
                    assert_eq!(ret["result"]["content"][0]["text"], "ok");
                }
                write_line(
                    &mut write_half,
                    &json!({
                        "t": "res", "id": id, "ok": true,
                        "result": { "instructionId": "inst-agent-0", "cycleId": "cyc:agent:agent:0" }
                    }),
                )
                .await;
            }
            _ => {
                write_line(
                    &mut write_half,
                    &json!({
                        "t": "res", "id": id, "ok": false,
                        "error": { "code": "unknown_op", "message": op }
                    }),
                )
                .await;
            }
        }
    }
}

async fn write_line(writer: &mut tokio::net::unix::OwnedWriteHalf, frame: &Value) {
    let mut line = serde_json::to_string(frame).unwrap();
    line.push('\n');
    writer.write_all(line.as_bytes()).await.unwrap();
    writer.flush().await.unwrap();
}

async fn read_frame(
    reader: &mut tokio::io::Lines<BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> Option<Value> {
    let line = reader.next_line().await.ok()??;
    serde_json::from_str(&line).ok()
}

/// A connector that dials the mock listener instead of spawning Node.
struct MockConnector {
    path: PathBuf,
}

#[async_trait]
impl Connector for MockConnector {
    async fn connect(&self, ports: Arc<dyn HostPorts>) -> anyhow::Result<Connection> {
        let stream = connect_unix_retry(&self.path, Duration::from_secs(5)).await?;
        let hello = super::HelloParams {
            protocol: super::PROTOCOL_VERSION,
            host: "openhuman/test".to_string(),
            ports: vec!["inference".to_string(), "tools".to_string()],
            tools: ports.tool_specs(),
        };
        Connection::establish(stream, ports, hello, None).await
    }

    fn describe(&self) -> String {
        "mock".to_string()
    }
}

fn build(path: PathBuf, state: Arc<RecordingState>) -> MedullaSupervisor {
    let ports: Arc<dyn HostPorts> = Arc::new(RecordingPorts { state });
    MedullaSupervisor::new(Arc::new(MockConnector { path }), ports)
}

#[tokio::test]
async fn handshake_negotiates_ready_and_hello() {
    let path = unique_socket_path("handshake");
    let listener = UnixListener::bind(&path).unwrap();
    spawn_mock_serve(listener, MockOpts::default());

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    supervisor.ensure().await.expect("handshake should succeed");

    let status = supervisor.snapshot().await;
    assert!(status.running);
    assert_eq!(status.serve_version.as_deref(), Some("3.12.0-test"));
    assert_eq!(status.session_id.as_deref(), Some("agent"));
    assert_eq!(status.ports, vec!["inference", "tools"]);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn instruct_round_trip_returns_receipt() {
    let path = unique_socket_path("instruct");
    let listener = UnixListener::bind(&path).unwrap();
    spawn_mock_serve(listener, MockOpts::default());

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    let receipt = supervisor
        .instruct("reconcile the world", json!({ "origin": "wake" }))
        .await
        .expect("instruct should return a receipt");
    assert_eq!(receipt.instruction_id, "inst-agent-0");
    assert_eq!(receipt.cycle_id, "cyc:agent:agent:0");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn inference_callback_routes_to_host_ports() {
    let path = unique_socket_path("inference");
    let listener = UnixListener::bind(&path).unwrap();
    spawn_mock_serve(
        listener,
        MockOpts {
            issue_inference: true,
            ..MockOpts::default()
        },
    );

    let state = Arc::new(RecordingState::default());
    let supervisor = build(path.clone(), state.clone());
    let receipt = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect("instruct with an inference callback should complete");
    assert_eq!(receipt.instruction_id, "inst-agent-0");

    // The serve inference call was dispatched to the host ports with its tier.
    let tiers = state.inference_tiers.lock().unwrap().clone();
    assert_eq!(tiers, vec!["orchestrator"]);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn tools_are_advertised_and_invocable() {
    let path = unique_socket_path("tools");
    let listener = UnixListener::bind(&path).unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    spawn_mock_serve(
        listener,
        MockOpts {
            issue_tool_call: true,
            observed_hello_tools: Some(observed.clone()),
            ..MockOpts::default()
        },
    );

    let state = Arc::new(RecordingState::default());
    let supervisor = build(path.clone(), state.clone());
    let receipt = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect("instruct with a tools callback should complete");
    assert_eq!(receipt.instruction_id, "inst-agent-0");

    // The host advertised its curated tool surface in the hello handshake, so
    // serve could bind the tool and drive a `tools.invoke` for it.
    assert_eq!(observed.lock().unwrap().clone(), vec!["file_read"]);
    // And that invocation reached the host ports with the advertised name.
    let names = state.tool_names.lock().unwrap().clone();
    assert_eq!(names, vec!["file_read"]);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn restart_on_death_retries_idempotent_status_once() {
    let path = unique_socket_path("restart");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            die_first_status: true,
            connection_count: Some(connections.clone()),
            ..MockOpts::default()
        },
    );

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    // First connection dies mid-status; status is idempotent, so the
    // supervisor restarts and the second connection answers.
    let status = supervisor
        .harness_status()
        .await
        .expect("restart-and-retry-once should recover an idempotent op");
    assert_eq!(status.state, "running");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "a mid-request transport death must trigger exactly one respawn"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn instruct_transport_failure_fails_fast_without_replay() {
    let path = unique_socket_path("instruct-transport");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    let instructs = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            die_first_instruct: true,
            connection_count: Some(connections.clone()),
            instruct_count: Some(instructs.clone()),
            ..MockOpts::default()
        },
    );

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    let error = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect_err("a mid-instruct transport break must surface as an error");

    // The error is the typed maybe-applied outcome, telling the caller the
    // instruction may or may not have been enqueued…
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::MaybeApplied { op, .. } if op == "instruct"),
        "expected MaybeApplied for the non-idempotent op, got: {request_error:?}"
    );
    assert!(!request_error.is_retryable());

    // …the instruct was submitted exactly once — no duplicate enqueue…
    assert_eq!(
        instructs.load(Ordering::SeqCst),
        1,
        "a non-idempotent op must never be replayed after a transport break"
    );
    // …and no respawn-driven retry connection was made for it.
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "failing fast must not respawn to replay the instruct"
    );

    // The broken connection was reset: a later idempotent request reconnects.
    let status = supervisor
        .harness_status()
        .await
        .expect("the supervisor must recover on the next request");
    assert_eq!(status.state, "running");
    assert_eq!(connections.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn serve_rejection_fails_fast_without_restart() {
    let path = unique_socket_path("reject");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            reject_instruct: true,
            connection_count: Some(connections.clone()),
            ..MockOpts::default()
        },
    );

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    let error = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect_err("an ok=false serve rejection must surface as an error");

    // The error is the typed, non-retryable serve rejection…
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::Serve { code, .. } if code == "bad_request"),
        "expected a Serve error carrying the wire code, got: {request_error:?}"
    );
    assert!(!request_error.is_retryable());

    // …the healthy child was NOT killed and respawned…
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "an application-level rejection must not trigger a restart"
    );

    // …and the connection is still live for the next request.
    let status = supervisor.snapshot().await;
    assert!(
        status.running,
        "the connection must survive a serve rejection"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn ensure_started_backoff_is_keyed_on_config_fingerprint() {
    // An explicit (nonexistent) serve entry wins over any env override, so
    // the build fails deterministically before touching Node or the network.
    let mut config_a = crate::openhuman::config::Config::default();
    config_a.subconscious.medulla_local.serve_entry =
        "/nonexistent/medulla-local-test/serve-a.js".to_string();

    let first = ensure_started(&config_a)
        .await
        .expect_err("a missing serve entry must fail startup");
    assert!(
        format!("{first:#}").contains("serve entry not found"),
        "unexpected first startup error: {first:#}"
    );

    // Same config within the backoff window: fail fast on the cached failure.
    let cached = ensure_started(&config_a)
        .await
        .expect_err("the same config must stay in start-failure backoff");
    assert!(
        format!("{cached:#}").contains("after previous startup failure"),
        "unexpected backoff error: {cached:#}"
    );

    // A changed config bypasses the backoff and attempts a fresh build — the
    // new snapshot may be exactly what fixes the failure.
    let mut config_b = config_a.clone();
    config_b.subconscious.medulla_local.serve_entry =
        "/nonexistent/medulla-local-test/serve-b.js".to_string();
    let rebuilt = ensure_started(&config_b)
        .await
        .expect_err("the changed config still points at a missing entry");
    let rebuilt_message = format!("{rebuilt:#}");
    assert!(
        !rebuilt_message.contains("after previous startup failure"),
        "a config change must bypass the stale backoff: {rebuilt_message}"
    );
    assert!(
        rebuilt_message.contains("serve-b.js"),
        "the fresh build must run against the NEW config: {rebuilt_message}"
    );
}

#[test]
fn config_fingerprint_tracks_relevant_config_changes() {
    let base = crate::openhuman::config::Config::default();
    let base_fingerprint = config_fingerprint(&base).unwrap();
    assert_eq!(
        base_fingerprint,
        config_fingerprint(&base.clone()).unwrap(),
        "the fingerprint must be deterministic for an identical config"
    );

    // The cached ports capture the whole config: a serve-entry change, a
    // security-root change, and an action-dir change must each invalidate.
    let mut serve_changed = base.clone();
    serve_changed.subconscious.medulla_local.serve_entry = "/elsewhere/serve.js".to_string();
    assert_ne!(
        base_fingerprint,
        config_fingerprint(&serve_changed).unwrap()
    );

    let mut action_dir_changed = base.clone();
    action_dir_changed.action_dir = std::path::PathBuf::from("/elsewhere/projects");
    assert_ne!(
        base_fingerprint,
        config_fingerprint(&action_dir_changed).unwrap()
    );
}
