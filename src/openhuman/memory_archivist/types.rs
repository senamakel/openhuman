//! Input shape for archivist.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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
