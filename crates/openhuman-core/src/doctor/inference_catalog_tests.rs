use super::{
    ops::{doctor_models, doctor_report_with_options},
    DoctorOptions, FindingSeverity, ModelProbeOutcome,
};
use crate::config::schema::cloud_providers::{AuthStyle, CloudProviderCreds};
use crate::config::schema::ModelRouteConfig;
use crate::config::Config;
use crate::security::credentials::{AuthService, DEFAULT_AUTH_PROFILE_NAME};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
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

async fn missing_catalog_handler(State(state): State<CatalogServerState>) -> Response {
    state.requests.fetch_add(1, Ordering::SeqCst);
    StatusCode::NOT_FOUND.into_response()
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
    let app = if has_models_route {
        Router::new().route("/v1/models", get(catalog_handler))
    } else {
        Router::new()
            .route("/v1/health", get(catalog_handler))
            .fallback(get(missing_catalog_handler))
    };
    let app = app.with_state(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve local catalog");
    });
    (format!("http://{address}/v1"), state)
}

fn temporary_config() -> (tempfile::TempDir, Config) {
    let root = tempfile::tempdir().expect("temporary doctor fixture");
    let mut config = Config {
        config_path: root.path().join("config.toml"),
        workspace_dir: root.path().join("workspace"),
        action_dir: root.path().join("workspace"),
        ..Config::default()
    };
    config.cloud_providers = Vec::new();
    (root, config)
}

fn config_for_catalog(
    endpoint: String,
    provider_id: &str,
    slug: &str,
) -> (tempfile::TempDir, Config) {
    let (root, mut config) = temporary_config();
    config.secrets.encrypt = false;
    config.cloud_providers = vec![CloudProviderCreds {
        id: provider_id.to_owned(),
        slug: slug.to_owned(),
        label: "Doctor catalog fixture".to_owned(),
        endpoint,
        auth_style: AuthStyle::None,
        default_model: None,
    }];
    (root, config)
}

fn catalog_body(ids: &[&str]) -> serde_json::Value {
    serde_json::json!({
        "data": ids.iter().map(|id| serde_json::json!({ "id": id })).collect::<Vec<_>>()
    })
}

fn store_provider_key(config: &Config, slug: &str, token: &str) {
    AuthService::from_config(config)
        .store_provider_token(
            &crate::inference::provider::factory::auth_key_for_slug(slug),
            DEFAULT_AUTH_PROFILE_NAME,
            token,
            std::collections::HashMap::new(),
            true,
        )
        .expect("seed scoped provider credential");
}

fn auth_profiles_path(config: &Config) -> std::path::PathBuf {
    config
        .config_path
        .parent()
        .expect("fixture config parent")
        .join("auth-profiles.json")
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
async fn doctor_models_does_not_reuse_a_catalog_for_another_profiles_credential() {
    let (endpoint, server) =
        spawn_catalog_server(StatusCode::OK, catalog_body(&["configured-model"]), true).await;
    let (_temp_a, mut config_a) = config_for_catalog(
        endpoint.clone(),
        "p_doctor_497_selected",
        "doctor-497-selected",
    );
    let (_temp_b, mut config_b) =
        config_for_catalog(endpoint, "p_doctor_497_selected", "doctor-497-selected");
    config_a.cloud_providers[0].auth_style = AuthStyle::Bearer;
    config_b.cloud_providers[0].auth_style = AuthStyle::Bearer;
    store_provider_key(&config_a, "doctor-497-selected", "fixture-token-profile-a");
    store_provider_key(&config_b, "doctor-497-selected", "fixture-token-profile-b");
    let auth_file_a_before = std::fs::read(auth_profiles_path(&config_a)).expect("profile A file");
    let auth_file_b_before = std::fs::read(auth_profiles_path(&config_b)).expect("profile B file");

    doctor_models(&config_a, true)
        .await
        .expect("first profile catalog should be available");
    doctor_models(&config_b, true)
        .await
        .expect("selected profile catalog should use its own credential");
    doctor_models(&config_b, true)
        .await
        .expect("the selected profile may reuse its own cached catalog");

    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        *server
            .authorizations
            .lock()
            .unwrap_or_else(|error| error.into_inner()),
        vec![
            Some("Bearer fixture-token-profile-a".to_owned()),
            Some("Bearer fixture-token-profile-b".to_owned())
        ]
    );
    assert_eq!(
        std::fs::read(auth_profiles_path(&config_a)).unwrap(),
        auth_file_a_before
    );
    assert_eq!(
        std::fs::read(auth_profiles_path(&config_b)).unwrap(),
        auth_file_b_before
    );
}

#[tokio::test]
async fn doctor_models_never_falls_back_to_an_ordinary_mutable_catalog() {
    use crate::core::runtime::{CoreContext, DomainSet};
    use crate::core::slots::Extensions;
    use crate::core::Outcome;
    use crate::inference::registry::{register_catalog_in, CatalogEntry, CatalogListing};

    let (endpoint, _server) =
        spawn_catalog_server(StatusCode::OK, catalog_body(&["must-not-be-used"]), true).await;
    let (_temp, config) = config_for_catalog(
        endpoint,
        "p_doctor_497_no_safe_hook",
        "doctor-497-no-safe-hook",
    );
    let normal_calls = Arc::new(AtomicUsize::new(0));
    let call_counter = Arc::clone(&normal_calls);
    let ordinary_listing: CatalogListing = Arc::new(move |_, _| {
        let call_counter = Arc::clone(&call_counter);
        Box::pin(async move {
            call_counter.fetch_add(1, Ordering::SeqCst);
            Ok(Outcome::new(
                serde_json::json!({ "models": [{ "id": "must-not-be-used" }] }),
                Vec::new(),
            ))
        })
    });
    let extensions = Extensions::new();
    register_catalog_in(
        &extensions,
        "doctor-497-no-safe-hook",
        CatalogEntry {
            list: ordinary_listing,
            limits: None,
        },
    );
    let context = CoreContext::for_test_with_extensions(DomainSet::full(), None, extensions);

    let result = CoreContext::scope(context, async { doctor_models(&config, false).await }).await;

    assert_eq!(normal_calls.load(Ordering::SeqCst), 0);
    match result {
        Ok(report) => assert_ne!(report.value.entries[0].outcome, ModelProbeOutcome::Ok),
        Err(_) => {}
    }
}

#[tokio::test]
async fn doctor_models_skips_a_provider_without_a_models_route() {
    let (endpoint, server) = spawn_catalog_server(StatusCode::OK, catalog_body(&[]), false).await;
    let (_temp, config) =
        config_for_catalog(endpoint, "p_doctor_497_no_models", "doctor-497-no-models");

    let endpoint_status = reqwest::get(format!("{}/models", config.cloud_providers[0].endpoint))
        .await
        .expect("mock provider should answer the missing catalog route");
    assert_eq!(endpoint_status.status(), StatusCode::NOT_FOUND);
    server.requests.store(0, Ordering::SeqCst);

    let report = doctor_models(&config, false)
        .await
        .expect("a provider without a model-listing route is an ordinary skipped check");

    assert_eq!(report.value.summary.skipped, 1);
    assert_eq!(report.value.summary.errors, 0);
    assert_eq!(report.value.entries[0].outcome, ModelProbeOutcome::Skipped);
    assert!(report.value.entries[0]
        .message
        .as_deref()
        .is_some_and(|message| message.to_ascii_lowercase().contains("models")));
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
            && finding.config_path.as_deref() == Some("default_model")
            && finding.message.contains("default_model")
    });
    assert!(missing_default.is_some(), "{:#?}", report.value.findings);
    let missing_route = report.value.findings.iter().find(|finding| {
        finding.check_id == "inference/model-in-catalog"
            && finding.severity == FindingSeverity::Warn
            && finding.config_path.as_deref() == Some("model_routes[0].model")
    });
    assert!(missing_route.is_some(), "{:#?}", report.value.findings);
    assert!(report.value.findings.iter().all(|finding| {
        !finding.message.contains("missing-default")
            && !finding.message.contains("missing-route-model")
    }));
}

#[tokio::test]
async fn recorded_provider_auth_errors_are_reported_as_errors() {
    let provider = "doctor-497-auth-error-fixture";
    crate::inference::auth_error_registry::clear(provider);
    crate::inference::auth_error_registry::record(provider, 401);
    let (_temp, config) = temporary_config();
    let report = doctor_report_with_options(
        &config,
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
