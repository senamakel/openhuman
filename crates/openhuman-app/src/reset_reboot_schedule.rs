//! Windows-only fallback for `reset_local_data` (issue #1615).
//!
//! When the in-process `remove_dir_all` step fails because a third-party
//! process (anti-virus, file-indexer, sibling OpenHuman window) still holds
//! an open handle inside the `.openhuman` tree, Windows returns
//! `ERROR_SHARING_VIOLATION` (os error 32) / `ERROR_LOCK_VIOLATION` (33)
//! and the user is stuck — see PR #2395 / #1811, which surface a "close all
//! OpenHuman windows" prompt but cannot break a foreign lock.
//!
//! This module walks the still-present sub-tree depth-first and asks the
//! Windows Session Manager to delete each entry at next boot via
//! `MoveFileExW(src, NULL, MOVEFILE_DELAY_UNTIL_REBOOT)`. The session
//! manager requires that directories be empty when boot-time deletion
//! runs, so children are scheduled before their parent.
//!
//! Reference:
//!   https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw
//!
//! Privileges: `MoveFileExW(.., NULL, MOVEFILE_DELAY_UNTIL_REBOOT)` writes
//! to `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\PendingFileRenameOperations`
//! (the boot-time session manager reads from HKLM, not the per-user hive),
//! so the call **may fail for non-administrator users** with `ERROR_ACCESS_DENIED`.
//! That is by design — Microsoft documents the elevation requirement on the
//! `MOVEFILE_DELAY_UNTIL_REBOOT` flag — and the caller in `lib.rs` handles
//! the failure path gracefully: it preserves the original lock error plus
//! the schedule failure reason and falls back to the "close all OpenHuman
//! windows and try again" guidance from PR #2395 / #1811.

#![cfg(target_os = "windows")]

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

/// Tally of entries handed off to `MoveFileExW`, returned to the caller so
/// it can log and surface (e.g. "scheduled 142 files / 14 dirs for deletion
/// on next reboot") instead of just an opaque "ok".
///
/// `partial` is `true` when the walk aborted mid-tree (e.g. a directory
/// became unreadable, or an individual `MoveFileExW` call failed). In that
/// case `files` / `dirs` represent **only** what was queued before the
/// failure point — useful for support logs to distinguish "everything is
/// queued" from "some of the tree is queued but the rest still needs
/// manual cleanup." Pair with the `Result::Err` returned by
/// [`schedule_path_for_reboot_deletion`] for the cause.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RebootDeletionSchedule {
    pub files: u32,
    pub dirs: u32,
    pub partial: bool,
}

impl RebootDeletionSchedule {
    pub fn total(&self) -> u32 {
        self.files.saturating_add(self.dirs)
    }
}

/// Schedule `path` (and everything under it if it is a directory) for
/// deletion on the next reboot via `MoveFileExW(_, NULL, MOVEFILE_DELAY_UNTIL_REBOOT)`.
///
/// Strategy:
///   * Regular files / symlinks → scheduled directly.
///   * Directories → children scheduled first (depth-first), then the
///     directory itself once its contents are queued.
///
/// `path` not existing on disk yields `Err(RebootDeletionFailure { error: NotFound, .. })` —
/// callers can choose to treat that as a no-op since "nothing to remove" is
/// the same outcome.
///
/// On error the failure carries a partially-populated `RebootDeletionSchedule`
/// (`partial = true`) so the caller can surface "we queued N files and M
/// folders before scheduling failed" instead of just the bare io error.
/// The walk is depth-first, so the counts reflect entries queued *before*
/// the failing step.
pub fn schedule_path_for_reboot_deletion(
    path: &Path,
) -> Result<RebootDeletionSchedule, RebootDeletionFailure> {
    schedule_path_with_scheduler(path, &mut schedule_one)
}

/// Internal seam used by both [`schedule_path_for_reboot_deletion`] (which
/// passes the real `MoveFileExW` step as `scheduler`) and the unit tests
/// (which pass an injectable `Ok(())` stub so the traversal/counting logic
/// can be exercised on every dev machine without needing administrator
/// rights or actually queuing reboot-time deletions).
fn schedule_path_with_scheduler<F>(
    path: &Path,
    scheduler: &mut F,
) -> Result<RebootDeletionSchedule, RebootDeletionFailure>
where
    F: FnMut(&Path) -> io::Result<()>,
{
    let metadata = std::fs::symlink_metadata(path).map_err(|error| RebootDeletionFailure {
        error,
        partial: RebootDeletionSchedule {
            partial: true,
            ..RebootDeletionSchedule::default()
        },
    })?;
    let mut summary = RebootDeletionSchedule::default();
    match schedule_inner(path, &metadata, &mut summary, scheduler) {
        Ok(()) => Ok(summary),
        Err(error) => {
            summary.partial = true;
            Err(RebootDeletionFailure {
                error,
                partial: summary,
            })
        }
    }
}

/// Pair of `(io::Error, partial schedule)` returned when the depth-first
/// walk aborts mid-tree. The `partial` field records what was queued via
/// `MoveFileExW` *before* the failure point so the caller can include the
/// counts in user-facing copy and support logs ("123 files / 7 folders
/// were queued for the next reboot before scheduling failed: <reason>").
#[derive(Debug)]
pub struct RebootDeletionFailure {
    pub error: io::Error,
    pub partial: RebootDeletionSchedule,
}

impl std::fmt::Display for RebootDeletionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for RebootDeletionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

fn schedule_inner<F>(
    path: &Path,
    metadata: &std::fs::Metadata,
    summary: &mut RebootDeletionSchedule,
    scheduler: &mut F,
) -> io::Result<()>
where
    F: FnMut(&Path) -> io::Result<()>,
{
    // Symlinked directories must NOT be descended into — the lock lives
    // on the link target, not the link itself, and following would queue
    // unrelated paths for deletion. Treat symlinks (file or dir) as a
    // single leaf entry.
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let child_meta = entry.metadata()?;
            schedule_inner(&entry.path(), &child_meta, summary, scheduler)?;
        }
        scheduler(path)?;
        summary.dirs = summary.dirs.saturating_add(1);
    } else {
        scheduler(path)?;
        summary.files = summary.files.saturating_add(1);
    }
    Ok(())
}

fn schedule_one(path: &Path) -> io::Result<()> {
    // `MoveFileExW + MOVEFILE_DELAY_UNTIL_REBOOT` requires absolute paths —
    // the session manager runs at boot before any working directory is
    // established, so a relative path cannot be resolved. The call sites
    // in `reset_local_data` already resolve paths via the core's
    // `config_get_data_paths` RPC (which returns absolute paths) so this
    // is currently a no-op in release builds; the assert catches a future
    // regression that wires a different caller in without thinking.
    debug_assert!(
        path.is_absolute(),
        "MoveFileExW + DELAY_UNTIL_REBOOT requires an absolute path, got {}",
        path.display()
    );
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer that outlives the
    // call. The destination pointer is `NULL`, which (combined with
    // `MOVEFILE_DELAY_UNTIL_REBOOT`) tells Windows to delete (rather than
    // rename) the source at the next boot. `MoveFileExW` returns BOOL —
    // non-zero on success.
    let ok = unsafe { MoveFileExW(wide.as_ptr(), std::ptr::null(), MOVEFILE_DELAY_UNTIL_REBOOT) };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "reset_reboot_schedule_tests.rs"]
mod tests;
