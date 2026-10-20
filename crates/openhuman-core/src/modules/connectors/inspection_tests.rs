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
    delay: std::time::Duration,
}

struct FixtureReadyRuntime {
    proxy: tinybus::Proxy,
    proxy_requests: Arc<std::sync::atomic::AtomicUsize>,
}

impl ExistingRuntimeAccess for FixtureReadyRuntime {
    fn is_ready(&self) -> bool {
        true
    }

    fn proxy(&self) -> Result<tinybus::Proxy, ExistingCallError> {
        self.proxy_requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(self.proxy.clone())
    }
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
        tokio::time::sleep(self.delay).await;
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
    let mut selected = Config {
        config_path: scratch.path().join("config.toml"),
        workspace_dir: scratch.path().join("workspace-a"),
        ..Config::default()
    };
    selected.composio.mode = crate::config::schema::COMPOSIO_MODE_DIRECT.to_owned();
    selected.composio.api_key = Some("doctor-binding-key-a".to_owned());
    selected.composio.offered_toolkits_allow = vec!["github".to_owned()];
    seed_loaded_scope_for_test(&selected);
    let route = serde_json::json!({
        "route": "direct",
        "api_key": "doctor-binding-key-a",
        "entity_id": "default",
        "state_dir": selected.workspace_dir.join("state"),
        "offered_toolkits": super::super::offered_toolkits(&selected),
    });

    record_configured_route_for_test(&selected, &route);
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

    let mut reconfigured_b = selected.clone();
    reconfigured_b.workspace_dir = scratch.path().join("workspace-b");
    reconfigured_b.composio.api_key = Some("doctor-binding-key-b".to_owned());
    let route_b = serde_json::json!({
        "route": "direct",
        "api_key": "doctor-binding-key-b",
        "entity_id": "default",
        "state_dir": reconfigured_b.workspace_dir.join("state"),
        "offered_toolkits": super::super::offered_toolkits(&reconfigured_b),
    });
    record_configured_route_for_test(&reconfigured_b, &route_b);
    assert!(
        !binding_matches_config(&reconfigured_b),
        "reconfiguring a module loaded under A cannot relabel its state/policy as B"
    );

    *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

#[tokio::test]
async fn a_session_from_another_issuer_is_not_a_matching_connector_credential() {
    let scratch = tempfile::tempdir().expect("create isolated issuer fixture");
    let mut config = Config::default();
    config.config_path = scratch.path().join("config.toml");
    config.workspace_dir = scratch.path().join("workspace");
    config.secrets.encrypt = false;
    config.resolved_endpoints.control_plane_url = Some("https://account-b.example".to_owned());
    let profiles = scratch.path().join("auth-profiles.json");
    let raw = serde_json::json!({
        "schema_version": 1,
        "active_profiles": {},
        "profiles": {
            "app-session:default": {
                "provider": "app-session",
                "profile_name": "default",
                "kind": "token",
                "token": "issuer-mismatch-test-token",
                "metadata": { "session_issuing_backend": "https://account-a.example" }
            }
        }
    })
    .to_string();
    std::fs::write(&profiles, &raw).expect("seed selected issuer profile");

    assert!(selected_credential(&config).is_none());
    assert_eq!(std::fs::read_to_string(&profiles).unwrap(), raw);
    assert!(!scratch.path().join("auth-profiles.lock").exists());
}

#[tokio::test]
async fn load_scope_is_captured_only_when_the_module_receives_selected_paths_and_policy() {
    let _guard = route_lock().lock().await;
    let scratch = tempfile::tempdir().expect("create isolated load-scope fixture");
    let mut config = Config::default();
    config.config_path = scratch.path().join("config.toml");
    config.workspace_dir = scratch.path().join("workspace-a");
    config.composio.offered_toolkits_allow = vec!["github".to_owned()];
    let policy = super::super::offered_toolkits(&config);

    note_module_config_for_load(&config, &serde_json::json!({ "offered_toolkits": policy }));
    assert!(
        pending_load_scope().lock().unwrap().is_none(),
        "capability-only fallback does not attest a workspace"
    );

    let module_config = serde_json::json!({
        "route": "proxy",
        "state_dir": config.workspace_dir.join("state"),
        "offered_toolkits": super::super::offered_toolkits(&config),
    });
    note_module_config_for_load(&config, &module_config);
    assert_eq!(
        *pending_load_scope().lock().unwrap(),
        load_scope_fingerprint(&config)
    );
    *pending_load_scope().lock().unwrap() = None;
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
    seed_loaded_scope_for_test(&config);
    let accepted_route = serde_json::json!({
        "route": "direct",
        "api_key": "doctor-boundary-fixture-key",
        "entity_id": "default",
        "state_dir": config.workspace_dir.join("state"),
        "offered_toolkits": super::super::offered_toolkits(&config),
    });
    record_configured_route_for_test(&config, &accepted_route);

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
                delay: std::time::Duration::ZERO,
            },
        )
        .await
        .unwrap();
    let proxy = client.proxy(DESTINATION, OBJECT_PATH, INTERFACE).unwrap();
    let proxy_requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let runtime = FixtureReadyRuntime {
        proxy,
        proxy_requests: Arc::clone(&proxy_requests),
    };

    let request = serde_json::json!({ "mode": "backend" });
    let response = call_existing_readonly_with_runtime::<_, serde_json::Value, _>(
        &config,
        "ListConnectedIntegrations",
        request.clone(),
        &runtime,
    )
    .await
    .expect("read-only fixture responds")
    .expect("the exactly matching ready binding is probeable");

    assert_eq!(response, reply);
    let mut other_workspace = config.clone();
    other_workspace.workspace_dir = scratch.path().join("workspace-b");
    assert!(
        call_existing_readonly_with_runtime::<_, serde_json::Value, _>(
            &other_workspace,
            "ListConnectedIntegrations",
            request.clone(),
            &runtime,
        )
        .await
        .expect("mismatched binding is unavailable")
        .is_none()
    );
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ListConnectedIntegrations");
    assert_eq!(calls[0].1, serde_json::json!([request]));
    assert_eq!(proxy_requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    drop(calls);
    *configured_binding()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

#[tokio::test]
async fn existing_proxy_failures_are_sanitized_and_time_bounded() {
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
    service
        .serve_at(
            tinybus::ObjectPath::new(OBJECT_PATH).unwrap(),
            ReadOnlyFixture {
                calls,
                reply: serde_json::json!({ "private": "raw connector error fixture" }),
                delay: std::time::Duration::from_millis(40),
            },
        )
        .await
        .unwrap();
    let proxy = client.proxy(DESTINATION, OBJECT_PATH, INTERFACE).unwrap();

    let error = call_proxy_readonly_with_timeout::<_, String>(
        &proxy,
        "ListConnectedIntegrations",
        (),
        std::time::Duration::from_millis(1),
    )
    .await
    .expect_err("a slow connector status call must time out");

    assert_eq!(error, ExistingCallError::Failed);
    assert!(!format!("{error:?}").contains("raw connector error fixture"));

    let malformed = call_proxy_readonly_with_timeout::<_, String>(
        &proxy,
        "ListConnectedIntegrations",
        (),
        std::time::Duration::from_secs(1),
    )
    .await
    .expect_err("a malformed response must be surfaced as a safe failure");
    assert_eq!(malformed, ExistingCallError::Failed);
    assert!(!format!("{malformed:?}").contains("raw connector error fixture"));
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
