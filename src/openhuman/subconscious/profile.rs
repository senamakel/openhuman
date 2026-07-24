//! The `SubconsciousProfile` contract — one "world" a subconscious can be
//! instantiated over.
//!
//! A profile is the injected runtime for the generic [`super::instance`] tick
//! graph: its methods are the graph's nodes (observe → prepare_context →
//! reflect → commit). The runner owns everything world-agnostic (tick lock,
//! generation/supersede, timeout, provider gate + rate-cap halt, status); the
//! profile owns only what differs between worlds. Adding a new world is a new
//! profile file, not another engine.
//!
//! The shipped `memory` profile observes the user's connected sources and
//! reflects through hosted Medulla or its local decision-agent fallback.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::openhuman::agent::turn_origin::TrustedAutomationSource;
use crate::openhuman::config::Config;

/// What a profile's `observe` stage found changed in its world since the last
/// baseline. Serde so it can ride in the tick graph's checkpointed state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// Rendered world diff handed to `reflect` (empty when quiet).
    pub rendered: String,
    /// Whether this window is worth a reflection turn. `false` short-circuits
    /// the runner's quiet path (no LLM call — quiet ticks cost nothing).
    pub has_changes: bool,
    /// Whether the change window contains third-party content — the taint input
    /// for [`SubconsciousProfile::origin`].
    pub has_external_content: bool,
    /// Opaque token `commit` can use to advance exactly the observed window.
    /// The memory profile currently re-checkpoints the whole world instead.
    pub commit_token: Option<String>,
}

/// The outcome of a profile's `reflect` stage. Serde so it can ride in the tick
/// graph's checkpointed state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reflection {
    /// The decision agent acted through tools (memory profile).
    Acted { response_chars: usize },
    /// The model looked and correctly chose to do nothing.
    Idle,
}

/// One "world" a subconscious can be instantiated over. See the module docs.
#[async_trait::async_trait]
pub trait SubconsciousProfile: Send + Sync {
    /// Stable instance id — store-key namespace, log prefix, RPC name.
    /// (`"memory"`.)
    fn id(&self) -> &'static str;

    /// Tick cadence for this world (the heartbeat fans out on this).
    fn cadence(&self, config: &Config) -> Duration;

    /// Stage 1: what changed in this world since my baseline?
    ///
    /// Infallible by signature: errors and first-ever ticks surface as an empty
    /// observation (`has_changes == false`) so the runner's branch-free quiet
    /// path handles them uniformly. The profile logs its own errors.
    async fn observe(&self, config: &Config) -> Observation;

    /// Stage 2 (optional): grounding context for the reflection turn. The
    /// default returns `""`. Runs only when `obs.has_changes`.
    async fn prepare_context(&self, _config: &Config, _obs: &Observation) -> String {
        String::new()
    }

    /// Stage 3: the reflection turn. Runs only when `obs.has_changes`.
    ///
    /// Returns `Err(String)` on a genuine failure (agent/provider error) so the
    /// runner can classify it (tool-capability, permanent rate cap) and hold the
    /// baseline; a "looked and did nothing" result is `Ok(Reflection::Idle)`.
    async fn reflect(
        &self,
        config: &Config,
        obs: &Observation,
        prepared_context: &str,
    ) -> Result<Reflection, String>;

    /// Advance this world's baseline/cursor. Called by the runner after a quiet
    /// tick (refresh the baseline) and after a successful reflect — never after
    /// a failed or superseded one.
    async fn commit(&self, config: &Config, obs: &Observation);

    /// Turn-origin taint for this observation.
    fn origin(&self, obs: &Observation) -> TrustedAutomationSource;
}
