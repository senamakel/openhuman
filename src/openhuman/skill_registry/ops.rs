//! Business logic for the skill registry: fetch, index, search, and install.
//!
//! The catalog is sourced from the HermesHub aggregated JSON API which
//! includes skills from HermesHub (built-in + optional), ClawHub, skills.sh,
//! LobeHub, and browse.sh — all accessible from a single endpoint.

use super::store;
use super::types::CatalogEntry;

const CATALOG_URL: &str =
    "https://hermes-agent.nousresearch.com/docs/api/skills.json";
const FETCH_TIMEOUT_SECS: u64 = 60;

/// Fetch the full catalog, using cache when fresh.
pub async fn browse_catalog(force_refresh: bool) -> Result<Vec<CatalogEntry>, String> {
    if !force_refresh {
        if let Some(cached) = store::load_cached_catalog() {
            tracing::debug!(
                count = cached.len(),
                "[skill_registry] serving from cache"
            );
            return Ok(cached);
        }
    }

    tracing::info!("[skill_registry] fetching catalog from Hermes API");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;

    let response = client
        .get(CATALOG_URL)
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

    let raw_items: Vec<serde_json::Value> =
        serde_json::from_str(&body).map_err(|e| format!("invalid catalog json: {e}"))?;

    tracing::info!(
        total_raw = raw_items.len(),
        "[skill_registry] parsing catalog"
    );

    let entries: Vec<CatalogEntry> = raw_items
        .iter()
        .filter_map(|item| parse_hermes_entry(item))
        .collect();

    tracing::info!(
        count = entries.len(),
        "[skill_registry] catalog indexed"
    );

    store::save_catalog_cache(&entries);
    Ok(entries)
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
    let catalog = browse_catalog(false).await?;
    let q = query.to_lowercase();

    let filtered: Vec<CatalogEntry> = catalog
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
) -> Result<crate::openhuman::workflows::ops_install::InstallWorkflowFromUrlOutcome, String> {
    tracing::info!(
        entry_id = %entry.id,
        source = %entry.source,
        download_url = %entry.download_url,
        "[skill_registry] installing from catalog"
    );

    let params = crate::openhuman::workflows::ops_install::InstallWorkflowFromUrlParams {
        url: entry.download_url.clone(),
        timeout_secs: Some(60),
    };

    crate::openhuman::workflows::ops_install::install_workflow_from_url(workspace_dir, params).await
}

fn parse_hermes_entry(item: &serde_json::Value) -> Option<CatalogEntry> {
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
        .map(|s| s.to_string());

    let download_url = format!(
        "https://raw.githubusercontent.com/amanning3390/hermeshub/main/skills/{name}/SKILL.md"
    );

    Some(CatalogEntry {
        id: name.clone(),
        name,
        description,
        source,
        category,
        author,
        version,
        tags,
        platforms,
        download_url,
        docs_path,
        commands,
        env_vars,
        license,
    })
}
