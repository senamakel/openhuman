use super::*;
use crate::hosted::test_support::store_session;
use crate::session::test_support::{EXPIRED_JWT, LIVE_JWT};
use openhuman_embed::__host::config::Config;
use openhuman_embed::__host::core::runtime::CoreContext;
use openhuman_embed::__host::core::slots::Extensions;
use openhuman_embed::__host::doctor::{registered_checks, DoctorCheck};
use openhuman_embed::{CoreBuilder, DomainSet, HostKind, ServiceSet};
use reqwest::Method;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn context(tmp: &TempDir, api_url: &str) -> DoctorCtx {
    DoctorCtx::from_config(Config {
        workspace_dir: tmp.path().join("workspace"),
        action_dir: tmp.path().join("workspace"),
        config_path: tmp.path().join("config.toml"),
        api_url: Some(api_url.to_owned()),
        ..Config::default()
    })
}

fn check(source: &HostedDoctorCheckSource, id: &str) -> Arc<dyn DoctorCheck> {
    source
        .checks()
        .into_iter()
        .find(|check| check.id() == id)
        .unwrap_or_else(|| panic!("missing hosted doctor check {id}"))
}

async fn findings(source: &HostedDoctorCheckSource, ctx: &DoctorCtx) -> Vec<Finding> {
    let checks = source.checks();
    let mut findings = Vec::with_capacity(checks.len());
    for check in checks {
        findings.extend(check.detect(ctx).await);
    }
    findings
}

#[tokio::test]
async fn healthy_backend_passes_hosted_checks_and_uses_sdk_attribution() {
    let _registry = crate::registry_test_lock_async().await;
    let server = MockServer::start().await;
    for (route, body) in [
        ("/", json!({"ok": true})),
        ("/auth/me", json!({"success": true, "data": {"user": {"id": "private-user"}}})),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .and(header("x-sdk-name", "openhuman"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/openai/v1/models"))
        .and(query_param("catalog", "openrouter"))
        .and(header("x-sdk-name", "openhuman"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": []})))
        .expect(1)
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let ctx = context(&tmp, &server.uri());
    store_session(ctx.config(), &LIVE_JWT);
    let source = HostedDoctorCheckSource::with_socket_state_reader(|| {
        Some(ConnectionStatus::Connected)
    });

    let results = findings(&source, &ctx).await;
    assert_eq!(results.len(), 6);
    assert!(results.iter().all(|finding| finding.severity == FindingSeverity::Pass));
    assert!(results
        .iter()
        .all(|finding| !finding.message.contains("private-user")));
}

#[tokio::test]
async fn backend_outage_skips_authenticated_hosted_probes_without_leaking_response_data() {
    let _registry = crate::registry_test_lock_async().await;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(503).set_body_string("token=private-token user=private-user"),
        )
        .expect(3)
        .mount(&server)
        .await;

    let tmp = TempDir::new().unwrap();
    let ctx = context(&tmp, &server.uri());
    store_session(ctx.config(), &LIVE_JWT);
    let source = HostedDoctorCheckSource::with_socket_state_reader(|| {
        Some(ConnectionStatus::Connected)
    });

    let reachable = check(&source, "backend/reachable").detect(&ctx).await;
    let identity = check(&source, "backend/identity").detect(&ctx).await;
    let catalog = check(&source, "inference/managed-catalog").detect(&ctx).await;

    assert_eq!(reachable[0].severity, FindingSeverity::Error);
    assert_eq!(identity[0].severity, FindingSeverity::Skipped);
    assert_eq!(catalog[0].severity, FindingSeverity::Skipped);
    for finding in reachable.iter().chain(&identity).chain(&catalog) {
        assert!(!finding.message.contains("private-token"));
        assert!(!finding.message.contains("private-user"));
    }
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.iter().all(|request| request.url.path() == "/"));
}

#[tokio::test]
async fn locally_expired_session_is_reported_as_session_expired() {
    let _registry = crate::registry_test_lock_async().await;
    let tmp = TempDir::new().unwrap();
    let ctx = context(&tmp, "http://127.0.0.1:9");
    store_session(ctx.config(), &EXPIRED_JWT);
    let source = HostedDoctorCheckSource::default();

    let finding = check(&source, "backend/credential").detect(&ctx).await;

    assert_eq!(finding[0].severity, FindingSeverity::Error);
    assert!(finding[0].message.contains("SESSION_EXPIRED"));
    assert!(!finding[0].message.contains(&*EXPIRED_JWT));
    assert!(!finding[0].message.contains("user-123"));
}

#[tokio::test]
async fn hosted_checks_are_scoped_to_the_runtime_that_installed_tinyhumans() {
    let _registry = crate::registry_test_lock_async().await;
    let mut extensions = Extensions::new();
    register_check_source_in(&extensions, HostedDoctorCheckSource::default());
    let config = Config::default();
    let hosted = CoreBuilder::new(HostKind::Library)
        .services(ServiceSet::none())
        .domains(DomainSet::none())
        .config(config.clone())
        .extensions(extensions)
        .build()
        .await
        .expect("hosted context");
    let core_only = CoreBuilder::new(HostKind::Library)
        .services(ServiceSet::none())
        .domains(DomainSet::none())
        .config(config)
        .build()
        .await
        .expect("plain core context");

    let hosted_ids = CoreContext::scope(hosted.context().clone(), async {
        registered_checks()
            .expect("registered hosted checks")
            .iter()
            .map(|check| check.id().to_owned())
            .collect::<Vec<_>>()
    })
    .await;
    let core_ids = CoreContext::scope(core_only.context().clone(), async {
        registered_checks()
            .expect("registered core checks")
            .iter()
            .map(|check| check.id().to_owned())
            .collect::<Vec<_>>()
    })
    .await;

    for id in [
        "backend/reachable",
        "backend/credential",
        "backend/identity",
        "backend/socket",
        "backend/api-url",
        "inference/managed-catalog",
    ] {
        assert!(hosted_ids.iter().any(|registered| registered == id), "{id}");
        assert!(!core_ids.iter().any(|registered| registered == id), "{id}");
    }
}
