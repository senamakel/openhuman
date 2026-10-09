//! The memory explorer: `memory_explore` and `memory_items_get`.
//!
//! An explorer walks what memory holds by TinyMemory's standard facets (kind,
//! source, workspace, folder, file, thread, agent, tool call, tag, …; see
//! `tinymemory_api::explore`). The client keeps only a breadcrumb [`PathStep`]
//! list; the core turns it into a filter with `Facet::narrow`, so drilling
//! down means exactly the same thing in every client and for every engine:
//!
//! 1. `memory_explore {facet, path}` counts the items under `path` per value
//!    of `facet`;
//! 2. picking a bucket appends `{facet, value}` to `path`;
//! 3. `memory_items_list {path}` pages through the items there, and
//!    `memory_items_get {ids}` reads one whole.

use serde::{Deserialize, Serialize};
use tinymemory_api::{ExplorePage, ExploreRequest, Facet, GetRequest, Hit, ItemId, MetaFilter};

use crate::config::Config;

use super::engine;
use super::error::{MemoryError, MemoryResult};

/// Default bucket count of `memory_explore`.
pub const DEFAULT_BUCKETS: usize = 50;

/// Most steps an explorer path may take (one per facet is the useful most).
pub const MAX_PATH_STEPS: usize = 16;

/// One step down the explorer: the items whose `facet` is `value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathStep {
    /// The facet.
    pub facet: Facet,
    /// Its chosen value, as a bucket reported it.
    pub value: String,
}

/// `memory_explore` params.
#[derive(Debug, Clone, Deserialize)]
pub struct ExploreParams {
    /// The facet to group by.
    pub facet: Facet,
    /// Where in the explorer: each step narrows the items grouped.
    #[serde(default)]
    pub path: Vec<PathStep>,
    /// An extra metadata filter, applied before the path.
    #[serde(default)]
    pub filter: Option<MetaFilter>,
    /// Most buckets (default [`DEFAULT_BUCKETS`]).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Most items a listing-based engine reads.
    #[serde(default)]
    pub scan_limit: Option<usize>,
}

/// `memory_items_get` params.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemsGetParams {
    /// Item ids.
    pub ids: Vec<String>,
    /// Only items in this reach are returned; unset reads every namespace.
    #[serde(default)]
    pub reach: Option<tinymemory_api::Reach>,
}

/// `memory_items_get` result.
#[derive(Debug, Clone, Serialize)]
pub struct ItemsGetView {
    /// The items found, in the order asked; unknown ids are left out.
    pub items: Vec<Hit>,
}

/// `filter` narrowed by every step of `path`.
///
/// # Errors
///
/// [`MemoryError::invalid`] for a path that is too long or a step whose
/// value the facet cannot take (blank, or an unknown kind).
pub fn narrowed(filter: Option<MetaFilter>, path: &[PathStep]) -> MemoryResult<MetaFilter> {
    if path.len() > MAX_PATH_STEPS {
        return Err(MemoryError::invalid(format!(
            "an explorer path takes at most {MAX_PATH_STEPS} steps"
        )));
    }
    let mut filter = filter.unwrap_or_default();
    for step in path {
        step.facet.narrow(&mut filter, &step.value)?;
    }
    Ok(filter)
}

/// `memory_explore`.
pub async fn explore(config: &Config, params: ExploreParams) -> MemoryResult<ExplorePage> {
    let bound = engine::resolve(config).engine()?;
    let mut request = ExploreRequest::new(params.facet, params.limit.unwrap_or(DEFAULT_BUCKETS));
    request.filter = super::ops::confine_filter(config, narrowed(params.filter, &params.path)?)?;
    if let Some(scan_limit) = params.scan_limit {
        request.scan_limit = scan_limit;
    }
    request.validate()?;
    let page = bound.engine.explore(request).await?;
    tracing::debug!(
        engine = %bound.id,
        facet = params.facet.as_str(),
        depth = params.path.len(),
        buckets = page.buckets.len(),
        total = page.total,
        truncated = page.truncated,
        "[memory:explore] explored"
    );
    Ok(page)
}

/// `memory_items_get`.
pub async fn items_get(config: &Config, params: ItemsGetParams) -> MemoryResult<ItemsGetView> {
    let bound = engine::resolve(config).engine()?;
    let request = GetRequest {
        ids: params.ids.into_iter().map(ItemId::new).collect(),
        reach: params.reach,
    };
    request.validate()?;
    let asked = request.ids.len();
    let items = bound.engine.get(request).await?;
    tracing::debug!(
        engine = %bound.id,
        asked,
        found = items.len(),
        "[memory:explore] items read"
    );
    Ok(ItemsGetView { items })
}

#[cfg(test)]
#[path = "explore_tests.rs"]
mod tests;
