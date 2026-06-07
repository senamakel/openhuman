//! Business logic for the skill registry: fetch catalogs, search, and install.

use serde::Deserialize;

use super::store;
use super::types::{CatalogEntry, RegistryKind, RegistrySource};

const MAX_CATALOG_BYTES: usize = 5 * 1024 * 1024;
const FETCH_TIMEOUT_SECS: u64 = 30;

/// Default registry sources shipped with the app.
pub fn default_sources() -> Vec<RegistrySource> {
    vec![
        RegistrySource {
            id: "openhuman-community".into(),
            name: "OpenHuman Community Skills".into(),
            url: "https://raw.githubusercontent.com/tinyhumansai/skill-registry/main/index.json"
                .into(),
            kind: RegistryKind::GithubIndex,
            enabled: true,
        },
        RegistrySource {
            id: "hermeshub".into(),
            name: "HermesHub Community Skills".into(),
            url: "https://api.github.com/repos/amanning3390/hermeshub/contents/skills".into(),
            kind: RegistryKind::GithubSkillsDir,
            enabled: true,
        },
        RegistrySource {
            id: "clawhub".into(),
            name: "ClawHub (OpenClaw Registry)".into(),
            url: "https://clawhub.ai/api/v1/skills".into(),
            kind: RegistryKind::ClawHubApi,
            enabled: true,
        },
    ]
}

/// Resolve the active list of registry sources: defaults + any user-added custom ones.
pub fn list_sources() -> Vec<RegistrySource> {
    let mut sources = default_sources();
    let custom = store::load_custom_sources();
    tracing::debug!(
        default_count = sources.len(),
        custom_count = custom.len(),
        "[skill_registry] list_sources"
    );
    for c in custom {
        if !sources.iter().any(|s| s.id == c.id) {
            sources.push(c);
        }
    }
    tracing::debug!(total = sources.len(), "[skill_registry] list_sources done");
    sources
}

/// Add a custom registry source.
pub fn add_source(source: RegistrySource) -> Result<(), String> {
    tracing::debug!(
        source_id = %source.id,
        source_url = %source.url,
        "[skill_registry] add_source"
    );
    if source.id.is_empty() {
        return Err("source id must not be empty".into());
    }
    if source.url.is_empty() {
        return Err("source url must not be empty".into());
    }
    let mut custom = store::load_custom_sources();
    if custom.iter().any(|s| s.id == source.id) {
        return Err(format!("source '{}' already exists", source.id));
    }
    custom.push(source);
    store::save_custom_sources(&custom);
    store::clear_cache();
    tracing::info!("[skill_registry] source added, cache cleared");
    Ok(())
}

/// Remove a custom registry source by id.
pub fn remove_source(id: &str) -> Result<(), String> {
    tracing::debug!(source_id = %id, "[skill_registry] remove_source");
    let mut custom = store::load_custom_sources();
    let before = custom.len();
    custom.retain(|s| s.id != id);
    if custom.len() == before {
        return Err(format!("source '{id}' not found in custom sources"));
    }
    store::save_custom_sources(&custom);
    store::clear_cache();
    tracing::info!(source_id = %id, "[skill_registry] source removed, cache cleared");
    Ok(())
}

/// Fetch the full catalog from all enabled sources, using cache when fresh.
pub async fn browse_catalog(force_refresh: bool) -> Result<Vec<CatalogEntry>, String> {
    if !force_refresh {
        if let Some(cached) = store::load_cached_catalog() {
            return Ok(cached);
        }
    }

    let sources = list_sources();
    let enabled: Vec<&RegistrySource> = sources.iter().filter(|s| s.enabled).collect();

    if enabled.is_empty() {
        return Ok(Vec::new());
    }

    tracing::info!(
        count = enabled.len(),
        "[skill_registry] fetching catalogs from {} source(s)",
        enabled.len()
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;

    let mut all_entries: Vec<CatalogEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for source in &enabled {
        match fetch_source_catalog(&client, source).await {
            Ok(entries) => {
                tracing::debug!(
                    source = %source.id,
                    count = entries.len(),
                    "[skill_registry] fetched catalog"
                );
                all_entries.extend(entries);
            }
            Err(e) => {
                tracing::warn!(
                    source = %source.id,
                    error = %e,
                    "[skill_registry] failed to fetch catalog"
                );
                errors.push(format!("{}: {e}", source.id));
            }
        }
    }

    all_entries.sort_by(|a, b| {
        b.stars
            .unwrap_or(0)
            .cmp(&a.stars.unwrap_or(0))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    store::save_catalog_cache(&all_entries);

    if all_entries.is_empty() && !errors.is_empty() {
        return Err(format!(
            "all registry sources failed: {}",
            errors.join("; ")
        ));
    }

    Ok(all_entries)
}

/// Search the catalog by query string, matching against name, description, tags, and format.
pub async fn search_catalog(
    query: &str,
    format_filter: Option<&str>,
    source_filter: Option<&str>,
) -> Result<Vec<CatalogEntry>, String> {
    tracing::debug!(
        query = %query,
        format_filter = ?format_filter,
        source_filter = ?source_filter,
        "[skill_registry] search_catalog"
    );
    let catalog = browse_catalog(false).await?;
    let q = query.to_lowercase();

    let filtered: Vec<CatalogEntry> = catalog
        .into_iter()
        .filter(|entry| {
            if let Some(fmt) = format_filter {
                if entry.format != fmt {
                    return false;
                }
            }
            if let Some(src) = source_filter {
                if entry.source_id != src {
                    return false;
                }
            }
            if q.is_empty() {
                return true;
            }
            entry.name.to_lowercase().contains(&q)
                || entry.description.to_lowercase().contains(&q)
                || entry.tags.iter().any(|t| t.to_lowercase().contains(&q))
                || entry.format.to_lowercase().contains(&q)
                || entry
                    .author
                    .as_deref()
                    .map(|a| a.to_lowercase().contains(&q))
                    .unwrap_or(false)
        })
        .collect();

    tracing::debug!(
        result_count = filtered.len(),
        "[skill_registry] search_catalog complete"
    );
    Ok(filtered)
}

/// Install a skill from the catalog by its entry. Delegates to the existing
/// `install_workflow_from_url` in the workflows module.
pub async fn install_from_catalog(
    workspace_dir: &std::path::Path,
    entry: &CatalogEntry,
) -> Result<crate::openhuman::workflows::ops_install::InstallWorkflowFromUrlOutcome, String> {
    tracing::info!(
        entry_id = %entry.id,
        source = %entry.source_id,
        format = %entry.format,
        download_url = %entry.download_url,
        "[skill_registry] installing from catalog"
    );

    // ClawHub skills use a `clawhub://` scheme — direct HTTP install is not yet supported.
    // The OpenClaw CLI (`openclaw skills install <slug>`) is the official install path.
    if entry.download_url.starts_with("clawhub://") {
        let slug = entry
            .download_url
            .strip_prefix("clawhub://")
            .unwrap_or(&entry.id);
        tracing::warn!(
            slug = %slug,
            "[skill_registry] clawhub direct install not supported, directing user to CLI"
        );
        return Err(format!(
            "ClawHub skills cannot be installed directly yet. \
             Install via the OpenClaw CLI: openclaw skills install {slug}"
        ));
    }

    let params = crate::openhuman::workflows::ops_install::InstallWorkflowFromUrlParams {
        url: entry.download_url.clone(),
        timeout_secs: Some(60),
    };

    crate::openhuman::workflows::ops_install::install_workflow_from_url(workspace_dir, params).await
}

async fn fetch_source_catalog(
    client: &reqwest::Client,
    source: &RegistrySource,
) -> Result<Vec<CatalogEntry>, String> {
    tracing::debug!(
        source_id = %source.id,
        kind = ?source.kind,
        url = %source.url,
        "[skill_registry] fetch_source_catalog entry"
    );

    match source.kind {
        RegistryKind::GithubIndex | RegistryKind::HttpCatalog => {
            let response = client
                .get(&source.url)
                .send()
                .await
                .map_err(|e| format!("fetch failed: {e}"))?;

            if !response.status().is_success() {
                return Err(format!(
                    "fetch returned status {}",
                    response.status().as_u16()
                ));
            }

            if let Some(len) = response.content_length() {
                if len > MAX_CATALOG_BYTES as u64 {
                    return Err(format!("catalog too large: {len} bytes"));
                }
            }

            let body = response
                .text()
                .await
                .map_err(|e| format!("failed to read body: {e}"))?;

            if body.len() > MAX_CATALOG_BYTES {
                return Err(format!("catalog too large: {} bytes", body.len()));
            }

            parse_index_json(&body, &source.id)
        }
        RegistryKind::GithubSkillsDir => fetch_github_skills_dir(client, source).await,
        RegistryKind::ClawHubApi => fetch_clawhub_api(client, source).await,
    }
}

/// Fetch skills from a GitHub API `contents/` endpoint that lists a skills directory.
///
/// Each sub-directory of the listed path is treated as one skill. The entry's
/// `download_url` points to the raw `SKILL.md` in that sub-directory on the
/// `main` branch. Description fields are left empty — they can be enriched at
/// install time by fetching the SKILL.md content.
async fn fetch_github_skills_dir(
    client: &reqwest::Client,
    source: &RegistrySource,
) -> Result<Vec<CatalogEntry>, String> {
    tracing::debug!(
        source_id = %source.id,
        url = %source.url,
        "[skill_registry] fetch_github_skills_dir"
    );

    let response = client
        .get(&source.url)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "openhuman-core")
        .send()
        .await
        .map_err(|e| format!("github api fetch failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "github api returned status {}",
            response.status().as_u16()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|e| format!("failed to read github response: {e}"))?;

    let items: Vec<serde_json::Value> =
        serde_json::from_str(&body).map_err(|e| format!("invalid github api json: {e}"))?;

    // Extract owner/repo from a URL of the form:
    // https://api.github.com/repos/{owner}/{repo}/contents/{path}
    let parsed =
        url::Url::parse(&source.url).map_err(|e| format!("bad source url: {e}"))?;
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.collect())
        .unwrap_or_default();
    // segments: ["repos", owner, repo, "contents", ...]
    let (owner, repo) = if segments.len() >= 3 {
        (segments[1], segments[2])
    } else {
        return Err("cannot parse owner/repo from github api url".into());
    };

    tracing::debug!(
        owner = %owner,
        repo = %repo,
        item_count = items.len(),
        "[skill_registry] fetch_github_skills_dir parsed repo"
    );

    let mut entries = Vec::new();
    for item in &items {
        if item.get("type").and_then(|v| v.as_str()) != Some("dir") {
            continue;
        }
        let name = match item.get("name").and_then(|v| v.as_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let download_url = format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/main/skills/{name}/SKILL.md"
        );
        tracing::trace!(
            skill = %name,
            download_url = %download_url,
            "[skill_registry] fetch_github_skills_dir skill entry"
        );
        entries.push(CatalogEntry {
            id: name.clone(),
            name: name.clone(),
            description: String::new(),
            format: "agentskills".into(),
            author: None,
            version: None,
            tags: vec![],
            download_url,
            source_id: source.id.clone(),
            stars: None,
            updated_at: None,
        });
    }

    tracing::info!(
        source_id = %source.id,
        count = entries.len(),
        "[skill_registry] fetch_github_skills_dir complete"
    );
    Ok(entries)
}

/// Fetch skills from the ClawHub (OpenClaw) REST API with cursor-based pagination.
///
/// The API returns a JSON object with an `items` array and an optional
/// `nextCursor` string. Each item exposes `slug`, `displayName`, `summary`,
/// `stats.stars`, `tags.latest` (semver), and `owner.handle`.
///
/// Because ClawHub does not expose a public direct-download URL for SKILL.md
/// files, entries are tagged with a `clawhub://` scheme download URL. The
/// install handler in `install_from_catalog` will reject these with a clear
/// message directing the user to the OpenClaw CLI.
async fn fetch_clawhub_api(
    client: &reqwest::Client,
    source: &RegistrySource,
) -> Result<Vec<CatalogEntry>, String> {
    tracing::debug!(
        source_id = %source.id,
        url = %source.url,
        "[skill_registry] fetch_clawhub_api"
    );

    let mut all_entries: Vec<CatalogEntry> = Vec::new();
    let mut cursor: Option<String> = None;
    let max_pages = 5usize; // Safety cap against runaway pagination

    for page in 0..max_pages {
        tracing::debug!(
            page = page,
            cursor = ?cursor,
            "[skill_registry] fetch_clawhub_api page"
        );

        let mut req = client
            .get(&source.url)
            .header("User-Agent", "openhuman-core");

        if let Some(ref c) = cursor {
            req = req.query(&[("cursor", c.as_str())]);
        }

        let response = req
            .send()
            .await
            .map_err(|e| format!("clawhub api fetch failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "clawhub api returned status {}",
                response.status().as_u16()
            ));
        }

        let body = response
            .text()
            .await
            .map_err(|e| format!("failed to read clawhub response: {e}"))?;

        let data: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| format!("invalid clawhub json: {e}"))?;

        let items = data
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        tracing::debug!(
            page = page,
            item_count = items.len(),
            "[skill_registry] fetch_clawhub_api page items"
        );

        for item in &items {
            let slug = match item.get("slug").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => {
                    tracing::trace!("[skill_registry] fetch_clawhub_api skipping item without slug");
                    continue;
                }
            };
            let display_name = item
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or(&slug)
                .to_string();
            let summary = item
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let stars = item
                .pointer("/stats/stars")
                .and_then(|v| v.as_u64())
                .map(|s| s as u32);
            let version = item
                .pointer("/tags/latest")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let author = item
                .pointer("/owner/handle")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // Store the raw epoch-millis as a string — avoids pulling in chrono.
            let updated_at = item
                .get("updatedAt")
                .and_then(|v| v.as_u64())
                .map(|ts| ts.to_string());

            // Use a `clawhub://` scheme so the install handler can detect and
            // redirect users to the OpenClaw CLI.
            let download_url = format!("clawhub://{slug}");

            tracing::trace!(
                slug = %slug,
                stars = ?stars,
                version = ?version,
                "[skill_registry] fetch_clawhub_api entry"
            );

            all_entries.push(CatalogEntry {
                id: slug,
                name: display_name,
                description: summary,
                format: "clawhub".into(),
                author,
                version,
                tags: vec![],
                download_url,
                source_id: source.id.clone(),
                stars,
                updated_at,
            });
        }

        // Advance cursor or stop if no more pages.
        cursor = data
            .get("nextCursor")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        if cursor.is_none() {
            tracing::debug!(
                page = page,
                "[skill_registry] fetch_clawhub_api no nextCursor, stopping"
            );
            break;
        }
    }

    tracing::info!(
        source_id = %source.id,
        total = all_entries.len(),
        "[skill_registry] fetch_clawhub_api complete"
    );
    Ok(all_entries)
}

/// Parse a JSON index file. Expects either:
/// - An array of entry objects directly
/// - An object with a "skills" or "entries" array field
fn parse_index_json(body: &str, source_id: &str) -> Result<Vec<CatalogEntry>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;

    let entries_value = if value.is_array() {
        value
    } else if let Some(arr) = value.get("skills").or(value.get("entries")) {
        arr.clone()
    } else {
        return Err("index JSON must be an array or contain a 'skills'/'entries' field".into());
    };

    let raw_entries: Vec<RawIndexEntry> = serde_json::from_value(entries_value)
        .map_err(|e| format!("failed to parse index entries: {e}"))?;

    let entries = raw_entries
        .into_iter()
        .filter_map(|raw| {
            let name = raw.name.as_deref().or(raw.title.as_deref())?.to_string();
            let description = raw
                .description
                .as_deref()
                .or(raw.summary.as_deref())
                .unwrap_or("")
                .to_string();
            let id = raw
                .id
                .as_deref()
                .or(raw.slug.as_deref())
                .unwrap_or(&name)
                .to_string();

            let download_url = raw
                .download_url
                .as_deref()
                .or(raw.url.as_deref())
                .or(raw.raw_url.as_deref())?
                .to_string();

            let format = raw
                .format
                .as_deref()
                .or(raw.skill_format.as_deref())
                .unwrap_or("openhuman")
                .to_string();

            Some(CatalogEntry {
                id,
                name,
                description,
                format,
                author: raw.author,
                version: raw.version,
                tags: raw.tags.unwrap_or_default(),
                download_url,
                source_id: source_id.to_string(),
                stars: raw.stars.or(raw.star_count),
                updated_at: raw.updated_at.or(raw.last_updated),
            })
        })
        .collect();

    Ok(entries)
}

/// Flexible raw index entry that handles varying field names across registries.
#[derive(Debug, Deserialize)]
struct RawIndexEntry {
    id: Option<String>,
    slug: Option<String>,
    name: Option<String>,
    title: Option<String>,
    description: Option<String>,
    summary: Option<String>,
    format: Option<String>,
    skill_format: Option<String>,
    author: Option<String>,
    version: Option<String>,
    tags: Option<Vec<String>>,
    download_url: Option<String>,
    url: Option<String>,
    raw_url: Option<String>,
    stars: Option<u32>,
    star_count: Option<u32>,
    updated_at: Option<String>,
    last_updated: Option<String>,
}
