//! Business logic for the skill registry: fetch, index, search, and install.
//!
//! The catalog is sourced from the HermesHub aggregated JSON API which
//! includes skills from HermesHub (built-in + optional), ClawHub, skills.sh,
//! LobeHub, and browse.sh — all accessible from a single endpoint.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Mutex;

use super::download::{self, SkillsShRef};
use super::store;
use super::store::CachedCatalog;
use super::types::CatalogEntry;

const CATALOG_URL: &str = "https://hermes-agent.nousresearch.com/docs/api/skills.json";
const CATALOG_URL_ENV: &str = "OPENHUMAN_SKILL_REGISTRY_CATALOG_URL";
const REFRESH_ON_BOOT_ENV: &str = "OPENHUMAN_SKILL_REGISTRY_REFRESH_ON_BOOT";
const FETCH_TIMEOUT_SECS: u64 = 180;

/// Single-flight gate for catalog fetches. On mount the skills explorer issues
/// several catalog reads that each funnel into [`browse_catalog`] — `sources`
/// and `browse` from two separate effects, plus `search` as the user types —
/// and React StrictMode double-invokes those effects in dev, so a handful of
/// reads land within the same instant. Without this lock each would issue its
/// own ~80s download of the same ~90k-entry catalog. Concurrent cache-miss
/// callers serialize here, and all but the first re-read the just-written cache
/// instead of hitting the network.
static FETCH_LOCK: Mutex<()> = Mutex::const_new(());

/// True while a background (stale-while-revalidate) refresh is scheduled or
/// running, so a burst of stale-cache reads spawns at most one refresh task.
static REFRESHING: AtomicBool = AtomicBool::new(false);

/// Clears [`REFRESHING`] when the background refresh task ends (incl. panic).
struct RefreshGuard;
impl Drop for RefreshGuard {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::Release);
    }
}

/// Start a one-shot background refresh of the remote skills catalog.
///
/// This is intended for core startup: it warms the explorer/search cache without
/// making core readiness depend on registry availability. Set
/// `OPENHUMAN_SKILL_REGISTRY_REFRESH_ON_BOOT=0` to disable it in constrained
/// environments.
pub fn start_boot_catalog_refresh() {
    static STARTED: std::sync::Once = std::sync::Once::new();

    STARTED.call_once(|| {
        if !refresh_on_boot_enabled(std::env::var(REFRESH_ON_BOOT_ENV).ok().as_deref()) {
            tracing::info!(
                env = REFRESH_ON_BOOT_ENV,
                "[skill_registry] boot catalog refresh disabled"
            );
            return;
        }

        tracing::info!("[skill_registry] scheduling boot catalog refresh");
        tokio::spawn(async {
            let started = std::time::Instant::now();
            match browse_catalog(true).await {
                Ok(entries) => {
                    tracing::info!(
                        count = entries.len(),
                        elapsed_ms = started.elapsed().as_millis(),
                        "[skill_registry] boot catalog refresh complete"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        elapsed_ms = started.elapsed().as_millis(),
                        "[skill_registry] boot catalog refresh failed"
                    );
                }
            }
        });
    });
}

fn refresh_on_boot_enabled(raw: Option<&str>) -> bool {
    let Some(raw) = raw else { return true };
    let value = raw.trim();
    !(value == "0"
        || value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("no")
        || value.eq_ignore_ascii_case("off"))
}

/// Whether a past-TTL (stale) cache may be served without a network round-trip.
#[derive(Clone, Copy, PartialEq)]
enum StaleMode {
    /// Serve stale immediately + revalidate in the background — for the
    /// unfiltered browse, where a slightly-old catalog is fine and speed wins.
    Allow,
    /// Treat stale as a miss and fetch fresh under the single-flight lock — for
    /// search / filter reads, which must reflect the current catalog.
    Reject,
}

/// Fetch the full catalog for the **unfiltered browse** view, accepting a stale
/// cache (stale-while-revalidate):
/// - **Fresh cache** → returned immediately.
/// - **Stale cache** (past TTL) → returned immediately *and* a single background
///   refresh is kicked off, so the explorer renders from the last-known catalog
///   instead of blocking on the ~80s download.
/// - **No cache** → fetch under the single-flight lock; concurrent callers
///   coalesce onto that one request.
///
/// `force_refresh == true` (boot warm-up / explicit refresh) always re-fetches.
/// Search / filter reads use [`browse_catalog_fresh`], which never serves stale.
pub async fn browse_catalog(force_refresh: bool) -> Result<Vec<CatalogEntry>, String> {
    browse_catalog_with(force_refresh, StaleMode::Allow, fetch_catalog_uncached).await
}

/// Fetch the full catalog for **search / filter** reads. Never serves a stale
/// cache: a fresh cache is used as-is, but a stale-or-absent cache falls through
/// to a (single-flight) fresh fetch so results aren't computed over an outdated
/// catalog. Thanks to single-flight, a search issued while a background
/// revalidation is already running simply awaits that in-flight fetch rather
/// than starting a new one.
pub async fn browse_catalog_fresh() -> Result<Vec<CatalogEntry>, String> {
    browse_catalog_with(false, StaleMode::Reject, fetch_catalog_uncached).await
}

/// Core of [`browse_catalog`] / [`browse_catalog_fresh`], parameterised over the
/// fetcher so the cache / single-flight orchestration can be unit-tested without
/// real network I/O.
async fn browse_catalog_with<F, Fut>(
    force_refresh: bool,
    stale_mode: StaleMode,
    fetch: F,
) -> Result<Vec<CatalogEntry>, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<CatalogEntry>, String>>,
{
    if !force_refresh {
        match store::load_cached_catalog_state() {
            Some(CachedCatalog::Fresh(entries)) => {
                tracing::debug!(
                    count = entries.len(),
                    "[skill_registry] serving fresh cache"
                );
                return Ok(entries);
            }
            // Browse: serve stale now, revalidate in background.
            Some(CachedCatalog::Stale(entries)) if stale_mode == StaleMode::Allow => {
                tracing::info!(
                    count = entries.len(),
                    "[skill_registry] serving stale cache; revalidating in background"
                );
                spawn_background_refresh();
                return Ok(entries);
            }
            // Search / filter: stale is not good enough — fall through to fetch.
            Some(CachedCatalog::Stale(_)) => {
                tracing::debug!("[skill_registry] stale cache rejected for fresh read; fetching");
            }
            None => {}
        }
    }

    // Single-flight: only one fetch runs at a time. Callers that queued behind
    // the lock re-check the cache below and reuse the just-fetched result.
    let _guard = FETCH_LOCK.lock().await;
    if !force_refresh {
        if let Some(CachedCatalog::Fresh(entries)) = store::load_cached_catalog_state() {
            tracing::debug!(
                count = entries.len(),
                "[skill_registry] cache populated by concurrent fetch; reusing"
            );
            return Ok(entries);
        }
    }

    fetch().await
}

/// Spawn at most one background catalog refresh (stale-while-revalidate). Extra
/// calls while a refresh is in flight no-op via [`REFRESHING`]. The refresh runs
/// under [`FETCH_LOCK`] so it never races a foreground fetch.
fn spawn_background_refresh() {
    if REFRESHING.swap(true, Ordering::AcqRel) {
        return;
    }
    tokio::spawn(async {
        let _reset = RefreshGuard;
        let _guard = FETCH_LOCK.lock().await;
        match fetch_catalog_uncached().await {
            Ok(entries) => tracing::info!(
                count = entries.len(),
                "[skill_registry] background catalog refresh complete"
            ),
            Err(error) => {
                tracing::warn!(error = %error, "[skill_registry] background catalog refresh failed")
            }
        }
    });
}

/// Download, parse, index, and cache the catalog — the network path, unguarded.
/// Callers must go through [`browse_catalog_with`] / [`spawn_background_refresh`]
/// so this runs under the single-flight lock.
async fn fetch_catalog_uncached() -> Result<Vec<CatalogEntry>, String> {
    let catalog_url = catalog_url();
    tracing::info!(
        catalog_url = %redact_url_for_log(&catalog_url),
        "[skill_registry] fetching catalog"
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;

    let response = client
        .get(&catalog_url)
        .header("User-Agent", "openhuman-core")
        .send()
        .await
        .map_err(|e| format!("catalog fetch failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "catalog returned status {}",
            response.status().as_u16()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read response: {e}"))?;

    let raw_items: Vec<serde_json::Value> = parse_catalog_json(&body)?;

    tracing::info!(
        total_raw = raw_items.len(),
        "[skill_registry] parsing catalog"
    );

    let entries: Vec<CatalogEntry> = raw_items.iter().filter_map(parse_hermes_entry).collect();

    tracing::info!(count = entries.len(), "[skill_registry] catalog indexed");

    store::save_catalog_cache(&entries);
    Ok(entries)
}

fn catalog_url() -> String {
    std::env::var(CATALOG_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| CATALOG_URL.to_string())
}

fn redact_url_for_log(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("");
            let path = parsed.path();
            format!("{scheme}://{host}{path}")
        }
        Err(_) => "<unparseable>".to_string(),
    }
}

pub(crate) fn parse_catalog_json(body: &str) -> Result<Vec<serde_json::Value>, String> {
    serde_json::from_str(body).map_err(|e| format!("invalid catalog json: {e}"))
}

/// Search the catalog by query string.
pub async fn search_catalog(
    query: &str,
    source_filter: Option<&str>,
    category_filter: Option<&str>,
) -> Result<Vec<CatalogEntry>, String> {
    tracing::debug!(
        query = %query,
        source_filter = ?source_filter,
        category_filter = ?category_filter,
        "[skill_registry] search_catalog"
    );
    // Search/filter must reflect the current catalog — never serve stale.
    let catalog = browse_catalog_fresh().await?;
    let q = query.to_lowercase();

    let mut filtered: Vec<CatalogEntry> = catalog
        .into_iter()
        .filter(|entry| {
            if let Some(src) = source_filter {
                if !entry.source.eq_ignore_ascii_case(src) {
                    return false;
                }
            }
            if let Some(cat) = category_filter {
                if !entry.category.eq_ignore_ascii_case(cat) {
                    return false;
                }
            }
            if q.is_empty() {
                return true;
            }
            entry.name.to_lowercase().contains(&q)
                || entry.description.to_lowercase().contains(&q)
                || entry.tags.iter().any(|t| t.to_lowercase().contains(&q))
                || entry.category.to_lowercase().contains(&q)
                || entry
                    .author
                    .as_deref()
                    .map(|a| a.to_lowercase().contains(&q))
                    .unwrap_or(false)
        })
        .collect();
    // Entries install cannot fetch go last, so find-and-install reaches a
    // working hit first. The sort is stable: match order is otherwise kept.
    filtered.sort_by_key(|entry| !entry.has_direct_download());

    tracing::debug!(
        result_count = filtered.len(),
        "[skill_registry] search complete"
    );
    Ok(filtered)
}

/// Return the distinct set of upstream sources present in the catalog.
pub async fn list_sources() -> Result<Vec<String>, String> {
    let catalog = browse_catalog(false).await?;
    let mut sources: Vec<String> = catalog
        .iter()
        .map(|e| e.source.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    sources.sort();
    Ok(sources)
}

/// Return the distinct set of categories present in the catalog.
pub async fn list_categories() -> Result<Vec<String>, String> {
    let catalog = browse_catalog(false).await?;
    let mut categories: Vec<String> = catalog
        .iter()
        .map(|e| e.category.clone())
        .filter(|c| !c.is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    categories.sort();
    Ok(categories)
}

/// Install a skill from the catalog by its entry id.
pub async fn install_from_catalog(
    workspace_dir: &std::path::Path,
    entry: &CatalogEntry,
) -> Result<crate::skills::ops_install::InstallWorkflowFromUrlOutcome, String> {
    tracing::info!(
        entry_id = %entry.id,
        source = %entry.source,
        download_url = %entry.download_url,
        "[skill_registry] installing from catalog"
    );

    if !entry.has_direct_download() {
        let where_to_find = entry
            .source_url
            .as_deref()
            .map(|u| format!(" View it at {u}."))
            .unwrap_or_default();
        return Err(format!(
            "'{}' is hosted on {} and has no direct SKILL.md download, so it can't be installed automatically yet.{}",
            entry.name, entry.source, where_to_find
        ));
    }

    // A skills.sh entry's `download_url` is the most common location, not a
    // verified one: find where this repo keeps the skill before fetching.
    let url = match entry.source_url.as_deref().and_then(SkillsShRef::parse) {
        Some(skill) if skill.candidate_urls().first() == Some(&entry.download_url) => {
            skill.resolve().await?
        }
        _ => entry.download_url.clone(),
    };

    let params = crate::skills::ops_install::InstallWorkflowFromUrlParams {
        url,
        timeout_secs: Some(60),
    };

    crate::skills::ops_install::install_workflow_from_url(workspace_dir, params)
        .await
        .map_err(|error| {
            // ClawHub answers a slug that several authors publish under with
            // 409, and the catalog does not record which author's skill this is.
            if entry.source.eq_ignore_ascii_case("clawhub") && error.ends_with("returned status 409")
            {
                format!(
                    "'{}' is published on ClawHub by more than one author and the catalog does not say which one, so it can't be installed automatically.",
                    entry.name
                )
            } else {
                error
            }
        })
}

/// How many alternative ids an install error lists.
const MAX_SUGGESTED_IDS: usize = 5;

/// Resolve an install request to exactly one catalog entry.
///
/// `entry_id` is matched against [`CatalogEntry::id`]. Ids used to be display
/// names, which many entries share, so a name is still accepted when exactly
/// one entry carries it; otherwise the error names real ids to use instead.
pub fn find_catalog_entry<'a>(
    catalog: &'a [CatalogEntry],
    entry_id: &str,
) -> Result<&'a CatalogEntry, String> {
    let entry_id = entry_id.trim();
    if let Some(entry) = catalog.iter().find(|e| e.id == entry_id) {
        return Ok(entry);
    }
    let named: Vec<&CatalogEntry> = catalog.iter().filter(|e| e.name == entry_id).collect();
    match named.as_slice() {
        [entry] => Ok(*entry),
        [] => {
            let closest = closest_entry_ids(catalog, entry_id);
            tracing::debug!(
                entry_id = %entry_id,
                suggestions = closest.len(),
                "[skill_registry] install id not in catalog"
            );
            let hint = if closest.is_empty() {
                "Use an id returned by skill_registry_search.".to_string()
            } else {
                format!("Closest ids: {}.", closest.join(", "))
            };
            Err(format!("no catalog entry has id '{entry_id}'. {hint}"))
        }
        many => Err(format!(
            "{} catalog entries are named '{entry_id}'; install one by its id, e.g. {}.",
            many.len(),
            many.iter()
                .take(MAX_SUGGESTED_IDS)
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Ids sharing the most words with `wanted`; installable and shorter ids win ties.
fn closest_entry_ids(catalog: &[CatalogEntry], wanted: &str) -> Vec<String> {
    let words: Vec<String> = wanted
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut scored: Vec<(usize, bool, &str)> = catalog
        .iter()
        .filter_map(|entry| {
            let haystack = format!("{} {}", entry.id, entry.name).to_lowercase();
            let score = words
                .iter()
                .filter(|w| haystack.contains(w.as_str()))
                .count();
            (score > 0).then_some((score, entry.has_direct_download(), entry.id.as_str()))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(b.1.cmp(&a.1))
            .then(a.2.len().cmp(&b.2.len()))
    });
    scored
        .into_iter()
        .take(MAX_SUGGESTED_IDS)
        .map(|(_, _, id)| id.to_string())
        .collect()
}

pub(crate) fn parse_hermes_entry(item: &serde_json::Value) -> Option<CatalogEntry> {
    let name = item.get("name").and_then(|v| v.as_str())?.to_string();

    let description = item
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let source = item
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("hermes")
        .to_string();

    let category = item
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let author = item
        .get("author")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let version = item
        .get("version")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let license = item
        .get("license")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let tags = item
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let platforms = item
        .get("platforms")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let commands = item
        .get("commands")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let env_vars = item
        .get("envVars")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    let docs_path = item
        .get("docsPath")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let source_url = item
        .get("sourceUrl")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let identifier = item
        .get("identifier")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let download_url = download::derive_download_url(
        &source,
        identifier,
        &name,
        docs_path.as_deref(),
        source_url.as_deref(),
    );

    Some(CatalogEntry {
        id: catalog_entry_id(&source, identifier, &name),
        name,
        description,
        source,
        category,
        author,
        version,
        tags,
        platforms,
        download_url,
        source_url,
        docs_path,
        commands,
        env_vars,
        license,
    })
}

/// Stable, unique entry id.
///
/// Hermes publishes a unique `identifier` per entry. Most are already
/// source-qualified paths (`skills-sh/o/r/s`, `lobehub/x`, `owner/repo/path`),
/// but ClawHub's is a bare slug that can equal another source's skill name, so
/// a bare identifier is prefixed with its source. Bundled and optional Hermes
/// skills carry no identifier; their names are unique among themselves and
/// contain no `/`, so they cannot collide with a qualified id.
fn catalog_entry_id(source: &str, identifier: Option<&str>, name: &str) -> String {
    match identifier {
        Some(identifier) if identifier.contains('/') => identifier.to_string(),
        Some(slug) => format!("{}/{slug}", source.to_ascii_lowercase()),
        None => name.to_string(),
    }
}

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
