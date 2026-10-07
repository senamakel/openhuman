use super::*;

// --- Capability features: tools-shell / tools-fs-write / tools-exec /
// tools-system / composio -------------------------------------------------
//
// Each gate gets a present/absent pair. The present half runs in the
// contributor and product builds; the absent half runs in the gates-off lane
// (`--no-default-features`), which is where a registration that slipped past
// its gate would show up.

const SHELL_FAMILY: &[&str] = &["shell"];
const FS_WRITE_FAMILY: &[&str] = &["file_write", "edit", "apply_patch", "csv_export", "curl"];
const EXEC_FAMILY: &[&str] = &[
    "python_exec",
    "run_tests",
    "run_linter",
    "git_operations",
    "install_tool",
    "detect_tools",
];
const SYSTEM_FAMILY: &[&str] = &[
    "service_start",
    "service_stop",
    "service_restart",
    "service_shutdown",
    "service_install",
    "service_uninstall",
    "update_apply",
    "proxy_config",
    "daemon_host_prefs_set",
    "workspace_update_persona",
];
/// Read-only siblings of the system family. They are not part of the gate and
/// must survive it in every build.
const SYSTEM_READS: &[&str] = &[
    "service_status",
    "update_check",
    "daemon_host_prefs_get",
    "workspace_read_persona",
];
const COMPOSIO_FAMILY: &[&str] = &[
    "composio_list_toolkits",
    "composio_list_connections",
    "composio_authorize",
    "composio_connect",
    "composio_list_tools",
    "composio_execute",
];

fn registry_names() -> Vec<String> {
    let tmp = TempDir::new().unwrap();
    tool_names(&expansion_tools_for(&tmp))
}

fn assert_absent(names: &[String], family: &[&str], feature: &str) {
    for name in family {
        assert!(
            !names.iter().any(|n| n == name),
            "`{name}` must not be registered with `{feature}` compiled out; got: {names:?}"
        );
    }
}

/// Composio registers in direct mode without an app session, which is the one
/// configuration a unit test can satisfy.
fn composio_direct_mode_names() -> Vec<String> {
    let tmp = TempDir::new().unwrap();
    let mut cfg = test_config(&tmp);
    cfg.composio.mode = crate::config::schema::COMPOSIO_MODE_DIRECT.into();
    cfg.composio.api_key = Some("ck_direct_test".into());
    tool_names(&integration_tools_for_config(&tmp, &cfg))
}

fn connected_gmail() -> Vec<crate::agent::prompts::ConnectedIntegration> {
    vec![crate::agent::prompts::ConnectedIntegration {
        toolkit: "gmail".into(),
        description: "Gmail".into(),
        tools: vec![crate::agent::prompts::ConnectedIntegrationTool {
            name: "GMAIL_SEND_EMAIL".into(),
            description: "Send an email".into(),
            parameters: None,
        }],
        connected: true,
        ..Default::default()
    }]
}

#[test]
#[cfg(feature = "tools-shell")]
fn shell_family_registered_when_feature_on() {
    assert_contains_all(&registry_names(), SHELL_FAMILY);
    let baseline = tool_names(&default_tools(Arc::new(SecurityPolicy::default())));
    assert_contains_all(&baseline, &["shell", "file_read"]);
}

#[test]
#[cfg(not(feature = "tools-shell"))]
fn shell_family_absent_when_feature_off() {
    assert_absent(&registry_names(), SHELL_FAMILY, "tools-shell");
    let baseline = tool_names(&default_tools(Arc::new(SecurityPolicy::default())));
    assert_absent(&baseline, SHELL_FAMILY, "tools-shell");
    assert_contains_all(&baseline, &["file_read"]);
}

#[test]
#[cfg(feature = "tools-fs-write")]
fn fs_write_family_registered_when_feature_on() {
    assert_contains_all(&registry_names(), FS_WRITE_FAMILY);
    let baseline = tool_names(&default_tools(Arc::new(SecurityPolicy::default())));
    assert_contains_all(&baseline, &["file_write"]);
}

#[test]
#[cfg(not(feature = "tools-fs-write"))]
fn fs_write_family_absent_when_feature_off() {
    let names = registry_names();
    assert_absent(&names, FS_WRITE_FAMILY, "tools-fs-write");
    // Reading and navigating are not part of the gate.
    assert_contains_all(&names, &["file_read", "grep", "glob", "list"]);
    let baseline = tool_names(&default_tools(Arc::new(SecurityPolicy::default())));
    assert_absent(&baseline, &["file_write"], "tools-fs-write");
}

#[test]
#[cfg(feature = "tools-exec")]
fn exec_family_registered_when_feature_on() {
    assert_contains_all(&registry_names(), EXEC_FAMILY);
}

#[test]
#[cfg(not(feature = "tools-exec"))]
fn exec_family_absent_when_feature_off() {
    let names = registry_names();
    assert_absent(&names, EXEC_FAMILY, "tools-exec");
    assert_contains_all(&names, &["read_diff"]);
}

#[test]
#[cfg(feature = "tools-system")]
fn system_family_registered_when_feature_on() {
    let names = registry_names();
    assert_contains_all(&names, SYSTEM_FAMILY);
    assert_contains_all(&names, SYSTEM_READS);
}

#[test]
#[cfg(not(feature = "tools-system"))]
fn system_family_absent_when_feature_off() {
    let names = registry_names();
    assert_absent(&names, SYSTEM_FAMILY, "tools-system");
    assert_contains_all(&names, SYSTEM_READS);
}

#[test]
#[cfg(feature = "composio")]
fn composio_family_registered_when_feature_on() {
    assert_contains_all(&composio_direct_mode_names(), COMPOSIO_FAMILY);
    let actions = crate::tools::orchestrator_tools::collect_deferred_integration_actions(
        &connected_gmail(),
    );
    let names = tool_names(&actions);
    assert_eq!(names, vec!["GMAIL_SEND_EMAIL".to_string()]);
}

#[test]
#[cfg(not(feature = "composio"))]
fn composio_family_absent_when_feature_off() {
    let names = composio_direct_mode_names();
    assert!(
        !names.iter().any(|n| n.starts_with("composio")),
        "no composio tool may register with `composio` compiled out; got: {names:?}"
    );
    let actions = crate::tools::orchestrator_tools::collect_deferred_integration_actions(
        &connected_gmail(),
    );
    assert!(
        actions.is_empty(),
        "per-action integration tools must not be built with `composio` compiled out"
    );
}

// --- Runtime classification of the capability families ---------------------

#[test]
fn capability_families_classify_out_of_platform() {
    use crate::core::all::DomainGroup;
    for name in [
        "shell",
        "run_tests",
        "run_linter",
        "git_operations",
        "install_tool",
        "detect_tools",
    ] {
        assert_eq!(tool_group(name), DomainGroup::Exec, "`{name}`");
    }
    for name in FS_WRITE_FAMILY {
        assert_eq!(tool_group(name), DomainGroup::Filesystem, "`{name}`");
    }
    for name in [
        "service_start",
        "service_stop",
        "service_restart",
        "service_shutdown",
        "service_install",
        "service_uninstall",
        "service_status",
        "update_apply",
        "update_check",
        "proxy_config",
        "daemon_host_prefs_set",
        "daemon_host_prefs_get",
    ] {
        assert_eq!(tool_group(name), DomainGroup::System, "`{name}`");
    }
    // Families that already had a home keep it.
    assert_eq!(tool_group("python_exec"), DomainGroup::Runtimes);
    assert_eq!(tool_group("workspace_update_persona"), DomainGroup::Config);
    // The browser tools exist only with the module host compiled in.
    assert_eq!(tool_group("browser"), DomainGroup::Modules);
    assert_eq!(tool_group("browser_open"), DomainGroup::Modules);
}

#[test]
fn domain_set_presets_place_the_capability_families() {
    use crate::core::all::DomainGroup;
    use crate::core::runtime::DomainSet;
    let families = [DomainGroup::Exec, DomainGroup::Filesystem, DomainGroup::System];
    for g in families {
        assert!(DomainSet::full().allows(g), "full() keeps {g:?}");
        // `embedded()` had these tools through `platform: true`; it keeps them.
        assert!(DomainSet::embedded().allows(g), "embedded() keeps {g:?}");
        assert!(!DomainSet::harness().allows(g), "harness() drops {g:?}");
        assert!(!DomainSet::kernel().allows(g), "kernel() drops {g:?}");
        assert!(!DomainSet::none().allows(g), "none() drops {g:?}");
    }
    let mut no_shell = DomainSet::full();
    no_shell.exec = false;
    assert!(!no_shell.intersect(&DomainSet::full()).allows(DomainGroup::Exec));
    assert!(no_shell.allows(DomainGroup::Filesystem));
}

/// A runtime `DomainSet` with `exec` off drops the shell from the assembled
/// registry while keeping the rest of the platform surface.
#[tokio::test]
#[cfg(feature = "tools-shell")]
async fn domain_set_exec_off_drops_shell_from_the_registry() {
    use crate::core::runtime::context::CoreContext;
    use crate::core::runtime::DomainSet;

    let mut domains = DomainSet::full();
    domains.exec = false;
    let ctx = CoreContext::for_test(domains, None);
    let names = CoreContext::scope(ctx, async { registry_names() }).await;
    assert!(!names.iter().any(|n| n == "shell"), "got: {names:?}");
    assert_contains_all(&names, &["file_read", "todo"]);
}
