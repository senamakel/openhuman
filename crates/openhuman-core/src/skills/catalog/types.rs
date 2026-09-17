//! Domain types for the skill registry.

use serde::{Deserialize, Serialize};

/// One entry in the indexed skill catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Unique id: the upstream identifier, source-qualified (e.g.
    /// "clawhub/apple-design", "skills-sh/owner/repo/skill"), or the name for
    /// bundled Hermes skills (e.g. "apple-notes").
    pub id: String,
    /// Display name.
    pub name: String,
    /// Short description.
    pub description: String,
    /// Upstream source within the aggregated catalog (e.g. "built-in",
    /// "optional", "ClawHub", "skills.sh", "LobeHub", "browse.sh").
    pub source: String,
    /// Category label from the upstream catalog.
    pub category: String,
    /// Author name, if known.
    pub author: Option<String>,
    /// Version string, if declared.
    pub version: Option<String>,
    /// Tags for search/filter.
    pub tags: Vec<String>,
    /// Compatible platform hints.
    pub platforms: Vec<String>,
    /// Download URL for the SKILL.md file. Empty when the source publishes no
    /// `SKILL.md` (LobeHub agents); install surfaces an actionable error
    /// pointing at [`source_url`] instead of a misleading 404. For skills.sh
    /// it is the most common location, resolved at install time.
    pub download_url: String,
    /// Human-facing source page for the skill (GitHub blob/tree, LobeHub,
    /// ClawHub, skills.sh, …). Carried from the catalog's `sourceUrl`; used to
    /// derive the raw download URL for GitHub-hosted community skills and to
    /// give the user a link when no direct download exists. See issue #3741.
    pub source_url: Option<String>,
    /// Docs path from the Hermes catalog.
    pub docs_path: Option<String>,
    /// Required CLI commands.
    pub commands: Vec<String>,
    /// Required environment variables.
    pub env_vars: Vec<String>,
    /// Software license.
    pub license: Option<String>,
}

impl CatalogEntry {
    /// Whether this entry has a `SKILL.md` that can be fetched directly. An
    /// entry without one can never be installed automatically.
    pub fn has_direct_download(&self) -> bool {
        !self.download_url.trim().is_empty()
    }
}
