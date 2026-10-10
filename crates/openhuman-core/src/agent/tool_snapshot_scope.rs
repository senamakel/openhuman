//! Explicit one-turn tool authority over restored transcript declarations.

use std::future::Future;
tokio::task_local! { static FRESH: (); }

/// Run a turn with the current host belt as authoritative. Historical tool
/// declarations remain in the transcript but cannot restore a revoked tool.
/// Host-spawned dispatch tasks must explicitly carry this scope.
pub async fn with_fresh_snapshot<F: Future>(future: F) -> F::Output {
    FRESH.scope((), future).await
}

pub(crate) fn retain_recorded_tools() -> bool {
    FRESH.try_with(|_| ()).is_err()
}
