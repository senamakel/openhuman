//! Process-wide cancellation registry for running workflow engine loops.
//!
//! `stop_workflow_run` flips the flag; the engine loop checks it between
//! phases and aborts in-flight child tasks via the orchestration session
//! before marking the run `Interrupted`.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use tinyagents_harness::CancellationToken;

/// Per-run pair of legacy poll flag and crate-native cancellation token.
#[derive(Clone)]
pub(super) struct WorkflowCancelSignal {
    pub(super) flag: Arc<AtomicBool>,
    pub(super) token: CancellationToken,
}

fn cancel_registry() -> &'static Mutex<HashMap<String, WorkflowCancelSignal>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, WorkflowCancelSignal>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register (or reuse) a cancellation flag for `run_id`.
pub(super) fn register_cancel_signal(run_id: &str) -> WorkflowCancelSignal {
    let mut map = cancel_registry().lock().expect("cancel registry poisoned");
    map.entry(run_id.to_string())
        .or_insert_with(|| WorkflowCancelSignal {
            flag: Arc::new(AtomicBool::new(false)),
            token: CancellationToken::new(),
        })
        .clone()
}

/// Register (or reuse) a cancellation flag for `run_id`.
pub(super) fn register_cancel_flag(run_id: &str) -> Arc<AtomicBool> {
    register_cancel_signal(run_id).flag
}

/// Look up an existing cancellation signal for `run_id`, if one is registered.
pub(super) fn lookup_cancel_signal(run_id: &str) -> Option<WorkflowCancelSignal> {
    cancel_registry()
        .lock()
        .expect("cancel registry poisoned")
        .get(run_id)
        .cloned()
}

/// Look up an existing cancellation flag for `run_id`, if one is registered.
pub(super) fn lookup_cancel_flag(run_id: &str) -> Option<Arc<AtomicBool>> {
    lookup_cancel_signal(run_id).map(|signal| signal.flag)
}

/// Look up an SDK cancellation token for `run_id`, if one is registered.
pub(super) fn lookup_cancel_token(run_id: &str) -> Option<CancellationToken> {
    lookup_cancel_signal(run_id).map(|signal| signal.token)
}

/// Drop a run's cancellation flag once the engine loop is done with it.
pub(super) fn clear_cancel_flag(run_id: &str) {
    cancel_registry()
        .lock()
        .expect("cancel registry poisoned")
        .remove(run_id);
}
