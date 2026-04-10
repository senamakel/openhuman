//! Apify client facade for the discovery agent.
//!
//! > **Status: stubbed.** The real backend-proxy client has been
//! > removed for now. See the parent `discovery/mod.rs` docs for the
//! > rationale. The [`ApifyClient`] trait and [`StubApifyClient`]
//! > remain in place so downstream code (the runner, tests, tool
//! > dispatch) can keep compiling unchanged; when real Apify wiring
//! > returns, only this file and `bus.rs::trigger_discovery` need to
//! > change.
//!
//! The stub always returns `Ok` with an empty dataset and logs a
//! warning so any accidental invocation is visible in logs.

use async_trait::async_trait;
use serde_json::Value;

/// Errors surfaced from the Apify facade.
#[derive(Debug, thiserror::Error)]
pub enum ApifyError {
    #[error("apify proxy request failed: {0}")]
    Http(String),
    #[error("apify proxy returned non-SUCCEEDED status: {0}")]
    NonSuccess(String),
    #[error("apify proxy response was missing expected field '{0}'")]
    MissingField(&'static str),
}

/// Result of a single Apify actor run.
///
/// Shape matches the backend's `ApifyRunControllerResponse` envelope —
/// kept as-is so the real HTTP client can slot back in without
/// touching callers.
#[derive(Debug, Clone)]
pub struct ApifyRunResult {
    /// Apify run id (opaque).
    pub run_id: String,
    /// Actor id that was executed.
    pub actor_id: String,
    /// Apify run status — `"SUCCEEDED"` when the stub resolves.
    pub status: String,
    /// Dataset items returned by the actor (always empty for the stub).
    pub items: Vec<Value>,
}

/// Trait wrapping an Apify actor runner. Object-safe so it can be
/// stored as `Arc<dyn ApifyClient>` and swapped out for tests.
#[async_trait]
pub trait ApifyClient: Send + Sync {
    /// Run an Apify actor synchronously and return its dataset items.
    ///
    /// `input` is forwarded verbatim into the actor's input blob.
    /// `timeout_secs` is the run timeout (max 3600 in the real impl).
    async fn run_actor(
        &self,
        actor_id: &str,
        input: Value,
        timeout_secs: u32,
    ) -> Result<ApifyRunResult, ApifyError>;
}

/// No-op Apify client used while the real backend-proxy integration
/// is on hold. Every `run_actor` call logs a warning and returns an
/// empty dataset with `status = "SUCCEEDED"` so the discovery runner
/// keeps making forward progress instead of erroring out.
#[derive(Debug, Default)]
pub struct StubApifyClient;

impl StubApifyClient {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ApifyClient for StubApifyClient {
    async fn run_actor(
        &self,
        actor_id: &str,
        _input: Value,
        _timeout_secs: u32,
    ) -> Result<ApifyRunResult, ApifyError> {
        log::warn!(
            "[discovery:apify] stub invoked for actor '{}' — returning empty dataset",
            actor_id
        );
        Ok(ApifyRunResult {
            run_id: "stub".to_string(),
            actor_id: actor_id.to_string(),
            status: "SUCCEEDED".to_string(),
            items: Vec::new(),
        })
    }
}
