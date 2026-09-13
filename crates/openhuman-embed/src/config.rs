//! Config sub-facade — the first typed surface, and the proof of the pattern.
//!
//! Every other sub-facade (memory, workflows, chat, medulla, …) follows the
//! shape established here:
//!
//! 1. A borrowed newtype over `&Arc<CoreRuntime>` — zero-cost, no state.
//! 2. Facade-owned serde types, so hosts never name a domain's internal type
//!    and never touch `serde_json::Value`.
//! 3. Two-line methods delegating to [`call`](super::call::call).
//!
//! Config is deliberately first because it is registered under
//! `DomainGroup::Platform`, which every preset enables — so a failure here is
//! unambiguously a facade bug rather than a gating question.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::call::call;
use super::error::CoreError;
use openhuman_core::core::runtime::CoreRuntime;

/// Environment-driven runtime flags.
///
/// Mirrors the core's `RuntimeFlagsOut` (`config/ops/loader.rs`). Declared here
/// rather than re-exported so the facade's wire contract is explicit and a
/// change to the core's internal struct surfaces as a failing round-trip test
/// instead of silently altering the host-facing API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFlags {
    /// `OPENHUMAN_BROWSER_ALLOW_ALL` — browser tools bypass the allowlist.
    pub browser_allow_all: bool,
    /// `OPENHUMAN_LOG_PROMPTS` — full prompts are written to the log.
    pub log_prompts: bool,
}

/// Typed access to persisted and environment configuration.
///
/// Obtained from [`Core::config`](super::Core::config); never constructed
/// directly.
pub struct Config<'a>(pub(super) &'a Arc<CoreRuntime>);

impl Config<'_> {
    /// Read the environment-driven runtime flags.
    ///
    /// Note this method always travels wrapped in the `{"result", "logs"}`
    /// envelope, because its handler emits a log unconditionally. That is
    /// handled in [`call`](super::call::call) and is invisible here — which is
    /// the entire point of routing every method through one helper.
    pub async fn runtime_flags(&self) -> Result<RuntimeFlags, CoreError> {
        call(
            self.0,
            "openhuman.config_get_runtime_flags",
            serde_json::json!({}),
        )
        .await
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
