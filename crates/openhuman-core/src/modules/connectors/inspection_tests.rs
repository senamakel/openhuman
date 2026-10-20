use super::*;
use std::sync::{Arc, Mutex};
use tinybus::service::Interface;
use tinybus::transport::memory::MemoryBus;

const DESTINATION: &str = "ai.tinyhumans.openhuman.DoctorConnectorFixture";
const OBJECT_PATH: &str = "/ai/tinyhumans/openhuman/DoctorConnectorFixture";
const INTERFACE: &str = "ai.tinyhumans.openhuman.DoctorConnectorFixture";

struct ReadOnlyFixture {
    calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    reply: serde_json::Value,
}

#[async_trait::async_trait]
impl Interface for ReadOnlyFixture {
    fn name(&self) -> tinybus::InterfaceName {
        tinybus::InterfaceName::new(INTERFACE).unwrap()
    }

    fn members(&self) -> Vec<tinybus::MemberName> {
        vec![tinybus::MemberName::new("ListConnectedIntegrations").unwrap()]
    }

    async fn call(
        &self,
        member: &tinybus::MemberName,
        args: serde_json::Value,
    ) -> tinybus::Result<serde_json::Value> {
        self.calls
            .lock()
            .unwrap()
            .push((member.as_str().to_owned(), args));
        Ok(self.reply.clone())
    }
}

#[tokio::test]
async fn a_matching_config_binding_is_pinned_to_workspace_policy_and_credential() {
    let _guard = route_lock().lock().await;
    let scratch = tempfile::tempdir().expect("create isolated binding fixture");
    let selected = Config {
        config_path: scratch.path().join("config.toml"),
        workspace_dir: scratch.path().join("workspace-a"),
        ..Config::default()
    };
    selected.composio.mode = crate::config::schema::COMPOSIO_MODE_DIRECT.to_owned();
    selected.composio.api_key = Some("doctor-binding-key-a".to_owned());
    selected.composio.offered_toolkits_allow = vec!["github".to_owned()];
    let route = serde_json::json!({
        "route": "direct",
        "api_key": "doctor-binding-key-a",
        "entity_id": "default",
        "state_dir": selected.workspace_dir.join("state"),
        "offered_toolkits": super::super::offered_toolkits(&selected),
    });

    record_configured_route(&selected, &route);
    assert!(binding_matches_config(&selected));

    let mut other_workspace = selected.clone();
    other_workspace.workspace_dir = scratch.path().join("workspace-b");
    assert!(!binding_matches_config(&other_workspace));

    let mut other_credential = selected.clone();
    other_credential.composio.api_key = Some("doctor-binding-key-b".to_owned());
    assert!(!binding_matches_config(&other_credential));

    let mut other_policy = selected.clone();
    other_policy
        .composio
        .offered_toolkits_allow
        .push("slack".to_owned());
    assert!(!binding_matches_config(&other_policy));

    *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

#[tokio::test]
async fn existing_readonly_call_returns_a_matching_ready_response_with_a_bound() {
    let _route_guard = route_lock().lock().await;
    let scratch = tempfile::tempdir().expect("create selected connector workspace");
    let mut config = Config::default();
    config.config_path = scratch.path().join("config.toml");
    config.workspace_dir = scratch.path().join("workspace");
    config.composio.mode = crate::config::schema::COMPOSIO_MODE_DIRECT.to_owned();
    config.composio.api_key = Some("doctor-boundary-fixture-key".to_owned());
    let accepted_route = serde_json::json!({
        "route": "direct",
        "api_key": "doctor-boundary-fixture-key",
        "entity_id": "default",
        "state_dir": config.workspace_dir.join("state"),
        "offered_toolkits": super::super::offered_toolkits(&config),
    });
    record_configured_route(&config, &accepted_route);

    let bus = MemoryBus::new();
    tinybus::broker::Broker::new().spawn(bus.clone());
    let service = tinybus::Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    let client = tinybus::Connection::connect(bus.connect().await.unwrap())
        .await
        .unwrap();
    service.request_name(DESTINATION).await.unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let reply = serde_json::json!({ "integrations": [{ "toolkit": "github", "connected": true }] });
    service
        .serve_at(
            tinybus::ObjectPath::new(OBJECT_PATH).unwrap(),
            ReadOnlyFixture {
                calls: Arc::clone(&calls),
                reply: reply.clone(),
            },
        )
        .await
        .unwrap();
    let proxy = client.proxy(DESTINATION, OBJECT_PATH, INTERFACE).unwrap();

    let request = serde_json::json!({ "mode": "backend" });
    let response = call_bound_proxy::<_, serde_json::Value>(
        &config,
        &proxy,
        "ListConnectedIntegrations",
        request.clone(),
    )
    .await
    .expect("read-only fixture responds")
    .expect("the exactly matching ready binding is probeable");

    assert_eq!(response, reply);
    let mut other_workspace = config.clone();
    other_workspace.workspace_dir = scratch.path().join("workspace-b");
    assert!(call_bound_proxy::<_, serde_json::Value>(
        &other_workspace,
        &proxy,
        "ListConnectedIntegrations",
        request,
    )
    .await
    .expect("mismatched binding is unavailable")
    .is_none());
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ListConnectedIntegrations");
    assert_eq!(calls[0].1, serde_json::json!([request]));
    drop(calls);
    *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

#[tokio::test]
async fn an_absent_connector_does_not_start_runtime_or_load_the_module() {
    if crate::util::module_test_process::run(
        module_path!(),
        "an_absent_connector_does_not_start_runtime_or_load_the_module",
    ) {
        return;
    }
    assert!(!crate::modules::runtime::is_started());
    let scratch = tempfile::tempdir().expect("create isolated missing connector config");
    let mut config = Config::default();
    config.config_path = scratch.path().join("config.toml");
    config.workspace_dir = scratch.path().join("workspace");
    config.composio.mode = crate::config::schema::COMPOSIO_MODE_BACKEND.to_owned();
    let before = crate::modules::ops::state_of(super::super::MODULE_ID);

    let result =
        call_existing_readonly::<_, serde_json::Value>(&config, "ListConnectedIntegrations", ())
            .await
            .expect("absence is an unavailable observation, not a call error");

    assert!(result.is_none());
    assert_eq!(
        crate::modules::ops::state_of(super::super::MODULE_ID),
        before
    );
    assert!(!crate::modules::runtime::is_started());
}
