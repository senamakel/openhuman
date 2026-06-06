//! Domain types for the skill registry: sources, catalog entries, and search results.

use serde::{Deserialize, Serialize};

/// A remote source of skills that the registry can fetch from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrySource {
    /// Unique identifier for this source (e.g. "openhuman-community", "clawhub", "hermes-hub").
    pub id: String,
    /// Human-readable label.
    pub name: String,
    /// Base URL for the registry API or GitHub repo index.
    pub url: String,
    /// What kind of registry this is.
    pub kind: RegistryKind,
    /// Whether this source is enabled.
    pub enabled: bool,
}

/// The type of registry source — determines the fetch/parse strategy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegistryKind {
    /// A GitHub repo containing an index of SKILL.md files (JSON manifest).
    GithubIndex,
    /// A raw HTTPS endpoint returning a JSON catalog.
    HttpCatalog,
}

/// One entry in the remote catalog — metadata about an installable skill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Unique slug within the source.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Skill format: "openhuman", "hermes", "openclaw".
    pub format: String,
    /// Author name, if known.
    pub author: Option<String>,
    /// Version string, if declared.
    pub version: Option<String>,
    /// Tags for search/filter.
    pub tags: Vec<String>,
    /// Direct download URL for the SKILL.md file.
    pub download_url: String,
    /// Which registry source this came from.
    pub source_id: String,
    /// Star count or popularity metric, if available.
    pub stars: Option<u32>,
    /// Last updated timestamp (ISO 8601), if available.
    pub updated_at: Option<String>,
}
