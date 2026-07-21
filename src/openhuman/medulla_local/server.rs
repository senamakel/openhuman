//! Supervisor for a local `medulla-serve` Node child.
//!
//! Mirrors `runtime_python_server/server.rs` conventions verbatim: a versioned
//! handshake, id-correlated NDJSON, per-request timeout, restart-and-retry-once
//! on transport failure, start-failure backoff, and a drained stderr pipe. The
//! transport is a unix domain socket (§1 of the serve protocol spec): serve
//! listens on `serve.sock` under the workspace state dir and the host connects.
//!
//! The wire is demultiplexed inline (single connection, single reader) exactly
//! like the Python server's response loop — extended to also service the
//! serve→host `call` port callbacks and fold the `event` stream, since medulla
//! runs a reverse-RPC plane the Python backend does not. Port dispatch goes
//! through the [`HostPorts`] seam so the transport stays testable without a
//! live Node process.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::UnixStream;
use tokio::process::{Child, ChildStderr};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::host_ports::OpenhumanHostPorts;
use super::ports::HostPorts;
use super::protocol::{
    ret_err, ret_ok, CallFrame, EventFrame, FrameKind, ReadyLine, ResFrame, ServeError,
    PROTOCOL_VERSION,
};
use super::types::{
    error_codes, HarnessStatus, HelloParams, HelloResult, InferenceCall, InstructReceipt,
    MedullaLocalStatus,
};
use crate::openhuman::config::Config;

/// Ceiling for the `ready` handshake (§7). Matches the Python server.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
/// Ceiling for a single `req`/`ret`-awaiting read. `instruct` returns its
/// receipt fast; the cycle itself is observed via events, not this timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Backoff after a failed spawn before the host retries (§7).
const START_FAILURE_BACKOFF: Duration = Duration::from_secs(300);
/// Poll interval while waiting for the child to create its listening socket.
const SOCKET_CONNECT_POLL: Duration = Duration::from_millis(50);

/// A live, handshaken connection to one serve child.
///
/// Owns the split unix-socket halves, the next-id counter (§2), and the
/// [`HostPorts`] the reverse-RPC plane dispatches to. Dropping it kills the
/// child (the [`Child`] is spawned with `kill_on_drop`).
pub struct Connection {
    writer: OwnedWriteHalf,
    reader: Lines<BufReader<OwnedReadHalf>>,
    next_id: u64,
    ports: Arc<dyn HostPorts>,
    ready: ReadyLine,
    hello: HelloResult,
    /// Kept alive for the connection's lifetime; `None` in tests that connect
    /// to a mock listener instead of spawning Node.
    _child: Option<Child>,
    last_event_seq: Option<u64>,
}

impl Connection {
    /// Read the `ready` banner, negotiate `hello`, and return a live
    /// connection. `child` is retained so the caller can tie the process
    /// lifetime to the connection.
    pub async fn establish(
        stream: UnixStream,
        ports: Arc<dyn HostPorts>,
        hello: HelloParams,
        child: Option<Child>,
    ) -> Result<Self> {
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half).lines();

        let ready_line = match tokio::time::timeout(HANDSHAKE_TIMEOUT, reader.next_line()).await {
            Ok(Ok(Some(line))) => line,
            Ok(Ok(None)) => bail!("medulla serve closed before the ready handshake"),
            Ok(Err(error)) => return Err(error).context("reading medulla serve ready line"),
            Err(_) => bail!("medulla serve ready handshake timed out"),
        };
        let ready: ReadyLine = serde_json::from_str(&ready_line)
            .with_context(|| format!("parsing medulla serve ready line: {ready_line}"))?;
        if let Some(error) = &ready.error {
            bail!("medulla serve reported startup failure: {error}");
        }
        if ready.protocol != PROTOCOL_VERSION {
            bail!(
                "medulla serve protocol mismatch: expected {PROTOCOL_VERSION}, got {}",
                ready.protocol
            );
        }
        info!(
            serve = ready.serve.as_deref().unwrap_or("<unknown>"),
            session = ready.session_id.as_deref().unwrap_or("<none>"),
            capabilities = ?ready.capabilities,
            "[medulla_local] serve ready"
        );

        let mut conn = Self {
            writer: write_half,
            reader,
            next_id: 0,
            ports,
            ready,
            hello: HelloResult::default(),
            _child: child,
            last_event_seq: None,
        };

        let hello_value = serde_json::to_value(&hello).context("encoding medulla hello params")?;
        let negotiated: HelloResult = conn
            .request("hello", hello_value)
            .await
            .context("medulla serve hello handshake failed")?;
        info!(
            ports = ?negotiated.ports,
            "[medulla_local] hello negotiated active port set"
        );
        conn.hello = negotiated;
        Ok(conn)
    }

    /// Typed request (§4): write a `req`, then drive the read loop —
    /// servicing interleaved `call` port callbacks and folding `event`s —
    /// until the correlated `res` arrives.
    pub async fn request<T: DeserializeOwned>(&mut self, op: &str, params: Value) -> Result<T> {
        let value = self.request_raw(op, params).await?;
        serde_json::from_value(value)
            .with_context(|| format!("decoding medulla serve `{op}` result"))
    }

    async fn request_raw(&mut self, op: &str, params: Value) -> Result<Value> {
        let id = self.next_id.to_string();
        self.next_id += 1;
        debug!(id = %id, op, "[medulla_local] sending req");
        self.write_frame(&super::protocol::req_frame(&id, op, params))
            .await?;

        loop {
            let line = self.next_line(REQUEST_TIMEOUT).await?;
            let frame: Value = match serde_json::from_str(&line) {
                Ok(frame) => frame,
                Err(error) => {
                    warn!(
                        "[medulla_local] unparseable frame skipped: {error}; line_len={}",
                        line.len()
                    );
                    continue;
                }
            };
            match FrameKind::of(&frame) {
                FrameKind::Res => {
                    let res: ResFrame = match serde_json::from_value(frame) {
                        Ok(res) => res,
                        Err(error) => {
                            warn!("[medulla_local] malformed res skipped: {error}");
                            continue;
                        }
                    };
                    if res.id.as_deref() != Some(id.as_str()) {
                        debug!(want = %id, got = ?res.id, "[medulla_local] skipping res for other id");
                        continue;
                    }
                    if !res.ok {
                        let message = res
                            .error
                            .map(|e| format!("{}: {}", e.code, e.message))
                            .unwrap_or_else(|| "unknown medulla serve error".to_string());
                        bail!("medulla serve `{op}` failed: {message}");
                    }
                    return Ok(res.result.unwrap_or(Value::Null));
                }
                FrameKind::Call => self.handle_call(frame).await,
                FrameKind::Event => self.fold_event(frame),
                FrameKind::Ready | FrameKind::Unknown => {
                    debug!("[medulla_local] ignoring unexpected inbound frame while awaiting res");
                }
            }
        }
    }

    /// Dispatch one serve→host port `call` to [`HostPorts`] and write the
    /// `ret` (§5). Only `inference` and `tools` are answered this draft; every
    /// other port is refused `port_unavailable` — centralised here so no
    /// implementer can forget the refusal.
    async fn handle_call(&mut self, frame: Value) {
        let call: CallFrame = match serde_json::from_value(frame) {
            Ok(call) => call,
            Err(error) => {
                warn!("[medulla_local] malformed call frame skipped: {error}");
                return;
            }
        };
        debug!(id = %call.id, port = %call.port, method = %call.method, "[medulla_local] port call");

        let ret = match (call.port.as_str(), call.method.as_str()) {
            ("inference", "invoke") => match serde_json::from_value::<InferenceCall>(call.params) {
                Ok(inference_call) => match self.ports.invoke_inference(inference_call).await {
                    Ok(result) => ret_ok(
                        &call.id,
                        serde_json::to_value(result).unwrap_or(Value::Null),
                    ),
                    Err(port_error) => ret_err(&call.id, &port_error.to_serve_error()),
                },
                Err(error) => ret_err(
                    &call.id,
                    &ServeError::new(error_codes::BAD_REQUEST, error.to_string()),
                ),
            },
            // Cancellation is a fresh call id naming the target (§5.1); the
            // draft answers its own ret and lets the in-flight call settle.
            ("inference", "cancel") => ret_ok(&call.id, json!({})),
            ("tools", "invoke") => {
                let name = call.params.get("name").and_then(Value::as_str);
                let args = call
                    .params
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match name {
                    Some(name) => match self.ports.invoke_tool(name, args).await {
                        Ok(result) => ret_ok(&call.id, result),
                        Err(port_error) => ret_err(&call.id, &port_error.to_serve_error()),
                    },
                    None => ret_err(
                        &call.id,
                        &ServeError::new(error_codes::BAD_REQUEST, "tools.invoke missing `name`"),
                    ),
                }
            }
            (port, method) => ret_err(
                &call.id,
                &ServeError::new(
                    error_codes::PORT_UNAVAILABLE,
                    format!("port `{port}.{method}` is not offered by this host (draft)"),
                ),
            ),
        };
        if let Err(error) = self.write_frame(&ret).await {
            warn!(
                "[medulla_local] failed to write ret for call {}: {error}",
                call.id
            );
        }
    }

    /// Fold one `event` frame (§6). Advisory-only for the draft: track the
    /// high-water `seq` and log a gap (which in the full design would trigger a
    /// `subscribe` replay + re-`status`).
    fn fold_event(&mut self, frame: Value) {
        let event: EventFrame = match serde_json::from_value(frame) {
            Ok(event) => event,
            Err(error) => {
                warn!("[medulla_local] malformed event skipped: {error}");
                return;
            }
        };
        if let Some(prev) = self.last_event_seq {
            if event.seq > prev + 1 {
                warn!(
                    prev,
                    seq = event.seq,
                    "[medulla_local] event seq gap — a full host would resync via subscribe(replay)"
                );
            }
        }
        self.last_event_seq = Some(event.seq);
        debug!(seq = event.seq, "[medulla_local] folded event");
    }

    async fn write_frame(&mut self, frame: &Value) -> Result<()> {
        let mut line = serde_json::to_string(frame).context("encoding medulla frame")?;
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .context("writing medulla frame")?;
        self.writer
            .flush()
            .await
            .context("flushing medulla frame")?;
        Ok(())
    }

    async fn next_line(&mut self, timeout: Duration) -> Result<String> {
        match tokio::time::timeout(timeout, self.reader.next_line()).await {
            Ok(Ok(Some(line))) => Ok(line),
            Ok(Ok(None)) => bail!("medulla serve closed the connection"),
            Ok(Err(error)) => Err(error).context("reading medulla serve frame"),
            Err(_) => bail!("medulla serve frame read timed out"),
        }
    }

    /// The `ready`/`hello` state captured at handshake, for status reporting.
    fn status(&self) -> MedullaLocalStatus {
        MedullaLocalStatus {
            enabled: true,
            running: true,
            serve_version: self.ready.serve.clone(),
            session_id: self
                .hello
                .session_id
                .clone()
                .or_else(|| self.ready.session_id.clone()),
            ports: self.hello.ports.clone(),
            message: None,
        }
    }
}

/// A source of live [`Connection`]s. Production spawns a Node child and connects
/// its socket; tests connect to a mock listener. Keeping this a trait is the
/// seam that makes the supervisor's restart-and-retry logic testable without a
/// real process.
#[async_trait]
pub trait Connector: Send + Sync {
    async fn connect(&self, ports: Arc<dyn HostPorts>) -> Result<Connection>;
    /// Human-readable identity for logs.
    fn describe(&self) -> String;
}

/// Production connector: resolves Node via `NodeBootstrap`, spawns medulla-v1's
/// `dist/serve` entry pointed at a unix socket, drains stderr, and connects.
pub struct NodeServeConnector {
    node_bootstrap: Arc<crate::openhuman::runtime_node::NodeBootstrap>,
    /// `None` when neither `subconscious.medulla_local.serve_entry` nor the
    /// `OPENHUMAN_MEDULLA_SERVE_ENTRY` env override is set; `connect` then bails
    /// with an actionable message rather than probing a bogus path.
    serve_entry: Option<PathBuf>,
    socket_path: PathBuf,
    host_identity: String,
}

impl NodeServeConnector {
    pub fn new(
        node_bootstrap: Arc<crate::openhuman::runtime_node::NodeBootstrap>,
        serve_entry: Option<PathBuf>,
        socket_path: PathBuf,
        host_identity: String,
    ) -> Self {
        Self {
            node_bootstrap,
            serve_entry,
            socket_path,
            host_identity,
        }
    }
}

#[async_trait]
impl Connector for NodeServeConnector {
    async fn connect(&self, ports: Arc<dyn HostPorts>) -> Result<Connection> {
        let serve_entry = self.serve_entry.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "medulla serve entry not configured: set subconscious.medulla_local.serve_entry \
                 or the OPENHUMAN_MEDULLA_SERVE_ENTRY env var to medulla-v1's built \
                 `dist/serve/index.js`"
            )
        })?;
        if !serve_entry.is_file() {
            bail!(
                "medulla serve entry not found: {} (build medulla-v1 `dist/serve` or set \
                 subconscious.medulla_local.serve_entry / OPENHUMAN_MEDULLA_SERVE_ENTRY)",
                serve_entry.display()
            );
        }
        let node = self
            .node_bootstrap
            .resolve()
            .await
            .context("resolving Node toolchain for medulla serve")?;

        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating medulla socket dir {}", parent.display()))?;
        }
        // serve `rm`s a stale path before listen; be defensive in case a prior
        // child died without cleanup so `connect` does not attach to a dead
        // socket inode.
        let _ = tokio::fs::remove_file(&self.socket_path).await;

        info!(
            node = %node.node_bin.display(),
            entry = %serve_entry.display(),
            socket = %self.socket_path.display(),
            "[medulla_local] spawning serve child"
        );
        let mut command = tokio::process::Command::new(&node.node_bin);
        command
            .arg(serve_entry)
            .arg("--socket")
            .arg(&self.socket_path)
            .env("PATH", prepend_path(&node.bin_dir))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().context("spawning medulla serve child")?;
        if let Some(stderr) = child.stderr.take() {
            drain_stderr(stderr);
        }

        let stream = connect_unix_retry(&self.socket_path, HANDSHAKE_TIMEOUT).await?;
        // Advertise the curated read-only tool surface so serve binds it into a
        // MedullaModule and the model can emit `tools.invoke` for these tools.
        // The spec set comes from the same `HostPorts` the `tools` port callback
        // dispatches to, so what is advertised is exactly what can be invoked.
        let tools = ports.tool_specs();
        let hello = HelloParams {
            protocol: PROTOCOL_VERSION,
            host: self.host_identity.clone(),
            ports: vec!["inference".to_string(), "tools".to_string()],
            tools,
        };
        Connection::establish(stream, ports, hello, Some(child)).await
    }

    fn describe(&self) -> String {
        format!("node serve @ {}", self.socket_path.display())
    }
}

fn prepend_path(bin_dir: &Path) -> String {
    match std::env::var("PATH") {
        Ok(existing) => format!("{}:{}", bin_dir.display(), existing),
        Err(_) => bin_dir.display().to_string(),
    }
}

/// Connect to the child's listening socket, retrying until it appears or the
/// handshake deadline elapses.
async fn connect_unix_retry(path: &Path, deadline: Duration) -> Result<UnixStream> {
    let start = Instant::now();
    loop {
        match UnixStream::connect(path).await {
            Ok(stream) => return Ok(stream),
            Err(error) => {
                if start.elapsed() >= deadline {
                    return Err(error).with_context(|| {
                        format!(
                            "connecting medulla serve socket {} timed out",
                            path.display()
                        )
                    });
                }
                tokio::time::sleep(SOCKET_CONNECT_POLL).await;
            }
        }
    }
}

/// Drain the child's stderr so a chatty serve never blocks on a full pipe
/// (mirrors `drain_server_stderr`).
fn drain_stderr(stderr: ChildStderr) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            debug!("[medulla_local] serve stderr: {line}");
        }
        debug!("[medulla_local] serve stderr drain closed");
    });
}

/// Supervises one serve connection with restart-and-retry-once semantics.
pub struct MedullaSupervisor {
    connector: Arc<dyn Connector>,
    ports: Arc<dyn HostPorts>,
    connection: Mutex<Option<Connection>>,
}

impl MedullaSupervisor {
    pub fn new(connector: Arc<dyn Connector>, ports: Arc<dyn HostPorts>) -> Self {
        Self {
            connector,
            ports,
            connection: Mutex::new(None),
        }
    }

    /// Ensure a connection exists (lazy spawn + handshake).
    pub async fn ensure(&self) -> Result<()> {
        let mut guard = self.connection.lock().await;
        if guard.is_none() {
            *guard = Some(self.connector.connect(self.ports.clone()).await?);
        }
        Ok(())
    }

    /// Request with restart-and-retry-once (§7): on any transport failure the
    /// host resets the connection, respawns via the connector (which replays
    /// `hello`), and retries the request exactly once.
    pub async fn request<T: DeserializeOwned>(&self, op: &str, params: Value) -> Result<T> {
        match self.request_once(op, params.clone()).await {
            Ok(value) => Ok(value),
            Err(error) => {
                warn!(
                    "[medulla_local] request `{op}` failed; restarting {} before retry: {error:#}",
                    self.connector.describe()
                );
                self.reset().await;
                self.request_once(op, params).await
            }
        }
    }

    async fn request_once<T: DeserializeOwned>(&self, op: &str, params: Value) -> Result<T> {
        let mut guard = self.connection.lock().await;
        if guard.is_none() {
            *guard = Some(self.connector.connect(self.ports.clone()).await?);
        }
        let connection = guard
            .as_mut()
            .context("medulla connection missing after connect")?;
        connection.request(op, params).await
    }

    async fn reset(&self) {
        // Dropping the connection kills the child (`kill_on_drop`).
        let mut guard = self.connection.lock().await;
        *guard = None;
    }

    /// Enqueue one instruction (§4.1). Returns the synchronous receipt.
    pub async fn instruct(&self, message: &str, meta: Value) -> Result<InstructReceipt> {
        self.request("instruct", json!({ "message": message, "meta": meta }))
            .await
    }

    /// Snapshot of `HarnessStatus` (§4.4).
    pub async fn harness_status(&self) -> Result<HarnessStatus> {
        self.request("status", json!({})).await
    }

    /// Non-spawning status snapshot from the currently-cached connection.
    pub async fn snapshot(&self) -> MedullaLocalStatus {
        let guard = self.connection.lock().await;
        match guard.as_ref() {
            Some(connection) => connection.status(),
            None => MedullaLocalStatus {
                enabled: true,
                running: false,
                serve_version: None,
                session_id: None,
                ports: Vec::new(),
                message: Some("medulla serve not connected".to_string()),
            },
        }
    }
}

/// Cached global supervisor + start-failure backoff, mirroring the Python
/// server's `ServerCache`.
enum SupervisorCache {
    Empty,
    Ready(Arc<MedullaSupervisor>),
    Failed {
        message: String,
        retry_after: Instant,
    },
}

static SUPERVISOR: std::sync::OnceLock<Mutex<SupervisorCache>> = std::sync::OnceLock::new();

fn supervisor_slot() -> &'static Mutex<SupervisorCache> {
    SUPERVISOR.get_or_init(|| Mutex::new(SupervisorCache::Empty))
}

/// Resolve (and lazily start) the process-global supervisor for `config`.
pub async fn ensure_started(config: &Config) -> Result<Arc<MedullaSupervisor>> {
    let mut guard = supervisor_slot().lock().await;
    match &*guard {
        SupervisorCache::Ready(existing) => {
            let existing = existing.clone();
            drop(guard);
            existing.ensure().await?;
            return Ok(existing);
        }
        SupervisorCache::Failed {
            message,
            retry_after,
        } if Instant::now() < *retry_after => {
            bail!("medulla serve unavailable after previous startup failure: {message}");
        }
        SupervisorCache::Failed { .. } | SupervisorCache::Empty => {}
    }

    match build_supervisor(config).await {
        Ok(supervisor) => {
            *guard = SupervisorCache::Ready(supervisor.clone());
            Ok(supervisor)
        }
        Err(error) => {
            let message = format!("{error:#}");
            warn!(
                "[medulla_local] startup failed; backing off {:?}: {message}",
                START_FAILURE_BACKOFF
            );
            *guard = SupervisorCache::Failed {
                message: message.clone(),
                retry_after: Instant::now() + START_FAILURE_BACKOFF,
            };
            bail!("medulla serve unavailable: {message}");
        }
    }
}

async fn build_supervisor(config: &Config) -> Result<Arc<MedullaSupervisor>> {
    let node_bootstrap = Arc::new(crate::openhuman::runtime_node::NodeBootstrap::new(
        config.node.clone(),
        config.workspace_dir.clone(),
        reqwest::Client::new(),
    ));
    let serve_entry = config.subconscious.medulla_local.resolved_serve_entry();
    let socket_path = medulla_socket_path(config);
    let host_identity = format!("openhuman/{}", env!("CARGO_PKG_VERSION"));
    let connector = Arc::new(NodeServeConnector::new(
        node_bootstrap,
        serve_entry,
        socket_path,
        host_identity,
    ));
    let ports: Arc<dyn HostPorts> = Arc::new(OpenhumanHostPorts::new(Arc::new(config.clone())));
    let supervisor = Arc::new(MedullaSupervisor::new(connector, ports));
    supervisor.ensure().await?;
    Ok(supervisor)
}

/// `serve.sock` under the workspace state dir (§1 path precedence, second hop —
/// `$XDG_RUNTIME_DIR` is the daemon's concern and left to a later milestone).
pub fn medulla_socket_path(config: &Config) -> PathBuf {
    config.workspace_dir.join("medulla").join("serve.sock")
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
