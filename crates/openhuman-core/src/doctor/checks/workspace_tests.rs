use super::*;

use crate::core::runtime::{CoreContext, DomainSet};
use crate::core::slots::Extensions;
use crate::doctor::{DoctorCtx, DoctorOptions, DoctorTier, FindingSeverity};

fn config_in(root: &std::path::Path) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config
}

async fn run_only(config: crate::config::Config, id: &str) -> crate::doctor::DoctorReport {
    let core = CoreContext::for_test_with_extensions(DomainSet::full(), None, Extensions::new());
    CoreContext::scope(core, async move {
        crate::doctor::runner::run(
            &DoctorCtx::from_config(config),
            DoctorOptions {
                tier: DoctorTier::Static,
                only: vec![id.to_owned()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("selected workspace check should be in the report")
}

#[cfg(unix)]
struct ReadOnlyWorkspace {
    temp: tempfile::TempDir,
    original_mode: u32,
}

#[cfg(unix)]
impl ReadOnlyWorkspace {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("read-only workspace fixture");
        let path = temp.path();
        let original_mode = std::fs::metadata(path)
            .expect("workspace metadata")
            .permissions()
            .mode();
        let mut permissions = std::fs::metadata(path)
            .expect("workspace metadata")
            .permissions();
        permissions.set_mode(original_mode & !0o222);
        std::fs::set_permissions(path, permissions).expect("make workspace read-only");
        Self {
            temp,
            original_mode,
        }
    }

    fn path(&self) -> &std::path::Path {
        self.temp.path()
    }
}

#[cfg(unix)]
impl Drop for ReadOnlyWorkspace {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(self.temp.path())
            .expect("workspace exists while restoring permissions")
            .permissions();
        permissions.set_mode(self.original_mode);
        std::fs::set_permissions(self.temp.path(), permissions)
            .expect("restore permissions before temporary cleanup");
    }
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
#[tokio::test]
async fn read_only_workspace_is_reported_by_the_native_writable_check() {
    let workspace = ReadOnlyWorkspace::new();
    let before = tree_snapshot(workspace.path());
    let mut config = crate::config::Config::default();
    config.workspace_dir = workspace.path().to_path_buf();

    let report = run_only(config, "workspace/writable").await;
    let after = tree_snapshot(workspace.path());

    assert_eq!(
        before, after,
        "the static probe must clean up its scratch file"
    );
    assert!(report
        .checks_run
        .iter()
        .any(|id| id == "workspace/writable"));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.check_id == "doctor/selection"));
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "workspace/writable")
        .expect("native workspace check produces a finding");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding
        .message
        .contains(&workspace.path().display().to_string()));
    assert!(!finding.fix_hint.trim().is_empty());
}

#[tokio::test]
async fn missing_workspace_is_an_error_without_creating_it() {
    let fixture = tempfile::tempdir().expect("workspace fixture");
    let config = config_in(fixture.path());
    let workspace = config.workspace_dir.clone();

    let report = run_only(config, "workspace/writable").await;

    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "workspace/writable")
        .expect("missing workspace finding");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert_eq!(finding.config_path.as_deref(), Some("workspace_dir"));
    assert!(!workspace.exists(), "doctor must not create the workspace");
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
#[test]
fn complete_workspace_layout_passes_at_the_disk_space_threshold() {
    let fixture = tempfile::tempdir().expect("workspace fixture");
    let workspace = fixture.path().join("workspace");
    std::fs::create_dir_all(workspace.join("memory")).expect("create memory directory");
    std::fs::write(workspace.join("SYSTEM.md"), "fixture prompt")
        .expect("write workspace prompt");

    let findings = layout_findings(&workspace, Some(512));

    assert!(findings.is_empty(), "complete layout should be clean: {findings:#?}");
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
