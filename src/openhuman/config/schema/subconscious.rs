//! Subconscious engine selection (plan §5.2 — the openhuman subconscious
//! replacement draft).
//!
//! `subconscious.engine` chooses which cognition drives the heartbeat tick's
//! observe/reflect/commit cycle:
//!
//! * `local` (default) — the existing local tinyagents graph. Unchanged.
//! * `medulla` — route each tick through a supervised local `medulla-serve`
//!   child via `openhuman::medulla_local`. Draft; only wired when the crate is
//!   built with the `medulla-local` feature.
//!
//! The default is `local`, so a config that omits the `[subconscious]` block —
//! every config today — behaves exactly as before.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which engine runs the subconscious reflect/commit cognition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SubconsciousEngine {
    /// The local tinyagents subconscious graph (unchanged default).
    #[default]
    Local,
    /// Route ticks through a local `medulla-serve` child (draft).
    Medulla,
}

impl SubconsciousEngine {
    /// Whether ticks should route through the local medulla brain.
    pub fn is_medulla(self) -> bool {
        matches!(self, Self::Medulla)
    }
}

/// Settings for the supervised local `medulla-serve` child.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MedullaLocalConfig {
    /// Path to medulla-v1's built serve entry (`dist/serve/index.js`). Empty
    /// resolves to the in-repo dev default via [`Self::resolved_serve_entry`].
    #[serde(default)]
    pub serve_entry: String,
}

/// The in-repo dev default for the serve entry — resolvable on a developer
/// checkout of the umbrella workspace. Shipping builds set an explicit path.
const DEV_SERVE_ENTRY: &str =
    "/Users/enamakel/work/tinyhumansai/workflow-medulla/medulla-v1/dist/serve/index.js";

impl Default for MedullaLocalConfig {
    fn default() -> Self {
        Self {
            serve_entry: String::new(),
        }
    }
}

impl MedullaLocalConfig {
    /// The configured serve entry, falling back to the dev default when unset.
    pub fn resolved_serve_entry(&self) -> std::path::PathBuf {
        let trimmed = self.serve_entry.trim();
        if trimmed.is_empty() {
            std::path::PathBuf::from(DEV_SERVE_ENTRY)
        } else {
            std::path::PathBuf::from(trimmed)
        }
    }
}

/// The `[subconscious]` config block.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SubconsciousConfig {
    /// Which engine drives the subconscious tick. Default `local`.
    #[serde(default)]
    pub engine: SubconsciousEngine,
    /// Local `medulla-serve` child settings (only used when `engine = medulla`).
    #[serde(default)]
    pub medulla_local: MedullaLocalConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_engine_is_local() {
        assert_eq!(
            SubconsciousConfig::default().engine,
            SubconsciousEngine::Local
        );
        assert!(!SubconsciousConfig::default().engine.is_medulla());
    }

    #[test]
    fn missing_block_deserializes_to_local() {
        let config: SubconsciousConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(config.engine, SubconsciousEngine::Local);
    }

    #[test]
    fn engine_serde_round_trip() {
        assert_eq!(
            serde_json::to_string(&SubconsciousEngine::Medulla).unwrap(),
            r#""medulla""#
        );
        assert_eq!(
            serde_json::from_str::<SubconsciousEngine>(r#""local""#).unwrap(),
            SubconsciousEngine::Local
        );
    }

    #[test]
    fn serve_entry_falls_back_to_dev_default() {
        let empty = MedullaLocalConfig::default();
        assert!(empty
            .resolved_serve_entry()
            .ends_with("dist/serve/index.js"));
        let custom = MedullaLocalConfig {
            serve_entry: "/tmp/serve.js".to_string(),
        };
        assert_eq!(
            custom.resolved_serve_entry(),
            std::path::PathBuf::from("/tmp/serve.js")
        );
    }
}
