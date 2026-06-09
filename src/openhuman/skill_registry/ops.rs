//! Business logic for the skill registry: fetch, index, search, and install.
//!
//! The catalog is sourced from the HermesHub aggregated JSON API which
//! includes skills from HermesHub (built-in + optional), ClawHub, skills.sh,
//! LobeHub, and browse.sh — all accessible from a single endpoint.

use super::store;
use super::types::CatalogEntry;

const CATALOG_URL: &str = "https://hermes-agent.nousresearch.com/docs/api/skills.json";
const CATALOG_URL_ENV: &str = "OPENHUMAN_SKILL_REGISTRY_CATALOG_URL";
const DOWNLOAD_BASE_URL_ENV: &str = "OPENHUMAN_SKILL_REGISTRY_DOWNLOAD_BASE_URL";
const REFRESH_ON_BOOT_ENV: &str = "OPENHUMAN_SKILL_REGISTRY_REFRESH_ON_BOOT";
const FETCH_TIMEOUT_SECS: u64 = 180;

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

/// Fetch the full catalog, using cache when fresh.
pub async fn browse_catalog(force_refresh: bool) -> Result<Vec<CatalogEntry>, String> {
    if !force_refresh {
        if let Some(cached) = store::load_cached_catalog() {
            tracing::debug!(count = cached.len(), "[skill_registry] serving from cache");
            return Ok(cached);
        }
    }

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
        .map(|s| s.to_string());

    let download_url = derive_download_url(&source, &category, &name, docs_path.as_deref());

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

fn derive_download_url(
    source: &str,
    category: &str,
    name: &str,
    docs_path: Option<&str>,
) -> String {
    if let Ok(base) = std::env::var(DOWNLOAD_BASE_URL_ENV) {
        let base = base.trim().trim_end_matches('/');
        if !base.is_empty() {
            return format!("{base}/{name}/SKILL.md");
        }
    }
    if let Some(url) = docs_path.and_then(download_url_from_docs_path) {
        return url;
    }
    let root = match source {
        "optional" => "optional-skills",
        _ => "skills",
    };
    format!(
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/{root}/{category}/{name}/SKILL.md"
    )
}

fn download_url_from_docs_path(docs_path: &str) -> Option<String> {
    let parts: Vec<&str> = docs_path.split('/').collect();
    if parts.len() != 3 {
        return None;
    }
    let root = match parts[0] {
        "bundled" => "skills",
        "optional" => "optional-skills",
        _ => return None,
    };
    let category = parts[1];
    let prefixed_slug = parts[2];
    let skill = prefixed_slug
        .strip_prefix(&format!("{category}-"))
        .unwrap_or(prefixed_slug);
    Some(format!(
        "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/{root}/{category}/{skill}/SKILL.md"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_hermes_entry_derives_bundled_download_url_from_docs_path() {
        let item = json!({
            "name": "apple-notes",
            "description": "Manage Apple Notes",
            "category": "apple",
            "source": "built-in",
            "docsPath": "bundled/apple/apple-apple-notes",
            "tags": ["Apple"],
            "platforms": ["macos"],
            "commands": ["memo"],
            "envVars": []
        });
        let entry = parse_hermes_entry(&item).expect("entry");
        assert_eq!(
            entry.download_url,
            "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/skills/apple/apple-notes/SKILL.md"
        );
    }

    #[test]
    fn parse_hermes_entry_derives_optional_download_url_from_docs_path() {
        let item = json!({
            "name": "docker-management",
            "description": "Manage Docker",
            "category": "devops",
            "source": "optional",
            "docsPath": "optional/devops/devops-docker-management"
        });
        let entry = parse_hermes_entry(&item).expect("entry");
        assert_eq!(
            entry.download_url,
            "https://raw.githubusercontent.com/NousResearch/hermes-agent/main/optional-skills/devops/docker-management/SKILL.md"
        );
    }

    #[test]
    fn parse_catalog_json_rejects_invalid_payloads() {
        let error = parse_catalog_json("{").expect_err("invalid json");
        assert!(error.contains("invalid catalog json"));
    }

    #[test]
    fn refresh_on_boot_enabled_defaults_on_and_accepts_common_false_values() {
        assert!(refresh_on_boot_enabled(None));
        assert!(refresh_on_boot_enabled(Some("1")));
        assert!(refresh_on_boot_enabled(Some("true")));

        assert!(!refresh_on_boot_enabled(Some("0")));
        assert!(!refresh_on_boot_enabled(Some("false")));
        assert!(!refresh_on_boot_enabled(Some(" no ")));
        assert!(!refresh_on_boot_enabled(Some("OFF")));
    }
}
