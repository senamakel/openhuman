//! Wire-format types for `openhuman.skill_registry_*` RPC methods.

use serde::{Deserialize, Serialize};

use crate::openhuman::skill_registry::types::{CatalogEntry, RegistryKind, RegistrySource};

// ── Params ──────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub(super) struct BrowseParams {
    #[serde(default)]
    pub(super) force_refresh: bool,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SearchParams {
    #[serde(default)]
    pub(super) query: String,
    #[serde(default)]
    pub(super) format: Option<String>,
    #[serde(default)]
    pub(super) source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct InstallParams {
    pub(super) entry_id: String,
    pub(super) source_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct AddSourceParams {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) url: String,
    #[serde(default = "default_kind")]
    pub(super) kind: RegistryKind,
}

fn default_kind() -> RegistryKind {
    RegistryKind::GithubIndex
}

#[derive(Debug, Deserialize)]
pub(super) struct RemoveSourceParams {
    pub(super) id: String,
}

// ── Results ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(super) struct BrowseResult {
    pub(super) entries: Vec<CatalogEntry>,
}

#[derive(Debug, Serialize)]
pub(super) struct SearchResult {
    pub(super) entries: Vec<CatalogEntry>,
}

#[derive(Debug, Serialize)]
pub(super) struct SourcesResult {
    pub(super) sources: Vec<RegistrySource>,
}

#[derive(Debug, Serialize)]
pub(super) struct InstallResult {
    pub(super) url: String,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) new_skills: Vec<String>,
}
