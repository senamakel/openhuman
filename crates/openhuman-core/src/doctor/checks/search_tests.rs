use super::*;
use crate::config::{
    PresentationMode, ProviderRoute, SearchEngineCredentials, SearchProviderSettings,
};
use crate::doctor::contract::{DoctorCtx, FindingSeverity};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct ReadOnlyManagedResolver(Arc<AtomicUsize>);

impl SearchProviderResolver for ReadOnlyManagedResolver {
    fn resolve(
        &self,
        config: &crate::config::Config,
    ) -> Vec<crate::search::providers::ResolvedProvider> {
        self.0.fetch_add(1, Ordering::SeqCst);
        if config.profile_providers.is_some() {
            // Preserve direct routes and credentials pinned by this selected
            // SaaS profile; the binding is already an immutable config input.
            return crate::search::providers::resolve(config);
        }
        crate::search::providers::resolve_with(
            config,
            read_only_managed_credential_available(config),
        )
    }
}

fn read_only_managed_credential_available(config: &crate::config::Config) -> bool {
    use crate::security::credentials::{
        AuthService, api_key,
        session_support::{
            APP_SESSION_PROVIDER, SessionTokenCheck, check_session_backend, classify_session_token,
            is_local_session_token,
        },
    };

    if api_key::get_api_key_read_only(config)
        .ok()
        .flatten()
        .is_some_and(|key| !key.trim().is_empty())
    {
        return true;
    }

    let profile = AuthService::get_profile_read_only(config, APP_SESSION_PROVIDER, None)
        .ok()
        .flatten();
    if check_session_backend(config, profile.as_ref()).is_err() {
        return false;
    }
    matches!(
        classify_session_token(profile.as_ref(), chrono::Utc::now()),
        SessionTokenCheck::Live(token) if !is_local_session_token(&token)
    )
}

fn config_in(root: &std::path::Path) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config.action_dir = root.join("actions");
    config.config_path = root.join("config.toml");
    config.search.providers.insert(
        "brave".into(),
        SearchProviderSettings {
            enabled: true,
            route: ProviderRoute::Direct,
        },
    );
    config
}

fn backend_config_in(root: &std::path::Path) -> crate::config::Config {
    let mut config = config_in(root);
    config.resolved_endpoints.control_plane_url = Some("https://backend-b.example".into());
    config.secrets.encrypt = false;
    config
}

fn write_session_profile(root: &std::path::Path, expires_at: &str, issuer: &str) {
    let now = chrono::Utc::now().to_rfc3339();
    std::fs::write(
        root.join("auth-profiles.json"),
        serde_json::json!({
            "schema_version": 1,
            "updated_at": now,
            "active_profiles": {"app-session": "app-session:default"},
            "profiles": {
                "app-session:default": {
                    "provider": "app-session",
                    "profile_name": "default",
                    "kind": "token",
                    "token": "doctor-fake-session-token",
                    "metadata": {
                        "session_issuing_backend": issuer,
                        "session_expires_at": expires_at
                    }
                }
            }
        })
        .to_string(),
    )
    .expect("write fake session profile");
}

fn write_api_key_profile(root: &std::path::Path) {
    let now = chrono::Utc::now().to_rfc3339();
    std::fs::write(
        root.join("auth-profiles.json"),
        serde_json::json!({
            "schema_version": 1,
            "updated_at": now,
            "active_profiles": {"api-key": "api-key:default"},
            "profiles": {
                "api-key:default": {
                    "provider": "api-key",
                    "profile_name": "default",
                    "kind": "token",
                    "token": "doctor-fake-managed-api-key",
                    "metadata": {"kind": "tinyhumans-api-key"}
                }
            }
        })
        .to_string(),
    )
    .expect("write fake API key profile");
}

fn status_for(
    resolver: &ReadOnlyManagedResolver,
    config: &crate::config::Config,
    provider: &str,
) -> crate::search::providers::ProviderStatus {
    resolver
        .resolve(config)
        .into_iter()
        .find(|resolved| resolved.id == provider)
        .expect("provider is in the search catalog")
        .status(crate::search::is_active(config))
}

#[tokio::test]
async fn needs_key_is_a_warning_with_the_provider_config_key_in_its_hint() {
    let ambient = tempfile::tempdir().expect("create ambient config fixture");
    let selected = tempfile::tempdir().expect("create selected config fixture");
    let mut config_a = config_in(ambient.path());
    config_a.search.brave.api_key = Some("doctor-fake-key-a".into());
    let config_b = config_in(selected.path());
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let check = SearchProviderCheck::with_resolver(
        "brave",
        Arc::new(ReadOnlyManagedResolver(Arc::clone(&resolver_calls))),
    );
    let ambient_context = crate::core::runtime::CoreContext::for_test_with_config(
        crate::core::runtime::DomainSet::full(),
        config_a,
    );
    let finding = crate::core::runtime::CoreContext::scope(ambient_context, async {
        check
            .detect(&DoctorCtx::from_config(config_b))
            .await
            .into_iter()
            .next()
            .expect("NeedsKey produces a finding")
    })
    .await;

    assert_eq!(finding.check_id, "search/brave");
    assert_eq!(finding.severity, FindingSeverity::Warn);
    assert!(finding.fix_hint.contains("search.brave.api_key"));
    assert!(!finding.message.contains("doctor-fake-key-a"));
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_selected_direct_key_makes_the_provider_ready_without_any_network_call() {
    let scratch = tempfile::tempdir().expect("create isolated config fixture");
    let mut config = config_in(scratch.path());
    config.search.brave.api_key = Some("doctor-fake-key-b".into());
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let check = SearchProviderCheck::with_resolver(
        "brave",
        Arc::new(ReadOnlyManagedResolver(Arc::clone(&resolver_calls))),
    );

    let finding = check
        .detect(&DoctorCtx::from_config(config))
        .await
        .into_iter()
        .next()
        .expect("ready provider produces a finding");

    assert_eq!(finding.check_id, "search/brave");
    assert_eq!(finding.severity, FindingSeverity::Pass);
    assert!(!finding.message.contains("doctor-fake-key-b"));
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_live_selected_b_session_enables_managed_search_without_refreshing_it() {
    let ambient = tempfile::tempdir().expect("create ambient credential fixture");
    let selected = tempfile::tempdir().expect("create selected credential fixture");
    let now = chrono::Utc::now();
    write_session_profile(
        ambient.path(),
        &(now - chrono::Duration::hours(1)).to_rfc3339(),
        "https://backend-a.example",
    );
    write_session_profile(
        selected.path(),
        &(now + chrono::Duration::hours(1)).to_rfc3339(),
        "https://backend-b.example",
    );
    let config_a = backend_config_in(ambient.path());
    let mut config_b = backend_config_in(selected.path());
    config_b
        .search
        .providers
        .insert("exa".into(), SearchProviderSettings::managed());
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let resolver = Arc::new(ReadOnlyManagedResolver(Arc::clone(&resolver_calls)));
    let check = SearchProviderCheck::with_resolver("exa", resolver.clone());
    let ambient_context = crate::core::runtime::CoreContext::for_test_with_config(
        crate::core::runtime::DomainSet::full(),
        config_a,
    );

    let finding = crate::core::runtime::CoreContext::scope(ambient_context, async {
        check
            .detect(&DoctorCtx::from_config(config_b.clone()))
            .await
            .into_iter()
            .next()
            .expect("live selected-B managed provider produces a finding")
    })
    .await;

    assert_eq!(finding.check_id, "search/exa");
    assert_eq!(finding.severity, FindingSeverity::Pass);
    assert!(!finding.message.contains("doctor-fake-session-token"));
    assert_eq!(
        status_for(&resolver, &config_b, "exa"),
        crate::search::providers::ProviderStatus::Ready
    );
    assert_eq!(resolver_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_expired_or_wrong_backend_session_does_not_make_managed_search_ready() {
    let scratch = tempfile::tempdir().expect("create isolated credential fixture");
    let now = chrono::Utc::now();
    let mut config = backend_config_in(scratch.path());
    config
        .search
        .providers
        .insert("exa".into(), SearchProviderSettings::managed());
    let resolver = ReadOnlyManagedResolver(Arc::new(AtomicUsize::new(0)));

    assert_eq!(
        status_for(&resolver, &config, "exa"),
        crate::search::providers::ProviderStatus::SignInRequired
    );

    write_session_profile(
        scratch.path(),
        &(now - chrono::Duration::hours(1)).to_rfc3339(),
        "https://backend-b.example",
    );
    assert_eq!(
        status_for(&resolver, &config, "exa"),
        crate::search::providers::ProviderStatus::SignInRequired
    );
    let check = SearchProviderCheck::with_resolver(
        "exa",
        Arc::new(ReadOnlyManagedResolver(Arc::new(AtomicUsize::new(0)))),
    );
    let finding = check
        .detect(&DoctorCtx::from_config(config.clone()))
        .await
        .into_iter()
        .next()
        .expect("expired managed credential produces an advisory finding");
    assert_ne!(finding.severity, FindingSeverity::Pass);
    assert!(!finding.message.contains("doctor-fake-session-token"));
    assert!(!finding.fix_hint.trim().is_empty());

    write_session_profile(
        scratch.path(),
        &(now + chrono::Duration::hours(1)).to_rfc3339(),
        "https://backend-a.example",
    );
    assert_eq!(
        status_for(&resolver, &config, "exa"),
        crate::search::providers::ProviderStatus::SignInRequired
    );
}

#[tokio::test]
async fn a_selected_api_key_is_a_valid_managed_credential() {
    let scratch = tempfile::tempdir().expect("create isolated credential fixture");
    write_api_key_profile(scratch.path());
    let mut config = backend_config_in(scratch.path());
    config
        .search
        .providers
        .insert("exa".into(), SearchProviderSettings::managed());
    let resolver = ReadOnlyManagedResolver(Arc::new(AtomicUsize::new(0)));

    assert_eq!(
        status_for(&resolver, &config, "exa"),
        crate::search::providers::ProviderStatus::Ready
    );
}

#[tokio::test]
async fn profile_search_binding_is_resolved_without_operator_session_credentials() {
    let scratch = tempfile::tempdir().expect("create isolated profile fixture");
    let mut config = backend_config_in(scratch.path());
    config
        .search
        .providers
        .insert("exa".into(), SearchProviderSettings::managed());
    let mut saas = crate::core::runtime::SaasConfig::new(scratch.path());
    let mut search = crate::config::SearchConfig::default();
    search.presentation = PresentationMode::Roles;
    search.providers = std::collections::BTreeMap::from([
        ("exa".into(), SearchProviderSettings::direct()),
        ("gemini".into(), SearchProviderSettings::direct()),
    ]);
    search.roles = std::collections::BTreeMap::from([
        ("search".into(), vec!["exa".into(), "gemini".into()]),
        ("answer".into(), vec!["gemini".into()]),
        ("contents".into(), vec!["exa".into()]),
    ]);
    search.exa = SearchEngineCredentials {
        api_key: Some("doctor-profile-exa-key".into()),
        base_url: Some("http://127.0.0.1:1234".into()),
    };
    search.gemini = SearchEngineCredentials {
        api_key: Some("doctor-profile-gemini-key".into()),
        base_url: Some("http://127.0.0.1:2345".into()),
    };
    saas.search = Some(search);
    crate::profiles::provider_settings::ProfileProviderSettings::from_saas(&saas)
        .expect("valid direct profile binding")
        .bind(&mut config);
    let resolver = ReadOnlyManagedResolver(Arc::new(AtomicUsize::new(0)));

    assert_eq!(
        status_for(&resolver, &config, "exa"),
        crate::search::providers::ProviderStatus::Ready
    );
}
