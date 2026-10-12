//! Whether a running Chrome holds a profile folder, and claiming one for a
//! browser about to start.
//!
//! Chrome lets one browser at a time use a profile folder: a second one
//! started on it hands its window to the first and exits, so a browser task
//! or session given the persistent profile while another Chrome holds it
//! could not start its browser. A task waits up to [`PROFILE_WAIT`] for a
//! Chrome that is closing (the one the task before it closes as it ends), a
//! session up to [`SESSION_PROFILE_WAIT`], and otherwise each runs in a fresh
//! profile.
//!
//! Chrome takes the folder's lock only once it has started, so two browsers
//! starting together would each find the folder free. A browser of this
//! process therefore claims the folder first ([`claim_within`]), and the
//! claim stands until its Chrome holds the folder itself. Claims are this
//! process's own; between processes, Chrome's lock is the only arbiter.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long a task waits for the Chrome holding its persistent profile to
/// let it go before it runs in a fresh profile instead.
pub(crate) const PROFILE_WAIT: Duration = Duration::from_secs(5);

/// How long a browser session waits for the same, before it opens in a
/// fresh profile: shorter than a task's, as a person waits on it, and the
/// Chrome holding the profile is most often another conversation's, which
/// does not close.
pub(crate) const SESSION_PROFILE_WAIT: Duration = Duration::from_secs(1);

/// How often the profile is looked at while waiting.
const PROFILE_POLL: Duration = Duration::from_millis(250);

/// How long a claim stands while its Chrome has not taken the folder's lock:
/// well past the time a browser takes to start. A session's Chrome starts
/// before its open returns, a task's as the task starts, beside its planning.
const CLAIM_GRACE: Duration = Duration::from_secs(30);

/// The profile folders browsers of this process may still be starting on,
/// with when each was claimed.
static CLAIMS: LazyLock<Mutex<HashMap<PathBuf, Instant>>> = LazyLock::new(Mutex::default);

/// Whether a running Chrome holds the profile folder `path`, by the lock it
/// keeps there: on macOS and Linux a `SingletonLock` link naming the host and
/// the process holding it, on Windows a `lockfile` held open. A link naming
/// another host is taken as held, as Chrome takes it; one naming a process
/// that is gone is a lock left by a crash, and free.
pub(crate) fn in_use(path: &Path) -> bool {
    #[cfg(windows)]
    {
        let lock = path.join("lockfile");
        lock.exists() && std::fs::OpenOptions::new().write(true).open(&lock).is_err()
    }
    #[cfg(not(windows))]
    {
        let Ok(target) = std::fs::read_link(path.join("SingletonLock")) else {
            return false;
        };
        held_by(&target.to_string_lossy())
    }
}

/// Whether a `SingletonLock` link's target, `<host>-<pid>`, names a live
/// process on this host, or another host.
#[cfg(not(windows))]
fn held_by(target: &str) -> bool {
    let Some((host, pid)) = target.rsplit_once('-') else {
        return false;
    };
    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    if sysinfo::System::host_name().is_some_and(|here| here != host) {
        return true;
    }
    let pid = sysinfo::Pid::from_u32(pid);
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some()
}

/// Claims the profile folder `path` for a browser about to start, waiting up
/// to `wait` for no Chrome, and no other browser of this process starting on
/// it, to hold it; whether it was claimed. The claim stands until that
/// browser's Chrome holds the folder, [`release`], or [`CLAIM_GRACE`] passes.
pub(crate) async fn claim_within(path: &Path, wait: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if claim(path) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(PROFILE_POLL).await;
    }
}

/// Claims `path` when neither a Chrome nor a claim holds it; whether it did.
fn claim(path: &Path) -> bool {
    let mut claims = CLAIMS.lock().unwrap_or_else(PoisonError::into_inner);
    if in_use(path) {
        // Chrome's own lock guards the folder now, so a claim made for that
        // Chrome has done its work.
        claims.remove(path);
        return false;
    }
    if claims
        .get(path)
        .is_some_and(|claimed| claimed.elapsed() < CLAIM_GRACE)
    {
        return false;
    }
    claims.insert(path.to_path_buf(), Instant::now());
    true
}

/// Gives up the claim on `path`: its browser did not start, or has started
/// and holds the folder itself.
pub(crate) fn release(path: &Path) {
    CLAIMS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(path);
}

#[cfg(test)]
#[path = "browser_profile_tests.rs"]
mod tests;
