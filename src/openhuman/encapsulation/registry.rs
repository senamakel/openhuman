//! Jail registry — manage many encapsulated workspaces side-by-side.
//!
//! A [`JailRegistry`] is rooted at a single base directory (typically
//! `~/.openhuman/jails/` or `<workspace>/jails/`) and owns every active
//! jail underneath it. Each jail has:
//!
//! - A stable **id** (UUID-ish, used in paths and the index).
//! - A user-visible **label** (free text, displayed in UI, used for
//!   AppContainer profile derivation on Windows).
//! - A **directory** at `<base>/<id>/` that the [`crate::openhuman::encapsulation::Jail`]
//!   is rooted in.
//! - **Metadata**: created/updated timestamps, backend used at create
//!   time, optional notes.
//!
//! All metadata is persisted to `<base>/index.json`. The on-disk index is
//! the source of truth; the in-memory state is rebuilt from it on every
//! [`JailRegistry::open`].
//!
//! Concurrency: a `std::sync::Mutex` guards mutations. The index file
//! is rewritten atomically via write-temp + rename. This is sufficient
//! for the single-process core; if we ever want multi-process registry
//! access we'll need OS-level file locking — explicit non-goal for now.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::jail::{Jail, JailBackend};
use super::{default_backend, encapsulate_with};

/// Metadata persisted for each jail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JailRecord {
    pub id: String,
    pub label: String,
    pub dir: PathBuf,
    pub backend_at_create: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    /// id → record. BTreeMap so list() is deterministically ordered.
    records: BTreeMap<String, JailRecord>,
    #[serde(default)]
    schema_version: u32,
}

const INDEX_SCHEMA_VERSION: u32 = 1;
const INDEX_FILENAME: &str = "index.json";

/// Top-level manager for multiple jailed workspaces.
#[derive(Debug)]
pub struct JailRegistry {
    base: PathBuf,
    index: Mutex<Index>,
}

impl JailRegistry {
    /// Open (or create) a registry rooted at `base`. The directory is
    /// created if it does not exist; the index file is loaded if present
    /// and seeded blank otherwise.
    pub fn open(base: impl AsRef<Path>) -> io::Result<Self> {
        let base = base.as_ref().to_path_buf();
        fs::create_dir_all(&base)?;
        let idx_path = base.join(INDEX_FILENAME);
        let index = if idx_path.exists() {
            let raw = fs::read(&idx_path)?;
            serde_json::from_slice::<Index>(&raw)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?
        } else {
            Index {
                records: BTreeMap::new(),
                schema_version: INDEX_SCHEMA_VERSION,
            }
        };
        Ok(Self {
            base,
            index: Mutex::new(index),
        })
    }

    /// Root directory of this registry.
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Create a new jail directory. Returns the persisted record.
    pub fn create(&self, label: impl Into<String>) -> io::Result<JailRecord> {
        let label = label.into();
        let id = generate_id();
        let dir = self.base.join(&id);
        fs::create_dir_all(&dir)?;
        let now = now_unix();
        let record = JailRecord {
            id: id.clone(),
            label,
            dir,
            backend_at_create: default_backend().name().to_string(),
            created_at_unix: now,
            updated_at_unix: now,
            notes: None,
        };
        let mut idx = self.index.lock().unwrap();
        idx.records.insert(id, record.clone());
        self.persist(&idx)?;
        Ok(record)
    }

    /// Look up a jail by id.
    pub fn get(&self, id: &str) -> Option<JailRecord> {
        self.index.lock().unwrap().records.get(id).cloned()
    }

    /// List every active jail. Deterministic order (by id).
    pub fn list(&self) -> Vec<JailRecord> {
        self.index
            .lock()
            .unwrap()
            .records
            .values()
            .cloned()
            .collect()
    }

    /// Search by label substring (case-insensitive). Useful for UI.
    pub fn find_by_label(&self, needle: &str) -> Vec<JailRecord> {
        let needle = needle.to_lowercase();
        self.index
            .lock()
            .unwrap()
            .records
            .values()
            .filter(|r| r.label.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    }

    /// Rename: changes the *label* only. The directory id stays put so
    /// existing path references keep working. AppContainer profile names
    /// are derived from `id` (stable), not `label`, for the same reason.
    pub fn rename(&self, id: &str, new_label: impl Into<String>) -> io::Result<JailRecord> {
        let new_label = new_label.into();
        let mut idx = self.index.lock().unwrap();
        let record = idx
            .records
            .get_mut(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no jail {id}")))?;
        record.label = new_label;
        record.updated_at_unix = now_unix();
        let cloned = record.clone();
        self.persist(&idx)?;
        Ok(cloned)
    }

    /// Update the free-form notes field.
    pub fn set_notes(&self, id: &str, notes: Option<String>) -> io::Result<JailRecord> {
        let mut idx = self.index.lock().unwrap();
        let record = idx
            .records
            .get_mut(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no jail {id}")))?;
        record.notes = notes;
        record.updated_at_unix = now_unix();
        let cloned = record.clone();
        self.persist(&idx)?;
        Ok(cloned)
    }

    /// Delete a jail. Removes both the directory and the index entry.
    ///
    /// Refuses to delete a jail whose directory is not under `self.base` —
    /// belt-and-suspenders against a corrupted index pointing at `/`.
    pub fn delete(&self, id: &str) -> io::Result<()> {
        let mut idx = self.index.lock().unwrap();
        let record = idx
            .records
            .remove(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no jail {id}")))?;

        let resolved = record
            .dir
            .canonicalize()
            .unwrap_or_else(|_| record.dir.clone());
        let resolved_base = self
            .base
            .canonicalize()
            .unwrap_or_else(|_| self.base.clone());
        if !resolved.starts_with(&resolved_base) {
            // Put it back; this index entry is suspicious — don't touch
            // anything on disk.
            idx.records.insert(id.to_string(), record);
            self.persist(&idx)?;
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing to delete jail outside registry base: {}",
                    resolved.display()
                ),
            ));
        }

        if record.dir.exists() {
            fs::remove_dir_all(&record.dir)?;
        }
        self.persist(&idx)?;
        Ok(())
    }

    /// Drop *all* jails. Convenience for tests / "reset everything"
    /// flows. Returns the number of jails removed.
    pub fn clear(&self) -> io::Result<usize> {
        let ids: Vec<String> = self.index.lock().unwrap().records.keys().cloned().collect();
        let n = ids.len();
        for id in ids {
            self.delete(&id)?;
        }
        Ok(n)
    }

    /// Spawn `cmd` inside the named jail, using the default backend.
    /// Convenience wrapper — the same effect as
    /// `encapsulate(&Jail::new(record.dir, record.label), cmd)`.
    pub fn spawn_in(&self, id: &str, cmd: Command) -> io::Result<Child> {
        let record = self
            .get(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no jail {id}")))?;
        let mut jail = Jail::new(&record.dir, &record.label);
        jail.canonicalize()?;
        default_backend().spawn(&jail, cmd)
    }

    /// Same as [`spawn_in`] but with a caller-supplied backend.
    pub fn spawn_in_with(
        &self,
        id: &str,
        backend: &dyn JailBackend,
        cmd: Command,
    ) -> io::Result<Child> {
        let record = self
            .get(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("no jail {id}")))?;
        let mut jail = Jail::new(&record.dir, &record.label);
        jail.canonicalize()?;
        encapsulate_with(backend, &jail, cmd)
    }

    /// Atomic-rename write of the index. Falls back to direct write on
    /// Windows if rename-over fails (Windows traditionally refused
    /// rename-over-existing, though modern NTFS/Win10 supports it).
    fn persist(&self, idx: &Index) -> io::Result<()> {
        let path = self.base.join(INDEX_FILENAME);
        let tmp = self.base.join(format!("{INDEX_FILENAME}.tmp"));
        let bytes = serde_json::to_vec_pretty(idx)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(&tmp, &bytes)?;
        match fs::rename(&tmp, &path) {
            Ok(()) => Ok(()),
            Err(_) => {
                // Fallback: direct overwrite.
                fs::write(&path, &bytes)?;
                let _ = fs::remove_file(&tmp);
                Ok(())
            }
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Short, URL-safe id. Not cryptographically random — we use it as a
/// directory name, not a token.
fn generate_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let ts = now_unix();
    format!("j{ts:x}{n:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "openhuman-registry-{}-{}-{}",
            tag,
            std::process::id(),
            now_unix()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn create_list_get_roundtrip() {
        let base = tempdir("crud");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("alpha").unwrap();
        let b = reg.create("beta").unwrap();
        assert_ne!(a.id, b.id);
        assert!(a.dir.exists());
        assert!(b.dir.exists());
        let listed = reg.list();
        assert_eq!(listed.len(), 2);
        assert_eq!(reg.get(&a.id).unwrap().label, "alpha");
        assert_eq!(reg.get(&b.id).unwrap().label, "beta");
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn rename_changes_label_not_id_or_dir() {
        let base = tempdir("rename");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("old").unwrap();
        let renamed = reg.rename(&a.id, "new").unwrap();
        assert_eq!(renamed.id, a.id);
        assert_eq!(renamed.dir, a.dir);
        assert_eq!(renamed.label, "new");
        assert!(renamed.updated_at_unix >= a.updated_at_unix);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn delete_removes_dir_and_record() {
        let base = tempdir("delete");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("doomed").unwrap();
        let dir = a.dir.clone();
        assert!(dir.exists());
        reg.delete(&a.id).unwrap();
        assert!(!dir.exists());
        assert!(reg.get(&a.id).is_none());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn delete_missing_errors() {
        let base = tempdir("missing");
        let reg = JailRegistry::open(&base).unwrap();
        let err = reg.delete("nope").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn index_persists_across_reopen() {
        let base = tempdir("persist");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("persistent").unwrap();
        drop(reg);
        let reg2 = JailRegistry::open(&base).unwrap();
        let listed = reg2.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, a.id);
        assert_eq!(listed[0].label, "persistent");
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn find_by_label_substring() {
        let base = tempdir("find");
        let reg = JailRegistry::open(&base).unwrap();
        reg.create("agent-alpha").unwrap();
        reg.create("agent-beta").unwrap();
        reg.create("tool-gamma").unwrap();
        assert_eq!(reg.find_by_label("AGENT").len(), 2);
        assert_eq!(reg.find_by_label("gamma").len(), 1);
        assert_eq!(reg.find_by_label("nope").len(), 0);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn clear_drops_everything() {
        let base = tempdir("clear");
        let reg = JailRegistry::open(&base).unwrap();
        reg.create("a").unwrap();
        reg.create("b").unwrap();
        reg.create("c").unwrap();
        let n = reg.clear().unwrap();
        assert_eq!(n, 3);
        assert_eq!(reg.list().len(), 0);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn parallel_jails_have_distinct_dirs() {
        let base = tempdir("parallel");
        let reg = JailRegistry::open(&base).unwrap();
        let jails: Vec<_> = (0..5)
            .map(|i| reg.create(format!("p{i}")).unwrap())
            .collect();
        let mut dirs: Vec<_> = jails.iter().map(|r| r.dir.clone()).collect();
        dirs.sort();
        dirs.dedup();
        assert_eq!(dirs.len(), 5);
        for r in &jails {
            assert!(r.dir.exists());
        }
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn set_notes_roundtrips() {
        let base = tempdir("notes");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("with-notes").unwrap();
        assert!(a.notes.is_none());
        let updated = reg.set_notes(&a.id, Some("hello".into())).unwrap();
        assert_eq!(updated.notes.as_deref(), Some("hello"));
        let cleared = reg.set_notes(&a.id, None).unwrap();
        assert!(cleared.notes.is_none());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn set_notes_on_missing_id_errors() {
        let base = tempdir("notes-missing");
        let reg = JailRegistry::open(&base).unwrap();
        let err = reg.set_notes("nope", Some("x".into())).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn rename_on_missing_id_errors() {
        let base = tempdir("rename-missing");
        let reg = JailRegistry::open(&base).unwrap();
        let err = reg.rename("nope", "x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn delete_twice_second_is_not_found() {
        let base = tempdir("delete-twice");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("once").unwrap();
        reg.delete(&a.id).unwrap();
        let err = reg.delete(&a.id).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn spawn_in_with_missing_id_errors() {
        let base = tempdir("spawn-missing");
        let reg = JailRegistry::open(&base).unwrap();
        let err = reg
            .spawn_in_with("nope", &super::super::NoopBackend, Command::new("true"))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn spawn_in_uses_default_backend() {
        let base = tempdir("spawn-default");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("def").unwrap();
        let cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.args(["/C", "exit"]);
            c
        } else {
            Command::new("true")
        };
        // spawn_in() uses the default platform backend — must succeed on
        // whatever sandbox is auto-picked (or noop fallback).
        let mut child = reg.spawn_in(&a.id, cmd).unwrap();
        let _ = child.wait().unwrap();
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn clear_on_empty_registry_is_zero() {
        let base = tempdir("empty-clear");
        let reg = JailRegistry::open(&base).unwrap();
        assert_eq!(reg.clear().unwrap(), 0);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn find_by_label_on_empty_registry() {
        let base = tempdir("empty-find");
        let reg = JailRegistry::open(&base).unwrap();
        assert!(reg.find_by_label("anything").is_empty());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn open_creates_base_directory_if_missing() {
        let base = std::env::temp_dir().join(format!(
            "oh-reg-mkdir-{}-{}",
            std::process::id(),
            now_unix()
        ));
        assert!(!base.exists());
        let reg = JailRegistry::open(&base).unwrap();
        assert!(base.exists());
        assert!(reg.list().is_empty());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn corrupt_index_returns_invalid_data() {
        let base = tempdir("corrupt");
        fs::write(base.join("index.json"), b"this is not json").unwrap();
        let err = JailRegistry::open(&base).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn persist_writes_index_file() {
        let base = tempdir("persist-file");
        let reg = JailRegistry::open(&base).unwrap();
        reg.create("x").unwrap();
        let path = base.join("index.json");
        assert!(path.exists());
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"label\": \"x\""));
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn base_accessor_returns_open_dir() {
        let base = tempdir("base-accessor");
        let reg = JailRegistry::open(&base).unwrap();
        assert_eq!(reg.base(), base.as_path());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn delete_refuses_path_outside_base() {
        // Corrupt the index so a record points at /tmp directly (outside
        // base). delete() should refuse and roll back.
        let base = tempdir("escape");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("escape").unwrap();
        // Reach into the mutex and rewrite the dir to escape base.
        {
            let mut idx = reg.index.lock().unwrap();
            idx.records.get_mut(&a.id).unwrap().dir = std::env::temp_dir();
        }
        let err = reg.delete(&a.id).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        // /tmp itself must still exist; the record must still be there
        // (delete rolled it back).
        assert!(std::env::temp_dir().exists());
        assert!(reg.get(&a.id).is_some());
        fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn spawn_in_uses_record_dir_as_root() {
        let base = tempdir("spawn");
        let reg = JailRegistry::open(&base).unwrap();
        let a = reg.create("spawn-target").unwrap();
        let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "true" });
        if cfg!(windows) {
            cmd.args(["/C", "exit"]);
        }
        // Use noop so this works regardless of OS sandbox availability.
        let mut child = reg
            .spawn_in_with(&a.id, &super::super::NoopBackend, cmd)
            .unwrap();
        let status = child.wait().unwrap();
        assert!(status.success() || cfg!(windows));
        fs::remove_dir_all(&base).ok();
    }
}
