use super::*;

/// A fresh profile folder under the system temporary directory.
fn folder(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "openhuman-browser-profile-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn a_folder_no_chrome_has_locked_is_free() {
    let path = folder("unlocked");
    assert!(!in_use(&path));
    assert!(!in_use(&path.join("missing")));
    let _ = std::fs::remove_dir_all(&path);
}

#[cfg(not(windows))]
#[test]
fn a_lock_naming_a_live_process_here_holds_the_folder() {
    let here = sysinfo::System::host_name().unwrap_or_default();
    let path = folder("live");
    std::os::unix::fs::symlink(
        format!("{here}-{}", std::process::id()),
        path.join("SingletonLock"),
    )
    .unwrap();
    assert!(in_use(&path));
    let _ = std::fs::remove_dir_all(&path);
}

#[cfg(not(windows))]
#[test]
fn a_lock_a_crashed_chrome_left_behind_is_free_and_another_hosts_is_held() {
    let here = sysinfo::System::host_name().unwrap_or_default();
    // No process has this id: the lock outlived its Chrome.
    let path = folder("stale");
    std::os::unix::fs::symlink(format!("{here}-4294967"), path.join("SingletonLock")).unwrap();
    assert!(!in_use(&path));
    let _ = std::fs::remove_dir_all(&path);

    let path = folder("elsewhere");
    std::os::unix::fs::symlink(
        format!("not-{here}-{}", std::process::id()),
        path.join("SingletonLock"),
    )
    .unwrap();
    assert!(in_use(&path), "a profile another computer holds is held");
    let _ = std::fs::remove_dir_all(&path);

    // A link with no process in it is no lock Chrome wrote.
    assert!(!held_by("no-number-here"));
    assert!(!held_by("nodash"));
}

#[cfg(not(windows))]
#[tokio::test]
async fn waiting_ends_as_soon_as_the_folder_is_let_go() {
    let here = sysinfo::System::host_name().unwrap_or_default();
    let path = folder("closing");
    let lock = path.join("SingletonLock");
    std::os::unix::fs::symlink(format!("{here}-{}", std::process::id()), &lock).unwrap();
    assert!(!claim_within(&path, Duration::from_millis(300)).await);
    let letting_go = {
        let lock = lock.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            std::fs::remove_file(lock).unwrap();
        })
    };
    assert!(claim_within(&path, PROFILE_WAIT).await);
    letting_go.await.unwrap();
    release(&path);
    let _ = std::fs::remove_dir_all(&path);
}

#[tokio::test]
async fn a_folder_claimed_for_a_starting_browser_is_held_until_released() {
    let path = folder("claimed");
    assert!(claim_within(&path, Duration::ZERO).await);
    assert!(
        !claim_within(&path, Duration::from_millis(50)).await,
        "a browser still starting on it holds it"
    );
    release(&path);
    assert!(claim_within(&path, Duration::ZERO).await);
    release(&path);
    let _ = std::fs::remove_dir_all(&path);
}

#[tokio::test]
async fn of_two_browsers_starting_together_one_claims_the_folder() {
    // Both look before either Chrome has locked the folder.
    let path = folder("together");
    let (first, second) = tokio::join!(
        claim_within(&path, Duration::from_millis(50)),
        claim_within(&path, Duration::from_millis(50)),
    );
    assert!(first ^ second, "first {first}, second {second}");
    release(&path);
    let _ = std::fs::remove_dir_all(&path);
}

#[cfg(not(windows))]
#[tokio::test]
async fn a_claim_ends_when_its_chrome_holds_the_folder() {
    let here = sysinfo::System::host_name().unwrap_or_default();
    let path = folder("taken-up");
    assert!(claim_within(&path, Duration::ZERO).await);
    // Its Chrome has started and locked the folder.
    let lock = path.join("SingletonLock");
    std::os::unix::fs::symlink(format!("{here}-{}", std::process::id()), &lock).unwrap();
    assert!(!claim_within(&path, Duration::ZERO).await);
    // That Chrome has closed: nothing of the claim is left to hold the folder.
    std::fs::remove_file(&lock).unwrap();
    assert!(claim_within(&path, Duration::ZERO).await);
    release(&path);
    let _ = std::fs::remove_dir_all(&path);
}
