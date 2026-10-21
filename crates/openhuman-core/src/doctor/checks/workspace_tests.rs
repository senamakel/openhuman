use super::*;

use crate::doctor::{DoctorCtx, DoctorOptions, DoctorTier, FindingSeverity};

fn config_in(root: &std::path::Path) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config
}

async fn run_only(config: crate::config::Config, id: &str) -> crate::doctor::DoctorReport {
    crate::doctor::runner::run(
        &DoctorCtx::from_config(config),
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec![id.to_owned()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("selected workspace check should be in the report")
}

#[cfg(unix)]
fn tree_snapshot(root: &std::path::Path) -> Vec<(std::path::PathBuf, Option<Vec<u8>>, u32)> {
    fn collect(
        root: &std::path::Path,
        path: &std::path::Path,
        entries: &mut Vec<(std::path::PathBuf, Option<Vec<u8>>, u32)>,
    ) {
        use std::os::unix::fs::PermissionsExt;

        let metadata = std::fs::symlink_metadata(path).expect("workspace entry metadata");
        let relative = path
            .strip_prefix(root)
            .expect("workspace entry stays under its root")
            .to_path_buf();
        let mode = metadata.permissions().mode();
        if metadata.file_type().is_dir() {
            entries.push((relative, None, mode));
            let mut children = std::fs::read_dir(path)
                .expect("read workspace directory")
                .map(|entry| entry.expect("workspace entry").path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                collect(root, &child, entries);
            }
        } else if metadata.file_type().is_file() {
            entries.push((
                relative,
                Some(std::fs::read(path).expect("read workspace file")),
                mode,
            ));
        } else if metadata.file_type().is_symlink() {
            entries.push((
                relative,
                Some(
                    std::fs::read_link(path)
                        .expect("read workspace symlink")
                        .to_string_lossy()
                        .as_bytes()
                        .to_vec(),
                ),
                mode,
            ));
        }
    }

    use std::os::unix::fs::PermissionsExt;

    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries
}

#[cfg(unix)]
#[tokio::test]
async fn writable_probe_succeeds_and_leaves_the_selected_workspace_unchanged() {
    let fixture = tempfile::tempdir().expect("workspace fixture");
    let config = config_in(fixture.path());
    std::fs::create_dir_all(&config.workspace_dir).expect("create workspace");
    std::fs::write(config.workspace_dir.join("existing.txt"), b"fixture")
        .expect("write fixture file");
    let before = tree_snapshot(&config.workspace_dir);

    let report = run_only(config, "workspace/writable").await;

    assert!(report
        .checks_run
        .iter()
        .any(|id| id == "workspace/writable"));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.check_id == "doctor/selection"));
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "workspace/writable" && finding.severity == FindingSeverity::Pass
    }));
    assert_eq!(before, tree_snapshot(&fixture.path().join("workspace")));
}

#[cfg(unix)]
#[test]
fn workspace_layout_reports_missing_entries_and_low_space_without_repairing_them() {
    let fixture = tempfile::tempdir().expect("workspace fixture");
    let workspace = fixture.path().join("workspace");
    std::fs::create_dir(&workspace).expect("create workspace");
    let before = tree_snapshot(&workspace);

    let findings = layout_findings(&workspace, Some(1));

    let paths = findings
        .iter()
        .filter(|finding| finding.check_id == "workspace/layout")
        .filter_map(|finding| finding.config_path.as_deref())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"workspace_dir/memory"));
    assert!(paths.contains(&"workspace_dir/SYSTEM.md"));
    assert!(paths.contains(&"workspace_dir"));
    assert!(findings.iter().all(|finding| {
        finding.check_id == "workspace/layout"
            && finding.severity == FindingSeverity::Warn
            && !finding.fix_hint.trim().is_empty()
    }));
    assert_eq!(before, tree_snapshot(&workspace));
}

#[cfg(unix)]
#[tokio::test]
async fn workspace_layout_finding_uses_the_registered_native_id() {
    let fixture = tempfile::tempdir().expect("workspace fixture");
    let config = config_in(fixture.path());
    std::fs::create_dir_all(&config.workspace_dir).expect("create workspace");

    let report = run_only(config, "workspace/layout").await;

    assert!(report.checks_run.iter().any(|id| id == "workspace/layout"));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.check_id == "doctor/selection"));
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "workspace/layout" && finding.severity == FindingSeverity::Warn
    }));
}
