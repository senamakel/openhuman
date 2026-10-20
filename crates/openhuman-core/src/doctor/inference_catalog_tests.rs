use super::{
    DoctorOptions, FindingSeverity, ModelProbeOutcome,
    ops::{doctor_models, doctor_report_with_options},
};
use crate::config::Config;
use crate::config::schema::ModelRouteConfig;
use crate::config::schema::cloud_providers::{AuthStyle, CloudProviderCreds};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct CatalogServerState {
    status: StatusCode,
    body: serde_json::Value,
    requests: Arc<AtomicUsize>,
    authorizations: Arc<std::sync::Mutex<Vec<Option<String>>>>,
}

async fn catalog_handler(State(state): State<CatalogServerState>, headers: HeaderMap) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    state
        .authorizations
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .push(
            headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        );
    (state.status, Json(state.body)).into_response()
}

async fn spawn_catalog_server(
    status: StatusCode,
    body: serde_json::Value,
    has_models_route: bool,
) -> (String, CatalogServerState) {
    let state = CatalogServerState {
        status,
        body,
        requests: Arc::new(AtomicUsize::new(0)),
        authorizations: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind local catalog server");
    let address = listener.local_addr().expect("local server address");
    let app = Router::new().route("/v1/models", get(catalog_handler));
    let app = if has_models_route {
        app
    } else {
        app.fallback(get(catalog_handler))
    };
    tokio::spawn(async move {
        axum::serve(listener, app.with_state(state.clone()))
            .await
            .expect("serve local catalog");
    });
    (format!("http://{address}/v1"), state)
}

fn config_for_catalog(
    endpoint: String,
    provider_id: &str,
    slug: &str,
) -> (tempfile::TempDir, Config) {
    let root = tempfile::tempdir().expect("temporary doctor fixture");
    let mut config = Config {
        config_path: root.join("config.toml"),
        workspace_dir: root.join("workspace"),
        action_dir: root.join("workspace"),
        ..Config::default()
    };
    config.secrets.encrypt = false;
    config.cloud_providers.push(CloudProviderCreds {
        id: provider_id.to_owned(),
        slug: slug.to_owned(),
        label: "Doctor catalog fixture".to_owned(),
        endpoint,
        auth_style: AuthStyle::None,
        default_model: None,
    });
    (root, config)
}

fn catalog_body(ids: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "data": ids.iter().map(|id| serde_json::json!({ "id": id })).collect::<Vec<_>>()
    })
}

#[tokio::test]
async fn doctor_models_lists_configured_catalogs_and_honors_use_cache() {
    let (endpoint, server) =
        spawn_catalog_server(StatusCode::OK, catalog_body(&["present-model"]), true).await;
    let (_temp, config) = config_for_catalog(endpoint, "p_doctor_497_cache", "doctor-497-cache");

    let first = doctor_models(&config, true)
        .await
        .expect("a configured provider catalog should be inspectable");
    let second = doctor_models(&config, true)
        .await
        .expect("a cached catalog should remain available");
    assert_eq!(first.value.entries.len(), 1);
    assert_eq!(first.value.entries[0].provider, "doctor-497-cache");
    assert_eq!(first.value.entries[0].outcome, ModelProbeOutcome::Ok);
    assert_eq!(second.value.entries[0].outcome, ModelProbeOutcome::Ok);
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);

    doctor_models(&config, false)
        .await
        .expect("use_cache=false should refresh the provider catalog");
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn doctor_models_does_not_reuse_a_catalog_for_another_selected_config() {
    let (endpoint_a, server_a) =
        spawn_catalog_server(StatusCode::OK, catalog_body(&["account-a-model"]), true).await;
    let (endpoint_b, server_b) =
        spawn_catalog_server(StatusCode::OK, catalog_body(&["account-b-model"]), true).await;
    let (_temp_a, config_a) =
        config_for_catalog(endpoint_a, "p_doctor_497_selected", "doctor-497-selected");
    let (_temp_b, config_b) =
        config_for_catalog(endpoint_b, "p_doctor_497_selected", "doctor-497-selected");

    doctor_models(&config_a, true)
        .await
        .expect("first profile catalog should be available");
    doctor_models(&config_b, true)
        .await
        .expect("selected profile catalog should use its own endpoint");
    doctor_models(&config_b, true)
        .await
        .expect("the selected profile may reuse its own cached catalog");

    assert_eq!(server_a.requests.load(Ordering::SeqCst), 1);
    assert_eq!(server_b.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn doctor_models_skips_a_provider_without_a_models_route() {
    let (endpoint, server) = spawn_catalog_server(StatusCode::OK, catalog_body(&[]), false).await;
    let (_temp, config) =
        config_for_catalog(endpoint, "p_doctor_497_no_models", "doctor-497-no-models");

    let report = doctor_models(&config, false)
        .await
        .expect("a provider without a model-listing route is an ordinary skipped check");

    assert_eq!(report.value.summary.skipped, 1);
    assert_eq!(report.value.summary.errors, 0);
    assert_eq!(report.value.entries[0].outcome, ModelProbeOutcome::Skipped);
    assert!(
        report.value.entries[0]
            .message
            .as_deref()
            .is_some_and(|message| message.to_ascii_lowercase().contains("models"))
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn doctor_models_classifies_provider_401_as_auth_or_access() {
    let (endpoint, server) = spawn_catalog_server(
        StatusCode::UNAUTHORIZED,
        serde_json::json!({ "error": "credentials rejected" }),
        true,
    )
    .await;
    let (_temp, config) = config_for_catalog(
        endpoint,
        "p_doctor_497_unauthorized",
        "doctor-497-unauthorized",
    );

    let report = doctor_models(&config, false)
        .await
        .expect("provider authorization failures should be represented in the report");

    assert_eq!(report.value.summary.auth_or_access, 1);
    assert_eq!(report.value.summary.errors, 0);
    assert_eq!(
        report.value.entries[0].outcome,
        ModelProbeOutcome::AuthOrAccess
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn missing_default_and_route_models_are_warned_by_the_inference_check() {
    let (endpoint, _server) =
        spawn_catalog_server(StatusCode::OK, catalog_body(&["present-model"]), true).await;
    let (_temp, mut config) =
        config_for_catalog(endpoint, "p_doctor_497_missing", "doctor-497-missing");
    config.default_model = Some("doctor-497-missing:missing-default".into());
    config.model_routes.push(ModelRouteConfig {
        hint: "coding".into(),
        model: "doctor-497-missing:missing-route-model".into(),
    });

    let report = doctor_report_with_options(
        &config,
        DoctorOptions {
            only: vec!["inference/model-in-catalog".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("model catalog findings should be returned as doctor findings");

    let missing_default = report.value.findings.iter().find(|finding| {
        finding.check_id == "inference/model-in-catalog"
            && finding.severity == FindingSeverity::Warn
            && finding.message.contains("default_model")
            && finding.message.contains("missing-default")
    });
    assert!(missing_default.is_some(), "{:#?}", report.value.findings);
    assert!(report.value.findings.iter().any(|finding| {
        finding.check_id == "inference/model-in-catalog"
            && finding.severity == FindingSeverity::Warn
            && finding.message.contains("missing-route-model")
    }));
}

#[tokio::test]
async fn recorded_provider_auth_errors_are_reported_as_errors() {
    let provider = "doctor-497-auth-error-fixture";
    crate::inference::auth_error_registry::clear(provider);
    crate::inference::auth_error_registry::record(provider, 401);
    let report = doctor_report_with_options(
        &Config::default(),
        DoctorOptions {
            only: vec!["inference/auth-errors".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("the auth-error check should return a doctor finding");
    crate::inference::auth_error_registry::clear(provider);

    assert!(report.value.findings.iter().any(|finding| {
        finding.check_id == "inference/auth-errors"
            && finding.severity == FindingSeverity::Error
            && finding.message.contains(provider)
            && !finding.message.contains("credentials rejected")
    }));
}
