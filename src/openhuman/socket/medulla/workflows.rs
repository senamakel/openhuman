//! The workflow plane of the medulla harness protocol: advertising this host's
//! saved workflow graphs, and serving the reads (and the one authoring turn)
//! the orchestrator round-trips back to whoever owns them.
//!
//! **Why a trait and not a direct call into `flows`.** Two different hosts embed
//! this transport — openhuman over its SQLite `flows` store, and medulla-public
//! over its own layered JSON workflow store — and they share nothing but the
//! wire shape. So the store side is a host-supplied [`WorkflowBridge`] the host
//! installs once at startup ([`set_workflow_bridge`]), and the engine types stay
//! entirely on the host's side of the seam: a graph, a node-kind catalog and a
//! run list all cross as opaque [`serde_json::Value`], exactly as they do in
//! medulla-v1's `WorkflowCatalog` port. Nothing in this module parses a node, an
//! edge, or a config field.
//!
//! **Why every path answers.** The server correlates a request by `requestId`
//! and waits on a deadline — ten seconds for a read, ten *minutes* for a copilot
//! turn. A dropped request is therefore not a cheap no-op: it burns that whole
//! window and, for `copilot`, holds a promise open for the rest of the cycle. So
//! a missing bridge, an unknown op argument, a bridge error and even a panicking
//! bridge method all become a `medulla:workflow_result` with `ok: false` and a
//! readable message. The only way to stay silent is for the socket to be gone,
//! which the server already handles by retiring the waiter on disconnect.

use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use async_trait::async_trait;
use futures::FutureExt;
use parking_lot::RwLock;
use serde_json::Value;

use super::payloads::{
    CopilotOutcome, RegisterWorkflows, WorkflowDescriptor, WorkflowOp, WorkflowRequest,
    WorkflowResult, EVENT_REGISTER_WORKFLOWS, EVENT_WORKFLOW_RESULT,
};

/// The store side of the workflow plane, supplied by the embedding host.
///
/// The reads are synchronous because every host backs them with a local store;
/// they are dispatched on a blocking thread so a slow store cannot stall the
/// socket runtime. `copilot` is `async` because it is not a read at all — it is
/// a whole agent turn on the host's own authoring copilot.
///
/// Every method returns `Result<_, String>` rather than a host error type: the
/// error is going straight onto a model's prompt surface as a tool result, so it
/// must already be a sentence a reader can act on.
#[async_trait]
pub trait WorkflowBridge: Send + Sync {
    /// The adverts this host publishes. Cheap and side-effect free — it is
    /// called on every (re)connect and on every store change.
    fn list(&self) -> Vec<WorkflowDescriptor>;

    /// One workflow's detail, graph included, as the host's own JSON shape.
    fn get(&self, id: &str) -> Result<Value, String>;

    /// The node-kind catalog, optionally narrowed to a single `kind`. This is an
    /// authoring aid: a manager reads it to brief the copilot accurately.
    fn node_kinds(&self, kind: Option<&str>) -> Result<Value, String>;

    /// Recent runs of one workflow, for visibility into a delegated graph.
    fn runs(&self, id: &str) -> Result<Value, String>;

    /// Run one authoring turn on the host's copilot. `workflow_id` absent means
    /// "create a new workflow". The outcome must be derived from a re-read of
    /// the host's own store rather than from the model's claim about what it
    /// did — that is the accountability property the whole delegation rests on.
    async fn copilot(
        &self,
        instruction: &str,
        workflow_id: Option<&str>,
    ) -> Result<CopilotOutcome, String>;

    /// Which roster agent owns these workflows, when the host runs more than
    /// one identity. Stamped onto the advert batch so the backend can route a
    /// delegated workflow to its owner. Defaults to unset — the backend then
    /// treats the whole socket as the owner.
    fn agent_id(&self) -> Option<String> {
        None
    }
}

/// The installed bridge. A plain `RwLock<Option<_>>` rather than a `OnceLock`
/// because a host may install its bridge after the socket is already up (the
/// store is initialized lazily), and tests need to swap it.
static BRIDGE: RwLock<Option<Arc<dyn WorkflowBridge>>> = RwLock::new(None);

/// Install (or replace) the host's workflow bridge, and advertise what it holds.
///
/// Registering re-advertises immediately so a bridge installed after connect
/// still reaches a backend that has already seen this socket's `ready`.
pub fn set_workflow_bridge(bridge: Arc<dyn WorkflowBridge>) {
    *BRIDGE.write() = Some(bridge);
    emit_register_workflows();
}

/// Remove the installed bridge and tell the backend this socket now advertises
/// nothing. Sending an empty batch is the only way to retract: the backend
/// replaces a socket's whole entry on each registration and has no delete event.
pub fn clear_workflow_bridge() {
    *BRIDGE.write() = None;
    emit_batch(RegisterWorkflows {
        workflows: Vec::new(),
        agent_id: None,
    });
}

fn bridge() -> Option<Arc<dyn WorkflowBridge>> {
    BRIDGE.read().clone()
}

/// Emit `medulla:register_workflows` — the advert batch, sent on connect and
/// whenever the host signals its workflow set changed.
///
/// A host without a bridge stays silent rather than advertising an empty list:
/// "I have no workflow plane" and "my store is empty" are different claims, and
/// only the second one should clear a previously registered set.
pub fn emit_register_workflows() {
    let Some(bridge) = bridge() else {
        log::debug!("[medulla] no workflow bridge installed — not advertising workflows");
        return;
    };
    // `list()` reads the host's store, so keep it off the socket runtime; a
    // panicking bridge surfaces as a `JoinError` here instead of unwinding.
    tokio::spawn(async move {
        match read_batch(bridge).await {
            Some(batch) => {
                log::info!(
                    "[medulla] advertising {} workflows to backend",
                    batch.workflows.len()
                );
                emit_batch(batch);
            }
            None => log::warn!("[medulla] workflow bridge list() failed — not advertising"),
        }
    });
}

/// The adverts this host currently publishes, for callers that need the list
/// itself rather than the registration frame — today the capability probe, which
/// carries the same descriptors in its reply bag. Empty when no bridge is
/// installed or the bridge failed, so a probe still answers.
pub async fn advertised_workflows() -> Vec<WorkflowDescriptor> {
    let Some(bridge) = bridge() else {
        return Vec::new();
    };
    read_batch(bridge)
        .await
        .map(|batch| batch.workflows)
        .unwrap_or_default()
}

/// Read one advert batch off the bridge on a blocking thread. `None` when the
/// bridge panicked or its thread was cancelled — never a partial batch.
async fn read_batch(bridge: Arc<dyn WorkflowBridge>) -> Option<RegisterWorkflows> {
    tokio::task::spawn_blocking(move || RegisterWorkflows {
        workflows: bridge.list(),
        agent_id: bridge.agent_id(),
    })
    .await
    .map_err(|err| log::warn!("[medulla] workflow bridge list() failed: {err}"))
    .ok()
}

fn emit_batch(batch: RegisterWorkflows) {
    super::emit(EVENT_REGISTER_WORKFLOWS, batch);
}

/// Handle an inbound `medulla:workflow_request`.
///
/// Returns immediately: the work runs on its own task so a `copilot` turn (up to
/// ten minutes of agent time) cannot hold the socket read loop, the same shape
/// `medulla:task_run` and `orch:tool_call` already use.
pub fn handle_workflow_request(request: WorkflowRequest) {
    tokio::spawn(async move {
        let request_id = request.request_id.clone();
        let op = request.op;
        let outcome = match bridge() {
            Some(bridge) => dispatch(bridge, request).await,
            None => Err("this agent has no workflow store installed".to_string()),
        };
        match &outcome {
            Ok(_) => log::debug!("[medulla] workflow {op:?} request_id={request_id} ok"),
            Err(err) => {
                log::warn!("[medulla] workflow {op:?} request_id={request_id} failed: {err}")
            }
        }
        super::emit(EVENT_WORKFLOW_RESULT, result_frame(request_id, outcome));
    });
}

/// Answer a `medulla:workflow_request` this build could not decode.
///
/// A frame with an unknown `op` (a newer backend) or a missing field would
/// otherwise be dropped, and a dropped request costs the backend the op's whole
/// deadline. The `requestId` is recovered from the raw JSON — without one there
/// is nothing to correlate, so the frame really is unanswerable and is only
/// logged.
pub fn reject_unparsed_request(raw: &Value, reason: &str) {
    let Some(request_id) = raw.get("requestId").and_then(Value::as_str) else {
        log::warn!("[medulla] undecodable workflow_request carries no requestId — cannot answer");
        return;
    };
    super::emit(
        EVENT_WORKFLOW_RESULT,
        result_frame(
            request_id.to_string(),
            Err(format!("this agent could not read the request: {reason}")),
        ),
    );
}

/// Project a dispatch outcome onto the wire frame. Split out so the mapping
/// (which arm is populated, and that exactly one of them is) is directly
/// testable without a socket or a runtime.
fn result_frame(request_id: String, outcome: Result<Value, String>) -> WorkflowResult {
    match outcome {
        Ok(data) => WorkflowResult {
            request_id,
            ok: true,
            data: Some(data),
            error: None,
        },
        Err(error) => WorkflowResult {
            request_id,
            ok: false,
            data: None,
            error: Some(error),
        },
    }
}

/// Route one request to `bridge`. Every failure mode — a missing op argument, a
/// bridge error, a panicking bridge method — comes back as an `Err(String)` the
/// caller turns into `ok: false`.
///
/// The bridge is a parameter rather than read from the registry here so the
/// whole dispatch table is exercisable against a scripted bridge without
/// touching process-global state.
async fn dispatch(
    bridge: Arc<dyn WorkflowBridge>,
    request: WorkflowRequest,
) -> Result<Value, String> {
    match request.op {
        WorkflowOp::Get => {
            let id = require_workflow_id(&request, "get")?;
            blocking(move || bridge.get(&id)).await
        }
        WorkflowOp::NodeKinds => {
            let kind = request.kind.clone();
            blocking(move || bridge.node_kinds(kind.as_deref())).await
        }
        WorkflowOp::Runs => {
            let id = require_workflow_id(&request, "runs")?;
            blocking(move || bridge.runs(&id)).await
        }
        WorkflowOp::Copilot => {
            let instruction = request
                .instruction
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "workflow copilot requires a non-empty instruction".to_string())?;
            let turn = bridge.copilot(instruction, request.workflow_id.as_deref());
            let outcome = catching_panics(turn).await??;
            serde_json::to_value(outcome)
                .map_err(|err| format!("failed to serialize the copilot outcome: {err}"))
        }
    }
}

/// The `workflowId` an op needs, or a message naming what was missing. Blank is
/// treated as absent: an empty id could only ever miss in the host's store, and
/// saying so up front beats a "not found" the reader cannot interpret.
fn require_workflow_id(request: &WorkflowRequest, op: &str) -> Result<String, String> {
    request
        .workflow_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("workflow {op} requires a workflowId"))
}

/// Run a synchronous bridge read on a blocking thread, mapping a panic inside it
/// to an error rather than letting it unwind through the task.
async fn blocking<F>(read: F) -> Result<Value, String>
where
    F: FnOnce() -> Result<Value, String> + Send + 'static,
{
    match tokio::task::spawn_blocking(read).await {
        Ok(result) => result,
        Err(err) => Err(format!("the workflow store failed to answer: {err}")),
    }
}

/// The async twin of [`blocking`]: run a bridge turn that must be awaited on the
/// socket runtime — today only `copilot` — and map a panic inside it to an error.
///
/// `copilot` cannot go on a blocking thread (it is a whole agent turn, not a
/// store read), so it does not inherit `spawn_blocking`'s `JoinError` net. Left
/// bare, a panicking `copilot` would abort the task that owes the backend a
/// `workflow_result` and cost it the op's whole ten-minute deadline. Caught
/// in-place rather than on a supervised child task so cancelling the answering
/// task still cancels the turn.
async fn catching_panics<F, T>(turn: F) -> Result<T, String>
where
    F: Future<Output = T>,
{
    match AssertUnwindSafe(turn).catch_unwind().await {
        Ok(value) => Ok(value),
        Err(panic) => {
            let reason = panic
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("unknown panic");
            log::error!("[medulla] workflow copilot panicked: {reason}");
            Err(format!(
                "the workflow store failed to answer: the copilot panicked: {reason}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A bridge whose every read is scripted, so the dispatch table can be
    /// exercised without a store.
    #[derive(Default)]
    struct FakeBridge {
        fail: bool,
        panic_on_get: bool,
        panic_on_copilot: bool,
    }

    #[async_trait]
    impl WorkflowBridge for FakeBridge {
        fn list(&self) -> Vec<WorkflowDescriptor> {
            vec![WorkflowDescriptor {
                id: "wf-1".into(),
                name: "Deploy".into(),
                description: String::new(),
                node_count: 3,
                enabled: None,
                trigger_kind: None,
                agent_id: None,
                workspace_id: None,
            }]
        }

        fn get(&self, id: &str) -> Result<Value, String> {
            if self.panic_on_get {
                panic!("store exploded");
            }
            if self.fail {
                return Err("no such workflow".into());
            }
            Ok(json!({ "id": id, "graph": [] }))
        }

        fn node_kinds(&self, kind: Option<&str>) -> Result<Value, String> {
            Ok(json!({ "kind": kind }))
        }

        fn runs(&self, id: &str) -> Result<Value, String> {
            Ok(json!({ "runs": [id] }))
        }

        async fn copilot(
            &self,
            instruction: &str,
            workflow_id: Option<&str>,
        ) -> Result<CopilotOutcome, String> {
            if self.panic_on_copilot {
                panic!("copilot exploded");
            }
            if self.fail {
                return Err("copilot refused".into());
            }
            Ok(CopilotOutcome {
                reply: instruction.to_string(),
                changes: vec![],
                created: workflow_id.is_none().then(|| "wf-new".to_string()),
            })
        }

        fn agent_id(&self) -> Option<String> {
            Some("orchestrator".into())
        }
    }

    /// A scripted bridge, handed straight to `dispatch` — the tests never touch
    /// the process-global registry, so they stay order- and thread-independent.
    fn bridge_of(fail: bool, panic_on_get: bool) -> Arc<dyn WorkflowBridge> {
        Arc::new(FakeBridge {
            fail,
            panic_on_get,
            ..FakeBridge::default()
        })
    }

    /// A bridge whose authoring turn panics mid-`await` — the async twin of
    /// `panic_on_get`, and the one arm `blocking()` does not cover.
    fn panicking_copilot_bridge() -> Arc<dyn WorkflowBridge> {
        Arc::new(FakeBridge {
            panic_on_copilot: true,
            ..FakeBridge::default()
        })
    }

    fn request(op: WorkflowOp) -> WorkflowRequest {
        WorkflowRequest {
            request_id: "r1".into(),
            op,
            workflow_id: None,
            kind: None,
            instruction: None,
            agent_id: None,
        }
    }

    #[test]
    fn result_frame_populates_exactly_one_arm() {
        let ok = result_frame("r1".into(), Ok(json!({ "a": 1 })));
        assert!(ok.ok);
        assert_eq!(ok.data, Some(json!({ "a": 1 })));
        assert!(ok.error.is_none());

        let failed = result_frame("r1".into(), Err("boom".into()));
        assert!(!failed.ok);
        assert!(failed.data.is_none());
        assert_eq!(failed.error.as_deref(), Some("boom"));
    }

    #[test]
    fn missing_workflow_id_is_reported_not_guessed() {
        let mut req = request(WorkflowOp::Get);
        assert_eq!(
            require_workflow_id(&req, "get"),
            Err("workflow get requires a workflowId".to_string())
        );
        // Blank is absent, not an id that will merely miss in the store.
        req.workflow_id = Some("   ".into());
        assert!(require_workflow_id(&req, "get").is_err());
        req.workflow_id = Some(" wf-1 ".into());
        assert_eq!(require_workflow_id(&req, "get"), Ok("wf-1".to_string()));
    }

    #[tokio::test]
    async fn dispatch_routes_each_read_to_the_bridge() {
        let bridge = bridge_of(false, false);
        let mut get = request(WorkflowOp::Get);
        get.workflow_id = Some("wf-1".into());
        assert_eq!(
            dispatch(Arc::clone(&bridge), get).await.unwrap(),
            json!({ "id": "wf-1", "graph": [] })
        );

        let mut kinds = request(WorkflowOp::NodeKinds);
        kinds.kind = Some("agent".into());
        assert_eq!(
            dispatch(Arc::clone(&bridge), kinds).await.unwrap(),
            json!({ "kind": "agent" })
        );

        let mut runs = request(WorkflowOp::Runs);
        runs.workflow_id = Some("wf-1".into());
        assert_eq!(
            dispatch(bridge, runs).await.unwrap(),
            json!({ "runs": ["wf-1"] })
        );
    }

    #[tokio::test]
    async fn a_read_missing_its_workflow_id_fails_without_reaching_the_store() {
        let outcome = dispatch(bridge_of(false, true), request(WorkflowOp::Get)).await;
        // `panic_on_get` proves the store was never consulted.
        assert_eq!(
            outcome,
            Err("workflow get requires a workflowId".to_string())
        );
    }

    #[tokio::test]
    async fn copilot_requires_an_instruction_and_reports_creation() {
        let bridge = bridge_of(false, false);
        // A blank instruction never reaches the host's agent.
        let mut blank = request(WorkflowOp::Copilot);
        blank.instruction = Some("  ".into());
        assert!(dispatch(Arc::clone(&bridge), blank).await.is_err());

        let mut create = request(WorkflowOp::Copilot);
        create.instruction = Some("build a deploy flow".into());
        let data = dispatch(bridge, create).await.unwrap();
        assert_eq!(data["reply"], "build a deploy flow");
        assert_eq!(data["created"], "wf-new");
    }

    #[tokio::test]
    async fn a_bridge_error_becomes_a_readable_failure() {
        let bridge = bridge_of(true, false);
        let mut get = request(WorkflowOp::Get);
        get.workflow_id = Some("wf-1".into());
        assert_eq!(
            dispatch(Arc::clone(&bridge), get).await,
            Err("no such workflow".to_string())
        );

        let mut copilot = request(WorkflowOp::Copilot);
        copilot.instruction = Some("change it".into());
        assert_eq!(
            dispatch(bridge, copilot).await,
            Err("copilot refused".to_string())
        );
    }

    #[tokio::test]
    async fn a_panicking_bridge_still_answers() {
        let mut get = request(WorkflowOp::Get);
        get.workflow_id = Some("wf-1".into());
        let outcome = dispatch(bridge_of(false, true), get).await;
        // The request must not be dropped — a dropped one costs the server its
        // whole deadline.
        assert!(outcome
            .unwrap_err()
            .starts_with("the workflow store failed to answer"));
    }

    #[tokio::test]
    async fn a_panicking_copilot_still_answers() {
        let mut copilot = request(WorkflowOp::Copilot);
        copilot.instruction = Some("build a deploy flow".into());
        let outcome = dispatch(panicking_copilot_bridge(), copilot).await;
        // A panic inside the authoring turn must come back as an answer, not
        // abort the task that owes the backend a frame: an unanswered copilot
        // costs the server its whole ten-minute deadline.
        assert!(outcome
            .clone()
            .unwrap_err()
            .starts_with("the workflow store failed to answer"));
        let frame = result_frame("r1".into(), outcome);
        assert!(!frame.ok);
        assert!(frame.error.is_some());
    }

    #[test]
    fn an_undecodable_frame_without_a_request_id_is_only_logged() {
        // Nothing to correlate, so nothing can be answered — but it must not
        // panic on the socket read path either.
        reject_unparsed_request(&json!({ "op": "apply_ops" }), "unknown variant");
    }
}
