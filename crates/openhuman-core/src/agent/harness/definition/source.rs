//! Provenance of a definition: built-in, custom TOML file, or the
//! config-backed custom registry.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Where an [`super::AgentDefinition`] was loaded from. Used for telemetry and
/// the `agent::list_definitions` RPC reply.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", content = "path")]
pub enum DefinitionSource {
    /// Built-in definition shipped as part of the binary (loaded from
    /// [`crate::agent::registry::agents`]).
    #[default]
    Builtin,
    /// Loaded from a TOML file at the given absolute path.
    File(PathBuf),
    /// Synthesized at lookup time from a user-authored
    /// [`AgentRegistryEntry`](crate::agent::registry::AgentRegistryEntry)
    /// (`AgentRegistrySource::Custom`) by `agent_registry::defaults::definition_from_registry_entry`.
    /// Never persisted in the [`super::AgentDefinitionRegistry`] — built fresh per
    /// factory call so config edits take effect immediately (closes the gap
    /// where custom agents ran persona-only instead of with their real tool
    /// belt).
    CustomRegistry,
}
