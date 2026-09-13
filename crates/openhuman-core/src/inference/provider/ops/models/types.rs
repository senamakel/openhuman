//! `ModelInfo`: the typed representation of one `/models` catalog entry.

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Human-readable name when the listing supplies one (the managed
    /// `?catalog=` listing does; a bare OpenAI-compatible `/models` does not).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Charged price in USD per 1M tokens, when the listing publishes it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_per_1m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_per_1m: Option<f64>,
}
