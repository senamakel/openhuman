use super::*;

/// Test-only no-op scheduler. Lets the traversal/counting tests run
/// in any user context (incl. non-administrator) by sidestepping the
/// `MoveFileExW + MOVEFILE_DELAY_UNTIL_REBOOT` call that would
/// otherwise need HKLM write access. We capture the call order so a
/// regression in depth-first ordering would surface as a wrong path
/// sequence here, even though the real OS-side scheduling stays
/// out of the test process.
fn noop_scheduler(
    captured: &mut Vec<std::path::PathBuf>,
) -> impl FnMut(&Path) -> io::Result<()> + '_ {
    move |path: &Path| {
        captured.push(path.to_path_buf());
        Ok(())
    }
}

#[test]
fn schedule_walks_files_then_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("reset-target");
    std::fs::create_dir_all(root.join("nested")).expect("mkdir nested");
    std::fs::write(root.join("a.txt"), b"a").expect("write a.txt");
    std::fs::write(root.join("nested").join("b.txt"), b"b").expect("write b.txt");

    let mut captured = Vec::new();
    let summary =
        schedule_path_with_scheduler(&root, &mut noop_scheduler(&mut captured)).expect("walk");

    // root + nested == 2 dirs; a.txt + nested/b.txt == 2 files
    assert_eq!(summary.files, 2, "expected 2 files queued, got {summary:?}");
    assert_eq!(summary.dirs, 2, "expected 2 dirs queued, got {summary:?}");
    assert_eq!(summary.total(), 4);
    assert!(!summary.partial, "Ok must not flag partial");

    // Depth-first: a parent must only appear after all of its children.
    // Track per-path positions in the call order, then assert each
    // directory sits after every entry whose path is rooted inside it.
    let position = |needle: &Path| -> usize {
        captured
            .iter()
            .position(|p| p == needle)
            .unwrap_or_else(|| panic!("missing {} in {captured:?}", needle.display()))
    };
    let root_pos = position(&root);
    let nested_pos = position(&root.join("nested"));
    let a_pos = position(&root.join("a.txt"));
    let b_pos = position(&root.join("nested").join("b.txt"));
    assert!(
        b_pos < nested_pos,
        "b.txt must be scheduled before its parent (nested)"
    );
    assert!(
        nested_pos < root_pos && a_pos < root_pos,
        "nested + a.txt must be scheduled before root"
    );
}

#[test]
fn schedule_single_file_reports_one_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("solo.txt");
    std::fs::write(&file, b"x").expect("write solo.txt");

    let mut captured = Vec::new();
    let summary =
        schedule_path_with_scheduler(&file, &mut noop_scheduler(&mut captured)).expect("walk");

    assert_eq!(
        summary,
        RebootDeletionSchedule {
            files: 1,
            dirs: 0,
            partial: false,
        }
    );
    assert_eq!(captured, vec![file]);
}

#[test]
fn schedule_missing_path_yields_not_found() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = dir.path().join("does-not-exist");

    let mut captured = Vec::new();
    let failure = schedule_path_with_scheduler(&missing, &mut noop_scheduler(&mut captured))
        .expect_err("missing");
    assert_eq!(failure.error.kind(), io::ErrorKind::NotFound);
    // Nothing scheduled, but partial flag still reports "did not
    // complete" so callers can distinguish from a clean success.
    assert!(failure.partial.partial);
    assert_eq!(failure.partial.files, 0);
    assert_eq!(failure.partial.dirs, 0);
    assert!(
        captured.is_empty(),
        "no scheduling should have been attempted: {captured:?}"
    );
}

#[test]
fn schedule_empty_dir_counts_one_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let empty = dir.path().join("empty-target");
    std::fs::create_dir(&empty).expect("mkdir empty-target");

    let mut captured = Vec::new();
    let summary =
        schedule_path_with_scheduler(&empty, &mut noop_scheduler(&mut captured)).expect("walk");

    assert_eq!(
        summary,
        RebootDeletionSchedule {
            files: 0,
            dirs: 1,
            partial: false,
        }
    );
    assert_eq!(captured, vec![empty]);
}

#[test]
fn schedule_propagates_scheduler_failure_with_partial_counts() {
    // Simulate the non-administrator MoveFileExW failure path: the
    // walk visits children successfully, then the third call (the
    // parent dir) errors out. The Err must carry the leaf counts
    // queued before the failure so the caller can surface "we did
    // get X files scheduled before this hit the registry wall."
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("partial-fail");
    std::fs::create_dir_all(&root).expect("mkdir");
    std::fs::write(root.join("a.txt"), b"a").expect("write a.txt");
    std::fs::write(root.join("b.txt"), b"b").expect("write b.txt");

    let root_path = root.clone();
    let mut count = 0usize;
    let mut scheduler = |path: &Path| -> io::Result<()> {
        count += 1;
        if path == root_path {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "simulated non-admin MoveFileExW failure",
            ))
        } else {
            Ok(())
        }
    };

    let failure =
        schedule_path_with_scheduler(&root, &mut scheduler).expect_err("scheduler failed");
    assert_eq!(failure.error.kind(), io::ErrorKind::PermissionDenied);
    assert!(failure.partial.partial);
    // Both leaf files were scheduled before the parent-dir call failed.
    assert_eq!(failure.partial.files, 2, "got {:?}", failure.partial);
    assert_eq!(failure.partial.dirs, 0, "got {:?}", failure.partial);
    // Sanity: scheduler was called for both leaves + the parent.
    assert_eq!(count, 3);
}
