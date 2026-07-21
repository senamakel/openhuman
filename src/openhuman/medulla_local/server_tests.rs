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
use crate::openhuman::medulla_local::types::{InferenceCall, InferenceResult, Usage};

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

#[async_trait]
impl HostPorts for RecordingPorts {
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
    /// Drop the connection on the first `instruct` of the first connection,
    /// forcing the supervisor to restart-and-retry.
    die_first_instruct: bool,
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
                if opts.die_first_instruct && conn_index == 0 {
                    // Drop the connection mid-request: the host must restart.
                    return;
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
            tools: Vec::new(),
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
async fn restart_on_death_retries_once() {
    let path = unique_socket_path("restart");
    let listener = UnixListener::bind(&path).unwrap();
    spawn_mock_serve(
        listener,
        MockOpts {
            die_first_instruct: true,
            ..MockOpts::default()
        },
    );

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    // First connection dies mid-instruct; the supervisor restarts and the
    // second connection answers.
    let receipt = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect("restart-and-retry-once should recover");
    assert_eq!(receipt.instruction_id, "inst-agent-0");
    let _ = std::fs::remove_file(&path);
}
