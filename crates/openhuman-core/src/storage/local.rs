//! The default layout for the small stores: SQLite document tables inside the
//! database file each store already had.
//!
//! Approvals, devices, notifications and task sources each keep one `.db`
//! file (`<workspace>/approval/approval.db`, `devices/devices.db`,
//! `notifications/notifications.db`, `task_sources/sources.db`). With no
//! storage URL configured ([`super::config::StorageMode::Default`]) a store's
//! first call opens that file with the SQLite storage driver, so its records
//! live in the driver's generic document tables beside the old ones, and
//! [`open`] runs a one-shot import of the old tables' rows.
//!
//! # The import
//!
//! A store describes its old tables with an [`ImportPlan`]: the table names
//! and a reader that turns their rows into documents shaped exactly as the
//! store's own document code writes them. [`open`] then
//!
//! 1. does nothing unless one of the plan's tables exists in the file;
//! 2. reads the rows (through the store's own legacy open path, so an old
//!    schema is migrated forward first);
//! 3. writes each as a document that must not already exist, so a re-run
//!    after a crash never overwrites a record the new code has since changed;
//! 4. renames every old table to `_legacy_<name>`, keeping its rows for one
//!    release, which is also what makes a second open a no-op.
//!
//! The writes and the rename are separate statements (the driver owns its
//! own connection), so the import is resumable rather than a single
//! transaction: a crash between 3 and 4 repeats 3 harmlessly.
//!
//! Records land under [`Scope::local`]: the file already belongs to one
//! workspace, so the scope inside it carries no isolation.
//!
//! Nothing here runs when a backend is installed (the host's explicit URL; the
//! store uses that backend and its scope as before), when the process opted
//! out ([`super::config::CLASSIC`]), in SaaS mode, or in a build without
//! `storage-sqlite`: those keep the legacy tables.

use std::path::Path;
use std::sync::Arc;
#[cfg(feature = "storage-sqlite")]
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{LazyLock, Mutex, PoisonError},
};

use anyhow::{anyhow, Context, Result};
use rusqlite::Connection;
use serde_json::Value;
use tinystoragedrivers::CollectionSpec;

use super::config::{mode, StorageMode};
use super::documents::Repo;
use super::{current_scope, installed, Scope, ScopedStorage, StorageBackend};
use crate::config::Config;

/// One legacy row as a document of the new layout.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDoc {
    /// The collection the store keeps it in.
    pub collection: &'static str,
    /// The document id.
    pub id: String,
    /// The document body.
    pub doc: Value,
}

/// What a store brings from its old tables.
pub struct ImportPlan<'a> {
    /// Log prefix, e.g. `approval::store`.
    pub domain: &'static str,
    /// The old tables. Each present one is renamed to `_legacy_<name>` after
    /// the rows are copied.
    pub tables: &'static [&'static str],
    /// Reads the old rows. Called only when one of `tables` exists.
    pub read: &'a dyn Fn() -> Result<Vec<ImportDoc>>,
}

/// A backend and the scope a store's records live under in it.
#[derive(Clone)]
pub struct Opened {
    /// The storage backend.
    pub backend: Arc<dyn StorageBackend>,
    /// The scope within it.
    pub scope: Scope,
}

impl Opened {
    /// The backend bound to the scope.
    ///
    /// # Errors
    ///
    /// When the backend refuses the scope.
    pub fn scoped(&self) -> Result<ScopedStorage> {
        self.backend
            .for_scope(&self.scope)
            .map_err(|error| anyhow!("open the storage scope: {error}"))
    }
}

/// A database this process has opened, and whether its import finished.
#[cfg(feature = "storage-sqlite")]
struct OpenedDb {
    backend: Arc<dyn StorageBackend>,
    /// The file the backend was opened on, so a deleted or replaced file (a
    /// data reset in the same process) is noticed and reopened.
    identity: Option<FileId>,
    imported: bool,
}

/// What names one file on disk, apart from its path.
#[cfg(feature = "storage-sqlite")]
type FileId = (u64, u64);

#[cfg(all(feature = "storage-sqlite", unix))]
fn file_id(path: &Path) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

#[cfg(all(feature = "storage-sqlite", not(unix)))]
fn file_id(path: &Path) -> Option<FileId> {
    // No inode to compare: existence is the best available signal.
    path.exists().then_some((0, 0))
}

/// Databases opened by this process, by path.
#[cfg(feature = "storage-sqlite")]
static OPENED: LazyLock<Mutex<HashMap<PathBuf, OpenedDb>>> = LazyLock::new(Mutex::default);

/// The storage a small store uses for this call, or `None` for its legacy
/// tables.
///
/// With a backend installed that is the backend, under the acting agent's
/// scope. Otherwise, in [`StorageMode::Default`], it is the SQLite file at
/// `db_path` (opened once per process, its old tables imported first) under
/// [`Scope::local`].
///
/// # Errors
///
/// When the scope cannot be resolved (SaaS mode with no acting agent), the
/// file cannot be opened, or a configured import fails to read its source.
pub fn open(
    config: &Config,
    db_path: &Path,
    collections: fn() -> Vec<CollectionSpec>,
    plan: &ImportPlan<'_>,
) -> Result<Option<Opened>> {
    if let Some(backend) = installed() {
        let scope = current_scope()
            .with_context(|| format!("[{}] resolve the storage scope", plan.domain))?;
        return Ok(Some(Opened { backend, scope }));
    }
    let selected = mode(config);
    if selected != StorageMode::Default {
        // A legacy call may recreate the old tables; the next default call
        // must open and import afresh instead of trusting an earlier import.
        #[cfg(feature = "storage-sqlite")]
        if selected == StorageMode::Classic {
            forget(db_path);
        }
        return Ok(None);
    }
    open_default(db_path, collections, plan)
}

/// [`open`] as a [`Repo`] for the stores built on one.
///
/// # Errors
///
/// As [`open`].
pub fn repo(
    config: &Config,
    db_path: &Path,
    domain: &'static str,
    collections: fn() -> Vec<CollectionSpec>,
    plan: &ImportPlan<'_>,
) -> Result<Option<Repo>> {
    open(config, db_path, collections, plan)?
        .map(|opened| Repo::on(&opened.backend, &opened.scope, domain, collections))
        .transpose()
}

#[cfg(not(feature = "storage-sqlite"))]
fn open_default(
    _db_path: &Path,
    _collections: fn() -> Vec<CollectionSpec>,
    plan: &ImportPlan<'_>,
) -> Result<Option<Opened>> {
    tracing::debug!(
        domain = plan.domain,
        "[storage::local] built without storage-sqlite; keeping the legacy tables"
    );
    Ok(None)
}

#[cfg(feature = "storage-sqlite")]
fn open_default(
    db_path: &Path,
    collections: fn() -> Vec<CollectionSpec>,
    plan: &ImportPlan<'_>,
) -> Result<Option<Opened>> {
    // Held across the import so two threads never both import one file.
    let mut opened = OPENED.lock().unwrap_or_else(PoisonError::into_inner);
    // A file deleted or replaced since it was opened (a data reset in this
    // process) is dropped, so its handle is released and the file is created
    // and imported afresh.
    if opened
        .get(db_path)
        .is_some_and(|entry| entry.identity != file_id(db_path))
    {
        opened.remove(db_path);
    }
    if !opened.contains_key(db_path) {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "[{}] create the directory {}",
                    plan.domain,
                    parent.display()
                )
            })?;
        }
        // Opened by path, not through a URL string, so a path that is not
        // UTF-8 keeps naming the file the legacy tables are in.
        let storage = tinystoragedrivers::sqlite::SqliteStorage::open(db_path)
            .map_err(|error| anyhow!("[{}] open {}: {error}", plan.domain, db_path.display()))?;
        opened.insert(
            db_path.to_path_buf(),
            OpenedDb {
                backend: Arc::new(storage),
                identity: file_id(db_path),
                imported: false,
            },
        );
    }
    let entry = opened.get_mut(db_path).expect("inserted above");
    let handle = Opened {
        backend: Arc::clone(&entry.backend),
        scope: Scope::local(),
    };
    if entry.imported {
        return Ok(Some(handle));
    }
    let count = import(db_path, &handle, collections, plan).with_context(|| {
        format!(
            "[{}] importing the legacy tables failed; they are untouched and the import is \
             retried on the next call (set storage url `{}` to keep using them as they are)",
            plan.domain,
            super::config::CLASSIC
        )
    })?;
    tracing::debug!(
        domain = plan.domain,
        path = %db_path.display(),
        imported = count,
        "[storage::local] opened the default store"
    );
    entry.imported = true;
    Ok(Some(handle))
}

/// Copies `plan`'s old rows into `handle` and retires the old tables.
/// Returns how many documents were written.
#[cfg(feature = "storage-sqlite")]
fn import(
    db_path: &Path,
    handle: &Opened,
    collections: fn() -> Vec<CollectionSpec>,
    plan: &ImportPlan<'_>,
) -> Result<usize> {
    if !db_path.exists() {
        return Ok(0);
    }
    if present_tables(db_path, plan.tables)?.is_empty() {
        return Ok(0);
    }
    let rows = (plan.read)().with_context(|| format!("[{}] read the legacy rows", plan.domain))?;
    // The reader brings an old schema forward, which can create tables that
    // were missing (empty); look again so every one is retired together and
    // none is left to trigger another import.
    let present = present_tables(db_path, plan.tables)?;
    let total = rows.len();
    let repo = Repo::on(&handle.backend, &handle.scope, plan.domain, collections)?;
    let written = repo.run(|docs| async move {
        let mut written = 0usize;
        for row in rows {
            match docs
                .put(
                    row.collection,
                    &row.id,
                    row.doc,
                    tinystoragedrivers::Precondition::Absent,
                )
                .await
            {
                Ok(_) => written += 1,
                // Already there (a resumed import, or newer than the row).
                Err(error) if error.kind() == tinystoragedrivers::ErrorKind::Conflict => {}
                Err(error) => return Err(error),
            }
        }
        Ok(written)
    })?;
    retire_tables(db_path, &present)?;
    tracing::info!(
        domain = plan.domain,
        rows = total,
        written,
        tables = ?present,
        "[storage::local] imported the legacy tables; kept as _legacy_<name>"
    );
    Ok(written)
}

/// Drops every cached default-store handle, so a data reset can delete the
/// files (Windows refuses while a handle is open) and the next call opens
/// and imports afresh.
#[cfg(feature = "storage-sqlite")]
pub fn release_all() {
    OPENED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

/// Forgets that `db_path` was opened, so the next call opens (and imports)
/// it afresh; also how a test models a restart.
#[cfg(feature = "storage-sqlite")]
pub(crate) fn forget(db_path: &Path) {
    OPENED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(db_path);
}

/// The names of `db_path`'s tables, sorted (test inspection).
#[cfg(test)]
pub(crate) fn table_names(db_path: &Path) -> Vec<String> {
    let conn = Connection::open(db_path).unwrap();
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

/// Whether `table` exists on `conn`.
pub fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
}

#[cfg(feature = "storage-sqlite")]
fn present_tables(db_path: &Path, tables: &[&'static str]) -> Result<Vec<&'static str>> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("open {} to look for legacy tables", db_path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let mut present = Vec::new();
    for table in tables {
        if table_exists(&conn, table)? {
            present.push(*table);
        }
    }
    Ok(present)
}

/// Renames each table to `_legacy_<name>` (a numeric suffix if that name is
/// taken, so an earlier retired copy is never overwritten).
#[cfg(feature = "storage-sqlite")]
fn retire_tables(db_path: &Path, tables: &[&str]) -> Result<()> {
    let mut conn = Connection::open(db_path)
        .with_context(|| format!("open {} to retire legacy tables", db_path.display()))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    // Immediate, and each table is checked inside it: another process on the
    // same workspace may have retired it already.
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for table in tables {
        if !table_exists(&tx, table)? {
            continue;
        }
        let mut target = format!("_legacy_{table}");
        let mut n = 1;
        while table_exists(&tx, &target)? {
            n += 1;
            target = format!("_legacy_{table}_{n}");
        }
        // Index names are database-global and survive a rename, which would
        // make a later `CREATE INDEX IF NOT EXISTS` on the recreated legacy
        // table a silent no-op; the archive does not need them.
        let indexes: Vec<String> = tx
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND tbl_name = ?1 AND sql IS NOT NULL")?
            .query_map([table], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for index in indexes {
            let quoted = index.replace('"', "\"\"");
            tx.execute_batch(&format!("DROP INDEX \"{quoted}\""))?;
        }
        tx.execute_batch(&format!("ALTER TABLE \"{table}\" RENAME TO \"{target}\""))
            .with_context(|| format!("rename {table} to {target}"))?;
    }
    // The schema these tables belonged to is gone; a legacy open must not
    // trust a version stamp that says it is initialized.
    tx.pragma_update(None, "user_version", 0)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
