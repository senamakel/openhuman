//! Explicit logout invalidates channel runtimes, independently of model auth.
use std::future::Future;
use std::sync::{LazyLock, Mutex};
use tokio_util::sync::CancellationToken;

static SESSION: LazyLock<Mutex<CancellationToken>> =
    LazyLock::new(|| Mutex::new(CancellationToken::new()));

pub(crate) fn channel_session() -> CancellationToken {
    SESSION.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn invalidate_channel_session() {
    let mut session = SESSION.lock().unwrap_or_else(|e| e.into_inner());
    session.cancel();
    *session = CancellationToken::new();
    tracing::debug!("[channels] invalidated channel runtimes on logout");
}

pub(super) async fn run_in_session(
    session: CancellationToken,
    runtime: impl Future<Output = anyhow::Result<()>>,
) -> anyhow::Result<()> {
    tokio::select! {
        biased;
        _ = session.cancelled() => {
            tracing::info!("[channels] stopping runtime after logout");
            Ok(())
        }
        result = runtime => result,
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
