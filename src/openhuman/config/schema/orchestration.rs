//! Orchestration configuration — controls the tiny.place harness session
//! ingest layer.
//!
//! Consumed by [`crate::openhuman::orchestration`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn default_enabled() -> bool {
    true
}

fn default_debounce_ms() -> u64 {
    750
}

fn default_max_supersteps() -> u32 {
    12
}

fn default_message_window() -> u32 {
    40
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct OrchestrationConfig {
    /// Ingest inbound tiny.place harness session DMs into the orchestration
    /// store. Default: `true`.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Coalesce a burst of DMs for one session into a single graph run: after a
    /// session message lands, wait this many milliseconds for the burst to
    /// settle before invoking the wake graph. Default: `750`.
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,

    /// Hard ceiling on graph super-steps for one wake cycle — the loop-continuity
    /// backstop (spec §5). The frontend ⇄ reasoning cycle must terminate on
    /// `channel_response`; this cap forces a terminal DM if it ever does not.
    /// Default: `12`.
    #[serde(default = "default_max_supersteps")]
    pub max_supersteps: u32,

    /// How many of the most recent persisted messages the `normalize` node folds
    /// into `OrchestrationState.messages` for a wake cycle. Default: `40`.
    #[serde(default = "default_message_window")]
    pub message_window: u32,
}

impl Default for OrchestrationConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            debounce_ms: default_debounce_ms(),
            max_supersteps: default_max_supersteps(),
            message_window: default_message_window(),
        }
    }
}
