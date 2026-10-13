//! Host side of `ai.tinyhumans.tinycomputer.SiteMemoryHost`: where the
//! TinyComputer module keeps what finished browser tasks learned about a site.
//!
//! The module owns the rules (reuse, fact-value filtering, expiry, size
//! limits); the host owns only the bytes: one file per site in
//! `<workspace>/state/computer/sites/<site>.json`, readable by its owner
//! alone. Site memory goes to the module and nowhere else: this file never
//! publishes it to the event bus, the agent's memory or a prompt.

use std::path::{Path, PathBuf};

use tinybus::ObjectPath;
use tinycomputer_bus::site_memory::{
    ForgetSiteRequest, ForgetSiteResponse, LoadSiteRequest, LoadSiteResponse, SaveSiteRequest,
    SaveSiteResponse, SiteMemory, INTERFACE, OBJECT_PATH,
};

/// Largest site file read or written: anything bigger was not written here.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// The directory site files live in.
pub(super) fn sites_dir(workspace: &Path) -> PathBuf {
    workspace.join("state").join("computer").join("sites")
}

/// Whether `site` is a plain lower-cased host name and so a safe file stem.
fn plain_site(site: &str) -> bool {
    !site.is_empty()
        && site.len() <= 253
        && !site.starts_with('.')
        && !site.contains("..")
        && site
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'-')
}

/// The site files under one directory.
pub(super) struct SiteStore {
    dir: PathBuf,
}

impl SiteStore {
    pub(super) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, site: &str) -> std::io::Result<PathBuf> {
        if plain_site(site) {
            Ok(self.dir.join(format!("{site}.json")))
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "site is not a plain host name",
            ))
        }
    }

    /// `site`'s memory; a missing file is empty, and an unreadable one is set
    /// aside as `<site>.json.corrupt` and read as empty.
    pub(super) async fn load(&self, site: &str) -> std::io::Result<SiteMemory> {
        let path = self.path(site)?;
        let read = match tokio::fs::metadata(&path).await {
            Ok(m) if m.len() <= MAX_FILE_BYTES => tokio::fs::read(&path).await,
            Ok(_) => Err(std::io::Error::other("larger than a site file can be")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(SiteMemory::default()),
            Err(e) => Err(e),
        };
        match read.and_then(|b| serde_json::from_slice(&b).map_err(std::io::Error::other)) {
            Ok(memory) => Ok(memory),
            Err(error) => {
                tracing::warn!(%error, "[site-memory-host] unreadable site file set aside");
                let _ = tokio::fs::rename(&path, path.with_extension("json.corrupt")).await;
                Ok(SiteMemory::default())
            }
        }
    }

    /// Replaces `site`'s memory; an empty one removes the file. Returns
    /// whether the file holds the memory as sent.
    pub(super) async fn save(&self, site: &str, memory: &SiteMemory) -> std::io::Result<bool> {
        let path = self.path(site)?;
        if memory.is_empty() {
            remove(&path).await?;
            return Ok(true);
        }
        let bytes = serde_json::to_vec_pretty(memory).map_err(std::io::Error::other)?;
        if u64::try_from(bytes.len()).map_or(true, |n| n > MAX_FILE_BYTES) {
            tracing::warn!("[site-memory-host] site memory over the file limit; not saved");
            return Ok(false);
        }
        write_private(&path, &bytes).await?;
        Ok(true)
    }

    /// Forgets `site`, or every site; returns how many were forgotten.
    pub(super) async fn forget(&self, site: Option<&str>) -> std::io::Result<u64> {
        if let Some(site) = site {
            let path = self.path(site)?;
            let aside = remove(&path.with_extension("json.corrupt")).await?;
            remove(&path.with_extension("json.tmp")).await?;
            let kept = remove(&path).await?;
            return Ok(u64::from(kept || aside));
        }
        let mut entries = match tokio::fs::read_dir(&self.dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let mut sites = std::collections::BTreeSet::new();
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(site) = name
                    .strip_suffix(".json")
                    .or_else(|| name.strip_suffix(".json.corrupt"))
                {
                    sites.insert(site.to_owned());
                }
                remove(&entry.path()).await?;
            }
        }
        Ok(sites.len() as u64)
    }
}

async fn remove(path: &Path) -> std::io::Result<bool> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

/// Writes through a temporary file, in a directory and file only their owner
/// can read: plans and elements tell where a person browses.
async fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use tokio::io::AsyncWriteExt;
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).await?;
        }
    }
    let staged = path.with_extension("json.tmp");
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&staged).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .await?;
    }
    file.write_all(bytes).await?;
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&staged, path).await
}

#[derive(Clone)]
struct SiteMemoryHost;

async fn store() -> tinybus::Result<SiteStore> {
    let config = crate::config::rpc::load_config_with_timeout()
        .await
        .map_err(|e| method_error(format!("config unavailable: {e}")))?;
    Ok(SiteStore::new(sites_dir(&config.workspace_dir)))
}

#[tinybus::interface(name = "ai.tinyhumans.tinycomputer.SiteMemoryHost")]
impl SiteMemoryHost {
    /// One site's memory; empty for a site with nothing kept.
    async fn load(&self, request: LoadSiteRequest) -> tinybus::Result<LoadSiteResponse> {
        let memory = store()
            .await?
            .load(&request.site)
            .await
            .map_err(method_error)?;
        Ok(LoadSiteResponse { memory })
    }

    /// Replaces one site's memory.
    async fn save(&self, request: SaveSiteRequest) -> tinybus::Result<SaveSiteResponse> {
        let saved = store()
            .await?
            .save(&request.site, &request.memory)
            .await
            .map_err(method_error)?;
        Ok(SaveSiteResponse { saved })
    }

    /// Forgets one site, or every site.
    async fn forget(&self, request: ForgetSiteRequest) -> tinybus::Result<ForgetSiteResponse> {
        let forgotten = store()
            .await?
            .forget(request.site.as_deref())
            .await
            .map_err(method_error)?;
        Ok(ForgetSiteResponse { forgotten })
    }
}

fn method_error(error: impl std::fmt::Display) -> tinybus::Error {
    tinybus::Error::MethodFailed {
        name: "ai.tinyhumans.tinycomputer.Error.SiteMemoryHost".to_string(),
        message: error.to_string(),
    }
}

pub(super) async fn install(connection: &tinybus::Connection) -> tinybus::Result<()> {
    connection
        .serve_at(ObjectPath::new(OBJECT_PATH)?, SiteMemoryHost)
        .await?;
    // The interface name is served at the module's own bus name; the module
    // reaches it through the host connection already holding the object.
    tracing::debug!(interface = INTERFACE, "[site-memory-host] served");
    Ok(())
}

#[cfg(test)]
#[path = "site_memory_host_tests.rs"]
mod tests;
