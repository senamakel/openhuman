use super::*;
use crate::doctor::contract::{DoctorOptions, FindingSeverity};

struct FixedStatuses(Result<Option<Vec<tinymcp_bus::ConnStatus>>, McpStatusFailure>);

#[async_trait]
impl McpStatusProbe for FixedStatuses {
    async fn statuses(
        &self,
        _config: &crate::config::Config,
    ) -> Result<Option<Vec<tinymcp_bus::ConnStatus>>, McpStatusFailure> {
        self.0.clone()
    }
}

fn server(name: &str) -> McpServerConfig {
    McpServerConfig {
        server: tinymcp_bus::McpServerConfig {
            name: name.to_owned(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn status(server_id: &str, status: tinymcp_bus::ServerStatus) -> tinymcp_bus::ConnStatus {
    tinymcp_bus::ConnStatus {
        server_id: server_id.to_owned(),
        qualified_name: "example/server".to_owned(),
        display_name: "fixture".to_owned(),
        status,
        tool_count: 0,
        last_error: Some("private endpoint response and token=secret".to_owned()),
        auth_hint: None,
    }
}

#[tokio::test]
async fn a_failed_existing_server_is_an_error_without_its_raw_error_text() {
    let check = McpServerCheck::with_probe(
        &server("server-a"),
        Arc::new(FixedStatuses(Ok(Some(vec![status(
            "server-a",
            tinymcp_bus::ServerStatus::Error,
        )])))),
    );

    let finding = check
        .detect(&DoctorCtx::from_config(crate::config::Config::default()))
        .await
        .remove(0);

    assert_eq!(finding.check_id, "mcp/server-a");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding.message.contains("status: error"));
    assert!(!finding.message.contains("private endpoint"));
    assert!(!finding.message.contains("secret"));
}

#[tokio::test]
async fn an_absent_existing_client_is_skipped_without_bootstrapping_one() {
    let check = McpServerCheck::with_probe(&server("server-a"), Arc::new(FixedStatuses(Ok(None))));

    let finding = check
        .detect(&DoctorCtx::from_config(crate::config::Config::default()))
        .await
        .remove(0);

    assert_eq!(finding.severity, FindingSeverity::Skipped);
    assert!(finding.message.contains("already open"));
}

#[tokio::test]
async fn an_existing_runtime_status_failure_is_reported_as_an_error() {
    let check = McpServerCheck::with_probe(
        &server("server-a"),
        Arc::new(FixedStatuses(Err(McpStatusFailure::Failed))),
    );

    let finding = check
        .detect(&DoctorCtx::from_config(crate::config::Config::default()))
        .await
        .remove(0);

    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding.message.contains("did not return server status"));
}

#[tokio::test]
async fn disabled_mcp_component_skips_each_configured_server() {
    let workspace = tempfile::tempdir().expect("create isolated config fixture");
    let mut config = crate::config::Config::default();
    config.workspace_dir = workspace.path().join("workspace");
    config.action_dir = workspace.path().join("actions");
    config.config_path = workspace.path().join("config.toml");
    config.mcp_client.servers.push(server("server-a"));
    let mut components = crate::core::runtime::ComponentSet::all();
    components.mcp = false;
    let report = crate::doctor::runner::run(
        &DoctorCtx::from_config(config).with_components(components),
        DoctorOptions {
            only: vec!["mcp/server-a".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("disabled MCP component is represented as skipped");

    assert_eq!(report.checks_skipped, vec!["mcp/server-a".to_owned()]);
    assert!(report.findings.iter().any(|finding| {
        finding.check_id == "mcp/server-a" && finding.severity == FindingSeverity::Skipped
    }));
}

#[test]
fn arbitrary_server_names_produce_valid_noncolliding_ids() {
    let first = McpServerCheck::configured(&server("Server A/α"));
    let second = McpServerCheck::configured(&server("server a/α"));

    assert_eq!(first.id(), "mcp/_53erver_20_41_2f_ce_b1");
    assert_ne!(first.id(), second.id());
}
