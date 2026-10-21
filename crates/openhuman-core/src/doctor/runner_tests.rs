use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use tempfile::TempDir;

use crate::config::Config;
use crate::core::runtime::{ComponentSet, CoreContext, DomainSet};
use crate::core::slots::Extensions;
use async_trait::async_trait;

use super::contract::{
    DoctorCheck, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorTier, Finding, FindingSeverity,
};
use super::registry::{DoctorCheckSource, register_check_source_in};
use super::runner;
use super::test_support::{FakeCheck, FakeCheckSource};

fn isolated_config(root: &Path) -> Config {
    let mut config = Config::default();
    config.workspace_dir = root.join("workspace");
    config.action_dir = root.join("actions");
    config.config_path = root.join("config.toml");
    config
}

fn scoped_config_context(config: Config, extensions: Extensions) -> Arc<CoreContext> {
    CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions).derive_with(
        crate::core::runtime::ContextOverlay::new(config, DomainSet::full(), Default::default())
            .without_user_skill_roots(),
    )
}

#[test]
fn no_unwritten_daemon_state_check_is_registered() {
    let checks = super::registry::registered_checks().expect("built-in checks are valid");

    assert!(
        checks.iter().all(|check| check.id() != "runtime/daemon"),
        "the doctor must not advertise a daemon state check whose file is never written"
    );
}

struct BlockingProbe {
    id: String,
    timeout: Duration,
    active: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
}

#[async_trait]
impl DoctorCheck for BlockingProbe {
    fn id(&self) -> &str {
        &self.id
    }

    fn category(&self) -> &str {
        "test"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Connect
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn is_blocking(&self) -> bool {
        true
    }

    async fn detect(&self, _ctx: &DoctorCtx) -> Vec<Finding> {
        Vec::new()
    }

    fn detect_blocking(&self, _ctx: &DoctorCtx) -> Option<Vec<Finding>> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
        self.active.fetch_sub(1, Ordering::SeqCst);
        Some(vec![Finding::pass(
            &self.id,
            "The blocking fake check completed.",
        )])
    }
}

struct BlockingProbeSource(Vec<Arc<dyn DoctorCheck>>);

impl DoctorCheckSource for BlockingProbeSource {
    fn id(&self) -> &str {
        "doctor-blocking-probes"
    }

    fn checks(&self) -> Vec<Arc<dyn DoctorCheck>> {
        self.0.clone()
    }
}

#[tokio::test]
async fn a_context_check_source_is_reported_with_its_check_id() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-runner-source",
            vec![FakeCheck::pass("hosted/ready")],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: vec!["hosted/ready".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("registered check source runs");

    let config_source: DoctorConfigSource = report.config_source;
    assert_eq!(config_source, DoctorConfigSource::Profile);
    assert!(report.checks_run.iter().any(|id| id == "hosted/ready"));
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "hosted/ready")
    );
}

#[tokio::test]
async fn hierarchical_check_ids_are_registered_and_selected_exactly() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-hierarchical-id-source",
            vec![FakeCheck::pass("inference/live/role")],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                tier: DoctorTier::Live,
                only: vec!["inference/live/role".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("hierarchical id is valid and selected as one exact id");

    assert_eq!(report.checks_run, vec!["inference/live/role".to_owned()]);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "inference/live/role")
    );
}

#[tokio::test]
async fn a_timed_out_check_is_an_error_without_stopping_other_checks() {
    let extensions = Extensions::new();
    let slow = FakeCheck::pass("test/slow")
        .sleep_for(Duration::from_millis(100))
        .with_timeout(Duration::from_millis(10));
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-timeout-source",
            vec![slow, FakeCheck::pass("test/fast")],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: vec!["test/slow".into(), "test/fast".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("timed out detector is returned as a finding");

    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "test/slow"
            && finding.severity == FindingSeverity::Error
            && finding.message.contains("timed out")
    }));
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "test/fast" && finding.severity == FindingSeverity::Pass
    }));
}

#[tokio::test]
async fn timed_out_blocking_checks_keep_the_concurrency_slot_until_the_worker_exits() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let ids = (0..16)
        .map(|index| format!("test/blocking-{index}"))
        .collect::<Vec<_>>();
    let checks = ids
        .iter()
        .map(|id| {
            Arc::new(BlockingProbe {
                id: id.clone(),
                timeout: Duration::from_millis(20),
                active: Arc::clone(&active),
                peak: Arc::clone(&peak),
            }) as Arc<dyn DoctorCheck>
        })
        .collect();
    let extensions = Extensions::new();
    register_check_source_in(&extensions, BlockingProbeSource(checks));
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: ids,
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("all blocking checks return timed-out findings");

    assert!(
        active.load(Ordering::SeqCst) > 0,
        "the report should return while timed-out blocking workers are still finishing"
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        while active.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("blocking workers eventually finish");
    assert_eq!(report.summary.error, 16);
    assert!(peak.load(Ordering::SeqCst) <= 8);
}

#[tokio::test]
async fn memory_checks_are_skipped_when_the_memory_component_is_disabled() {
    let mut components = ComponentSet::all();
    components.memory = false;
    let context = DoctorCtx::from_config(Config::default()).with_components(components);
    let report = runner::run(
        &context,
        DoctorOptions {
            tier: DoctorTier::Connect,
            only: vec!["memory/engine".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("component-gated check is represented as skipped");

    assert_eq!(report.checks_skipped, vec!["memory/engine".to_owned()]);
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "memory/engine" && finding.severity == FindingSeverity::Skipped
    }));
}

#[tokio::test]
async fn checks_above_the_requested_tier_are_skipped() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-tier-source",
            vec![FakeCheck::pass("inference/role-completion").with_tier(DoctorTier::Live)],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                tier: DoctorTier::Connect,
                only: vec!["inference/role-completion".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("above-tier check is represented as skipped");

    assert_eq!(
        report.checks_skipped,
        vec!["inference/role-completion".to_owned()]
    );
    assert_eq!(report.checks_run, Vec::<String>::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "inference/role-completion"
            && finding.severity == FindingSeverity::Skipped
            && finding.duration_ms == 0
    }));
}

#[tokio::test]
async fn only_filters_exact_ids_and_unknown_ids_get_a_selection_finding() {
    let extensions = Extensions::new();
    register_check_source_in(
        &extensions,
        FakeCheckSource::new(
            "doctor-selection-source",
            vec![
                FakeCheck::pass("config/temperature"),
                FakeCheck::pass("test/other"),
            ],
        ),
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(Config::default()),
            DoctorOptions {
                only: vec![
                    "config/temperature".into(),
                    "test/other".into(),
                    "missing/id".into(),
                ],
                skip: vec!["test/other".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("unknown selection is a report finding");

    assert_eq!(report.checks_run, vec!["config/temperature".to_owned()]);
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "doctor/selection"
                && finding.severity == FindingSeverity::Error)
    );
    assert_eq!(report.schema_version, 2);
    assert_eq!(report.summary.pass, 1);
    assert_eq!(report.summary.error, 1);
    assert!(!report.ok);
}

#[tokio::test]
async fn module_status_checks_are_registered_with_their_native_id() {
    let workspace = tempfile::tempdir().expect("create isolated doctor fixture");
    let report = runner::run(
        &DoctorCtx::from_config(isolated_config(workspace.path())),
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec!["modules/tinymcp".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("module check is part of the doctor report");

    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check_id == "doctor/selection"),
        "module check ids are exact registered ids, not unknown selectors: {:?}",
        report.findings
    );
    assert!(report.checks_run.iter().any(|id| id == "modules/tinymcp"));
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "modules/tinymcp")
    );
}

#[tokio::test]
async fn mcp_server_check_ids_come_from_the_selected_config() {
    let workspace_a = tempfile::tempdir().expect("create ambient config fixture");
    let workspace_b = tempfile::tempdir().expect("create selected config fixture");
    let mut config_a = isolated_config(workspace_a.path());
    let mut server_a = crate::config::McpServerConfig::default();
    server_a.name = "server-a".into();
    config_a.mcp_client.servers.push(server_a);
    let mut config_b = isolated_config(workspace_b.path());
    let mut server_b = crate::config::McpServerConfig::default();
    server_b.name = "server-b".into();
    config_b.mcp_client.servers.push(server_b);

    // The ambient context represents configuration A. The selected doctor
    // input is B, so its dynamic check id must be used for exact selection.
    let extensions_a = Extensions::new();
    register_check_source_in(
        &extensions_a,
        FakeCheckSource::new("doctor-mcp-config-a", vec![FakeCheck::pass("mcp/server-a")]),
    );
    let context_a = scoped_config_context(config_a, extensions_a);
    let report = CoreContext::scope(context_a, async {
        runner::run(
            &DoctorCtx::from_config(config_b),
            DoctorOptions {
                // Static ceiling proves the id is registered without probing
                // or starting any MCP server.
                tier: DoctorTier::Static,
                only: vec!["mcp/server-b".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("configured MCP check is represented in the report");

    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check_id == "doctor/selection"),
        "the selected config's id must be registered: {:?}",
        report.findings
    );
    assert!(
        report.checks_skipped.iter().any(|id| id == "mcp/server-b")
            || report.checks_run.iter().any(|id| id == "mcp/server-b")
    );
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.check_id == "mcp/server-b")
    );
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check_id == "mcp/server-a")
    );
}

#[tokio::test]
async fn duplicate_mcp_names_report_an_ambiguous_definition() {
    let workspace = tempfile::tempdir().expect("create isolated MCP config fixture");
    let mut config = isolated_config(workspace.path());
    let mut first = crate::config::McpServerConfig::default();
    first.name = "server-a".into();
    first.command = "first-server".into();
    let mut second = crate::config::McpServerConfig::default();
    second.name = "server-a".into();
    second.endpoint = "https://example.invalid/mcp".into();
    config.mcp_client.servers.extend([first, second]);

    let report = runner::run(
        &DoctorCtx::from_config(config),
        DoctorOptions {
            only: vec!["mcp/server-a".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("duplicate server names are reported under one valid id");

    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "mcp/server-a")
        .expect("one finding identifies the duplicate MCP server");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding.message.to_ascii_lowercase().contains("duplicate"));
    assert!(finding.fix_hint.contains("Remove"));
}

#[tokio::test]
async fn malformed_skill_discovery_reports_a_warning_with_its_path() {
    let workspace = tempfile::tempdir().expect("create isolated skill fixture");
    let config = isolated_config(workspace.path());
    let skill_dir = config.workspace_dir.join("skills").join("broken-skill");
    std::fs::create_dir_all(&skill_dir).expect("create skill fixture directory");
    let skill_path = skill_dir.join("SKILL.md");
    std::fs::write(
        &skill_path,
        "---\nname: [unterminated\n---\nA test skill.\n",
    )
    .expect("write malformed skill manifest");

    std::fs::create_dir_all(&config.workspace_dir).expect("create isolated workspace");
    let context = scoped_config_context(config.clone(), Extensions::new());
    let report = CoreContext::scope(context, async {
        runner::run(
            &DoctorCtx::from_config(config),
            DoctorOptions {
                tier: DoctorTier::Static,
                only: vec!["skills/discovery".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("skill discovery check is part of the doctor report");

    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "skills/discovery")
        .expect("malformed skill produces a discovery finding");
    assert_eq!(finding.severity, FindingSeverity::Warn);
    let expected_path = skill_path.to_string_lossy();
    assert_eq!(finding.config_path.as_deref(), Some(expected_path.as_ref()));
    assert!(finding.message.to_ascii_lowercase().contains("manifest"));
}

#[tokio::test]
async fn sandbox_backend_check_is_registered_without_running_above_static_tier() {
    let workspace = tempfile::tempdir().expect("create isolated doctor fixture");
    let mut config = isolated_config(workspace.path());
    config.sandbox.backend = crate::config::SandboxBackend::Docker;
    let mut components = ComponentSet::all();
    components.sandbox = false;
    let report = runner::run(
        &DoctorCtx::from_config(config).with_components(components),
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec!["sandbox/backend".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("sandbox check is represented without invoking Docker");

    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check_id == "doctor/selection"),
        "sandbox check id must be registered: {:?}",
        report.findings
    );
    assert_eq!(report.checks_skipped, vec!["sandbox/backend".to_owned()]);
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "sandbox/backend" && finding.severity == FindingSeverity::Skipped
    }));
}

#[cfg(unix)]
struct ReadOnlyWorkspace {
    temp: TempDir,
    original_mode: u32,
}

#[cfg(unix)]
impl ReadOnlyWorkspace {
    fn new() -> Self {
        let temp = TempDir::new().expect("temporary workspace");
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

    fn path(&self) -> &Path {
        self.temp.path()
    }
}

#[cfg(unix)]
impl Drop for ReadOnlyWorkspace {
    fn drop(&mut self) {
        let mut permissions = std::fs::metadata(self.temp.path())
            .expect("workspace still exists while restoring permissions")
            .permissions();
        permissions.set_mode(self.original_mode);
        std::fs::set_permissions(self.temp.path(), permissions)
            .expect("restore workspace permissions before temporary cleanup");
    }
}

#[cfg(unix)]
#[derive(Debug, PartialEq, Eq)]
enum WorkspaceEntry {
    Directory(u32),
    File(u32, Vec<u8>),
    Symlink(PathBuf),
    Other,
}

#[cfg(unix)]
fn workspace_snapshot(root: &Path) -> Vec<(PathBuf, WorkspaceEntry)> {
    fn collect(root: &Path, path: &Path, entries: &mut Vec<(PathBuf, WorkspaceEntry)>) {
        let metadata = std::fs::symlink_metadata(path).expect("workspace entry metadata");
        let relative = path
            .strip_prefix(root)
            .expect("workspace entry stays under its root")
            .to_path_buf();
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            entries.push((
                relative,
                WorkspaceEntry::Directory(metadata.permissions().mode()),
            ));
            let mut children = std::fs::read_dir(path)
                .expect("read workspace directory")
                .map(|entry| entry.expect("workspace directory entry").path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                collect(root, &child, entries);
            }
        } else if file_type.is_file() {
            entries.push((
                relative,
                WorkspaceEntry::File(
                    metadata.permissions().mode(),
                    std::fs::read(path).expect("read workspace file"),
                ),
            ));
        } else if file_type.is_symlink() {
            entries.push((
                relative,
                WorkspaceEntry::Symlink(std::fs::read_link(path).expect("read workspace link")),
            ));
        } else {
            entries.push((relative, WorkspaceEntry::Other));
        }
    }

    let mut entries = Vec::new();
    collect(root, root, &mut entries);
    entries
}

#[cfg(unix)]
#[tokio::test]
async fn read_only_workspace_is_reported_by_the_native_writable_check() {
    let workspace = ReadOnlyWorkspace::new();
    let before = workspace_snapshot(workspace.path());
    let mut config = Config::default();
    config.workspace_dir = workspace.path().to_path_buf();
    let extensions = Extensions::new();
    let core = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);

    let report = CoreContext::scope(core, async {
        runner::run(
            &DoctorCtx::from_config(config),
            DoctorOptions {
                tier: DoctorTier::Static,
                only: vec!["workspace/writable".into()],
                ..DoctorOptions::default()
            },
        )
        .await
    })
    .await
    .expect("selected native check returns a report");
    let after = workspace_snapshot(workspace.path());

    assert_eq!(before, after, "the static probe must clean up its scratch file");
    assert!(
        !report
            .findings
            .iter()
            .any(|finding| finding.check_id == "doctor/selection"),
        "workspace/writable must be registered rather than treated as an unknown selector: {report:?}"
    );
    assert!(report.checks_run.iter().any(|id| id == "workspace/writable"));
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.check_id == "workspace/writable")
        .expect("native workspace check produces a finding");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(
        finding.message.contains(&workspace.path().display().to_string()),
        "finding identifies the failing workspace path: {finding:?}"
    );
    assert!(!finding.fix_hint.trim().is_empty());
}
