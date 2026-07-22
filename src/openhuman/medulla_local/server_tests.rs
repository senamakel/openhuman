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
    /// Never resolve `invoke_inference` (after recording the dispatch): the
    /// shape of a hung model provider, so a test can assert the overall
    /// request deadline bounds port-callback servicing too.
    stall_inference: bool,
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
        if self.stall_inference {
            std::future::pending::<()>().await;
        }
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
    /// On `instruct`/`status`, stream `event` frames forever instead of ever
    /// answering the correlated `res`. Each frame feeds the host's per-read
    /// idle timeout, so only the overall per-request deadline can end the
    /// wait — the regression shape for a wedged child that keeps emitting.
    stream_events_instead_of_res: bool,
    /// On `instruct`/`status`, issue an `inference.invoke` port call and then
    /// go silent (never answer the correlated `res`). Paired with a host whose
    /// inference port never resolves, this is the regression shape for a hung
    /// provider/tool holding the connection during callback servicing — only
    /// the overall per-request deadline can end the wait.
    stall_call_before_res: bool,
    /// Drop the connection on `status` for each of the first N accepted
    /// connections (generalizes `die_first_status`), so the retry attempt can
    /// be made to fail too.
    die_status_connections: u64,
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
        if opts.stream_events_instead_of_res && matches!(op, "instruct" | "status") {
            if op == "instruct" {
                if let Some(counter) = &opts.instruct_count {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }
            // Never answer the correlated res; keep the connection chatty so
            // the per-read idle timeout is fed on every iteration. Stop when
            // the host gives up and drops the connection (write error).
            let mut seq = 0u64;
            loop {
                seq += 1;
                let mut line = json!({
                    "t": "event", "seq": seq, "at": 0,
                    "event": { "type": "progress" }
                })
                .to_string();
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err()
                    || write_half.flush().await.is_err()
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        if opts.stall_call_before_res && matches!(op, "instruct" | "status") {
            if op == "instruct" {
                if let Some(counter) = &opts.instruct_count {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }
            // Reverse-RPC into the host, then go silent: no further frames.
            // The host's inference port never resolves either, so only its
            // overall per-request deadline can end the callback servicing.
            write_line(
                &mut write_half,
                &json!({
                    "t": "call", "id": format!("stall-{conn_index}"),
                    "port": "inference", "method": "invoke",
                    "params": {
                        "tier": "orchestrator", "op": "orchestrate", "cycleId": "cyc:1",
                        "messages": [{ "role": "user", "content": "stall" }]
                    }
                }),
            )
            .await;
            // Drain until the host gives up and drops the connection.
            while let Ok(Some(_)) = reader.next_line().await {}
            return;
        }
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
                if (opts.die_first_status && conn_index == 0)
                    || conn_index < opts.die_status_connections
                {
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
    request_deadline: Duration,
}

#[async_trait]
impl Connector for MockConnector {
    async fn connect(&self, ports: Arc<dyn HostPorts>) -> anyhow::Result<Connection> {
        let stream = connect_unix_retry(&self.path, Duration::from_secs(5)).await?;
        let hello = mock_hello(&ports);
        Connection::establish(stream, ports, hello, None, self.request_deadline).await
    }

    fn describe(&self) -> String {
        "mock".to_string()
    }
}

fn mock_hello(ports: &Arc<dyn HostPorts>) -> super::HelloParams {
    super::HelloParams {
        protocol: super::PROTOCOL_VERSION,
        host: "openhuman/test".to_string(),
        ports: vec!["inference".to_string(), "tools".to_string()],
        tools: ports.tool_specs(),
    }
}

/// A connector that dials the mock listener AND spawns a stand-in child
/// process (`sleep`) whose only job is to be supervised, so the child-liveness
/// probe can be exercised hermetically: the transport stays "healthy" (the
/// mock listener never closes) while the supervised process dies.
struct ChildSpawningConnector {
    path: PathBuf,
    /// Pids of every stand-in child spawned, in connect order, so the test
    /// can kill one externally.
    spawned_pids: Arc<Mutex<Vec<u32>>>,
}

#[async_trait]
impl Connector for ChildSpawningConnector {
    async fn connect(&self, ports: Arc<dyn HostPorts>) -> anyhow::Result<Connection> {
        let child = tokio::process::Command::new("sleep")
            .arg("600")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        self.spawned_pids
            .lock()
            .unwrap()
            .push(child.id().expect("freshly spawned child has a pid"));
        let stream = connect_unix_retry(&self.path, Duration::from_secs(5)).await?;
        let hello = mock_hello(&ports);
        Connection::establish(stream, ports, hello, Some(child), Duration::from_secs(300)).await
    }

    fn describe(&self) -> String {
        "mock+child".to_string()
    }
}

fn build(path: PathBuf, state: Arc<RecordingState>) -> MedullaSupervisor {
    build_with_deadline(path, state, Duration::from_secs(300))
}

fn build_with_deadline(
    path: PathBuf,
    state: Arc<RecordingState>,
    request_deadline: Duration,
) -> MedullaSupervisor {
    build_with_ports(
        path,
        Arc::new(RecordingPorts {
            state,
            stall_inference: false,
        }),
        request_deadline,
    )
}

fn build_with_ports(
    path: PathBuf,
    ports: Arc<dyn HostPorts>,
    request_deadline: Duration,
) -> MedullaSupervisor {
    MedullaSupervisor::new(
        Arc::new(MockConnector {
            path,
            request_deadline,
        }),
        ports,
    )
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
async fn overall_deadline_bounds_instruct_despite_continuous_events() {
    let path = unique_socket_path("deadline-instruct");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    let instructs = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            stream_events_instead_of_res: true,
            connection_count: Some(connections.clone()),
            instruct_count: Some(instructs.clone()),
            ..MockOpts::default()
        },
    );

    let supervisor = build_with_deadline(
        path.clone(),
        Arc::new(RecordingState::default()),
        Duration::from_millis(300),
    );
    let started = std::time::Instant::now();
    let error = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect_err("a never-answered instruct must trip the overall deadline");

    // The request ended promptly even though event frames kept feeding the
    // per-read idle timeout — the overall deadline is what fired.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the overall deadline must bound the wait; waited {:?}",
        started.elapsed()
    );

    // The deadline on a non-idempotent op keeps the fail-fast MaybeApplied
    // contract: the instruct reached serve but its outcome was never observed.
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::MaybeApplied { op, .. } if op == "instruct"),
        "expected MaybeApplied for the deadline on a non-idempotent op, got: {request_error:?}"
    );
    assert!(format!("{request_error:#}").contains("deadline"));
    assert_eq!(
        instructs.load(Ordering::SeqCst),
        1,
        "the deadline must not cause a silent replay of the instruct"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "failing fast on the deadline must not respawn to replay the instruct"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn overall_deadline_bounds_idempotent_status_with_one_retry() {
    let path = unique_socket_path("deadline-status");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            stream_events_instead_of_res: true,
            connection_count: Some(connections.clone()),
            ..MockOpts::default()
        },
    );

    let supervisor = build_with_deadline(
        path.clone(),
        Arc::new(RecordingState::default()),
        Duration::from_millis(300),
    );
    let started = std::time::Instant::now();
    let error = supervisor
        .harness_status()
        .await
        .expect_err("a never-answered status must trip the deadline on both attempts");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "both attempts together must stay bounded; waited {:?}",
        started.elapsed()
    );

    // The deadline is a transport-class failure, so the idempotent op kept
    // its restart-and-retry-once semantics: exactly one respawn happened.
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::Transport(_)),
        "expected the typed transport deadline error, got: {request_error:?}"
    );
    assert!(format!("{request_error:#}").contains("deadline"));
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "an idempotent deadline trip must restart-and-retry exactly once"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn overall_deadline_bounds_instruct_during_hung_port_callback() {
    let path = unique_socket_path("deadline-callback-instruct");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    let instructs = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            stall_call_before_res: true,
            connection_count: Some(connections.clone()),
            instruct_count: Some(instructs.clone()),
            ..MockOpts::default()
        },
    );

    let state = Arc::new(RecordingState::default());
    let supervisor = build_with_ports(
        path.clone(),
        Arc::new(RecordingPorts {
            state: state.clone(),
            stall_inference: true,
        }),
        Duration::from_millis(300),
    );
    let started = std::time::Instant::now();
    let error = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect_err("a callback that never returns must trip the overall deadline");

    // The hung port callback could not suspend the deadline: the request
    // ended promptly instead of pinning the connection on the provider.
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the overall deadline must bound callback servicing; waited {:?}",
        started.elapsed()
    );

    // Deadline expiry rides the same path as a read-timeout expiry, so the
    // non-idempotent op keeps its fail-fast MaybeApplied contract.
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::MaybeApplied { op, .. } if op == "instruct"),
        "expected MaybeApplied for the deadline on a non-idempotent op, got: {request_error:?}"
    );
    assert!(format!("{request_error:#}").contains("deadline"));
    assert_eq!(
        instructs.load(Ordering::SeqCst),
        1,
        "the deadline must not cause a silent replay of the instruct"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "failing fast on the deadline must not respawn to replay the instruct"
    );
    // The callback was dispatched into the host ports exactly once.
    assert_eq!(state.inference_tiers.lock().unwrap().len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn overall_deadline_bounds_status_during_hung_port_callback() {
    let path = unique_socket_path("deadline-callback-status");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            stall_call_before_res: true,
            connection_count: Some(connections.clone()),
            ..MockOpts::default()
        },
    );

    let state = Arc::new(RecordingState::default());
    let supervisor = build_with_ports(
        path.clone(),
        Arc::new(RecordingPorts {
            state,
            stall_inference: true,
        }),
        Duration::from_millis(300),
    );
    let started = std::time::Instant::now();
    let error = supervisor
        .harness_status()
        .await
        .expect_err("a hung callback must trip the deadline on both attempts");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "both attempts together must stay bounded; waited {:?}",
        started.elapsed()
    );

    // The deadline surfaces as the typed transport error, so the idempotent
    // op kept restart-and-retry-once: exactly one respawn happened.
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::Transport(_)),
        "expected the typed transport deadline error, got: {request_error:?}"
    );
    assert!(format!("{request_error:#}").contains("deadline"));
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "an idempotent deadline trip must restart-and-retry exactly once"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn retry_failure_resets_cache_so_next_request_reestablishes() {
    let path = unique_socket_path("retry-failure-reset");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    let instructs = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            // Both the first attempt AND its retry die mid-status; the third
            // connection behaves.
            die_status_connections: 2,
            connection_count: Some(connections.clone()),
            instruct_count: Some(instructs.clone()),
            ..MockOpts::default()
        },
    );

    let supervisor = build(path.clone(), Arc::new(RecordingState::default()));
    let error = supervisor
        .harness_status()
        .await
        .expect_err("a retry that also dies mid-request must surface an error");
    let request_error = error
        .downcast_ref::<RequestError>()
        .expect("supervisor errors must stay downcastable to RequestError");
    assert!(
        matches!(request_error, RequestError::Transport(_)),
        "expected the transport error from the failed retry, got: {request_error:?}"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "restart-and-retry must have attempted exactly one respawn"
    );

    // The broken replacement connection was reset, not left cached: the next
    // request — the non-idempotent instruct — starts from a clean establish
    // and succeeds, instead of being written into the stale transport and
    // misreading as MaybeApplied.
    let receipt = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect("the request after a failed retry must re-establish and succeed");
    assert_eq!(receipt.instruction_id, "inst-agent-0");
    assert_eq!(
        instructs.load(Ordering::SeqCst),
        1,
        "the instruct must be submitted exactly once, on the fresh connection"
    );
    assert_eq!(
        connections.load(Ordering::SeqCst),
        3,
        "the request after a failed retry must open a fresh connection"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn dead_child_reports_not_running_and_respawns_on_next_request() {
    let path = unique_socket_path("dead-child");
    let listener = UnixListener::bind(&path).unwrap();
    let connections = Arc::new(AtomicU64::new(0));
    spawn_mock_serve(
        listener,
        MockOpts {
            connection_count: Some(connections.clone()),
            ..MockOpts::default()
        },
    );

    let spawned_pids = Arc::new(Mutex::new(Vec::new()));
    let ports: Arc<dyn HostPorts> = Arc::new(RecordingPorts {
        state: Arc::new(RecordingState::default()),
        stall_inference: false,
    });
    let supervisor = MedullaSupervisor::new(
        Arc::new(ChildSpawningConnector {
            path: path.clone(),
            spawned_pids: spawned_pids.clone(),
        }),
        ports,
    );

    supervisor.ensure().await.expect("handshake should succeed");
    assert!(
        supervisor.snapshot().await.running,
        "a live supervised child must report running"
    );

    // Kill the supervised child externally, leaving the cached transport
    // untouched — the exact shape of a child dying between requests.
    let pid = spawned_pids.lock().unwrap()[0];
    let killed = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill must run");
    assert!(killed.success(), "kill -9 must reach the stand-in child");

    // Signal delivery is asynchronous: poll until the snapshot notices.
    let mut status = supervisor.snapshot().await;
    let poll_started = std::time::Instant::now();
    while status.running && poll_started.elapsed() < Duration::from_secs(5) {
        tokio::time::sleep(Duration::from_millis(20)).await;
        status = supervisor.snapshot().await;
    }
    assert!(
        !status.running,
        "a dead child must not be reported as running from the cached handshake"
    );
    assert!(
        status
            .message
            .as_deref()
            .is_some_and(|message| message.contains("exited")),
        "the status must say the child exited, got: {:?}",
        status.message
    );

    // The cache moved to the restartable state: the next request respawns a
    // fresh child instead of writing into the dead one — in particular the
    // non-idempotent instruct succeeds rather than misreporting MaybeApplied.
    let receipt = supervisor
        .instruct("reconcile", json!({}))
        .await
        .expect("a request after child death must respawn and succeed");
    assert_eq!(receipt.instruction_id, "inst-agent-0");
    assert_eq!(
        connections.load(Ordering::SeqCst),
        2,
        "child death must lead to exactly one respawn on the next request"
    );
    assert_eq!(spawned_pids.lock().unwrap().len(), 2);
    assert!(
        supervisor.snapshot().await.running,
        "the respawned child must report running again"
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

#[test]
fn config_fingerprint_is_stable_across_map_insertion_orders() {
    use crate::openhuman::config::schema::TeamModelConfig;

    // `Config` carries HashMap-backed fields whose serde emission order is
    // unstable (per-instance hash seeds). Two configs holding the SAME team
    // pins inserted in opposite orders must fingerprint identically — a
    // mismatch would spuriously kill and respawn the supervised child.
    let team = |model: &str| TeamModelConfig {
        lead_model: Some(model.to_string()),
        agent_model: None,
    };
    let mut config_a = crate::openhuman::config::Config::default();
    for name in ["alpha", "beta", "gamma", "delta", "epsilon"] {
        config_a.teams.insert(name.to_string(), team(name));
    }
    let mut config_b = crate::openhuman::config::Config::default();
    for name in ["epsilon", "delta", "gamma", "beta", "alpha"] {
        config_b.teams.insert(name.to_string(), team(name));
    }

    assert_eq!(
        config_fingerprint(&config_a).unwrap(),
        config_fingerprint(&config_b).unwrap(),
        "identical configs must fingerprint identically regardless of map order"
    );

    // And a genuinely different map value still changes the fingerprint.
    let mut config_c = config_a.clone();
    config_c.teams.insert("alpha".to_string(), team("changed"));
    assert_ne!(
        config_fingerprint(&config_a).unwrap(),
        config_fingerprint(&config_c).unwrap()
    );
}

#[test]
fn canonical_json_hash_ignores_object_key_order() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let hash = |value: &Value| {
        let mut hasher = DefaultHasher::new();
        hash_canonical_json(value, &mut hasher);
        hasher.finish()
    };

    // serde_json is built with `preserve_order` in this workspace, so these
    // two literals genuinely serialize with different key orders.
    let ab = json!({ "a": 1, "b": { "x": [1, 2], "y": null } });
    let ba = json!({ "b": { "y": null, "x": [1, 2] }, "a": 1 });
    assert_eq!(hash(&ab), hash(&ba), "key order must not affect the hash");

    // Same keys, different value → different hash.
    let changed = json!({ "a": 2, "b": { "x": [1, 2], "y": null } });
    assert_ne!(hash(&ab), hash(&changed));
    // Array order stays significant.
    let reordered_array = json!({ "a": 1, "b": { "x": [2, 1], "y": null } });
    assert_ne!(hash(&ab), hash(&reordered_array));
}
