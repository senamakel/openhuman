//! Persistence for the skill registry: cached catalog entries and custom sources.
//!
//! The cache lives at `~/.openhuman/skill-registry/cache.json` with a TTL.
//! Custom sources are persisted at `~/.openhuman/skill-registry/sources.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::types::{CatalogEntry, RegistrySource};

const CACHE_DIR: &str = "skill-registry";
const CACHE_FILE: &str = "cache.json";
const SOURCES_FILE: &str = "sources.json";
const CACHE_TTL_SECS: u64 = 3600;

#[derive(Debug, Serialize, Deserialize)]
pub struct CatalogCache {
    pub entries: Vec<CatalogEntry>,
    pub fetched_at_epoch: u64,
}

fn registry_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".openhuman").join(CACHE_DIR))
}

pub fn load_cached_catalog() -> Option<Vec<CatalogEntry>> {
    let dir = registry_dir()?;
    let path = dir.join(CACHE_FILE);
    let data = std::fs::read_to_string(&path).ok()?;
    let cache: CatalogCache = serde_json::from_str(&data).ok()?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if now.saturating_sub(cache.fetched_at_epoch) > CACHE_TTL_SECS {
        tracing::debug!(
            age_secs = now - cache.fetched_at_epoch,
            "[skill_registry] cache expired"
        );
        return None;
    }

    tracing::debug!(
        count = cache.entries.len(),
        "[skill_registry] loaded catalog from cache"
    );
    Some(cache.entries)
}

pub fn save_catalog_cache(entries: &[CatalogEntry]) {
    let Some(dir) = registry_dir() else { return };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(error = %e, "[skill_registry] failed to create cache dir");
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cache = CatalogCache {
        entries: entries.to_vec(),
        fetched_at_epoch: now,
    };
    let path = dir.join(CACHE_FILE);
    match serde_json::to_string_pretty(&cache) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!(error = %e, "[skill_registry] failed to write cache");
            }
        }
        Err(e) => tracing::warn!(error = %e, "[skill_registry] failed to serialize cache"),
    }
}

pub fn load_custom_sources() -> Vec<RegistrySource> {
    let Some(dir) = registry_dir() else {
        return Vec::new();
    };
    let path = dir.join(SOURCES_FILE);
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&data).unwrap_or_default()
}

pub fn save_custom_sources(sources: &[RegistrySource]) {
    let Some(dir) = registry_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(SOURCES_FILE);
    if let Ok(json) = serde_json::to_string_pretty(sources) {
        if let Err(e) = std::fs::write(&path, json) {
            tracing::warn!(error = %e, "[skill_registry] failed to write sources");
        }
    }
}

pub fn clear_cache() {
    let Some(dir) = registry_dir() else { return };
    let path = dir.join(CACHE_FILE);
    let _ = std::fs::remove_file(&path);
    tracing::debug!("[skill_registry] cache cleared");
}
