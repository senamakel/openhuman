//! Input shapes for archivist.
//!
//! Two distinct types because they cover two distinct flows:
//!
//! - [`Turn`]         — input to the batch `archive_to_tree` flow
//!                      (clip-and-push-to-tree).
//! - [`ArchivedTurn`] — per-turn capture record persisted as a single md
//!                      file under `<content_root>/episodic/<session>/<seq>.md`.
//!                      Mirrors the legacy `unified::fts5::EpisodicEntry` so
//!                      the harness archivist can dual-write while we
//!                      migrate off FTS5.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One per-turn capture record persisted by [`crate::openhuman::memory_archivist::store::record_turn`].
/// Field names match the legacy `EpisodicEntry` so the harness archivist
/// can call into both surfaces with the same payload during the
/// migration window.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchivedTurn {
    pub session_id: String,
    /// Per-session sequence number, assigned by `record_turn` on write.
    pub seq: u32,
    /// Wall-clock timestamp of the turn (epoch milliseconds).
    pub timestamp_ms: i64,
    /// `"user"` / `"assistant"` / `"system"` / `"tool"`.
    pub role: String,
    pub content: String,
    /// Optional post-turn lesson (kept verbatim from the harness).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson: Option<String>,
    /// Serialized tool-call payload, when the turn issued any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls_json: Option<String>,
    /// Cost in microdollars; 0 when not yet billed.
    #[serde(default)]
    pub cost_microdollars: u64,
}

/// One conversation turn. `tool_calls_json` carries the raw model-side
/// tool-call payload when present; [`crate::openhuman::memory_archivist::clean_conversation`]
/// strips it before the turn lands in the tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    /// `"user"` / `"assistant"` / `"system"` / `"tool"` — free-form so we
    /// don't fight any specific harness's role taxonomy.
    pub role: String,
    /// Natural-language body.
    pub content: String,
    /// Raw JSON of any tool invocations the turn issued. Dropped during
    /// clipping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls_json: Option<String>,
    /// Wall-clock timestamp the turn occurred. Used as the tree leaf
    /// timestamp.
    pub timestamp: DateTime<Utc>,
}

impl Turn {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_calls_json: None,
            timestamp: Utc::now(),
        }
    }
}
