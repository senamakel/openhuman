//! Canonical archived-turn shape.

use serde::{Deserialize, Serialize};

/// One archived conversation turn. Mirrors the legacy
/// `memory_store::unified::fts5::EpisodicEntry` for migration parity, but
/// rebuilt as a serde type so it can round-trip through YAML front-matter
/// on disk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ArchivedTurn {
    pub session_id: String,
    /// Sequence number within `session_id`. Starts at 0 and increments on
    /// every `record_turn` call for the same session.
    pub seq: u32,
    /// Wall-clock timestamp the turn was captured at (epoch milliseconds).
    pub timestamp_ms: i64,
    /// `"user"` / `"assistant"` / `"system"` / `"tool"` — free-form so the
    /// archivist doesn't fight the harness's role taxonomy.
    pub role: String,
    pub content: String,
    /// Optional lesson the post-turn hook extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lesson: Option<String>,
    /// Serialized tool-call payload (JSON). `None` when the turn issued no
    /// tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls_json: Option<String>,
    /// Cost in microdollars; 0 when not yet billed.
    #[serde(default)]
    pub cost_microdollars: u64,
}
