use super::*;
use crate::config::{ProviderRoute, SearchProviderSettings};
use crate::doctor::contract::{DoctorCtx, FindingSeverity};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct SelectedConfigResolver(Arc<AtomicUsize>);

impl SearchProviderResolver for SelectedConfigResolver {
    fn resolve(
        &self,
        config: &crate::config::Config,
    ) -> Vec<crate::search::providers::ResolvedProvider> {
        self.0.fetch_add(1, Ordering::SeqCst);
        // This path reads only the selected config and deliberately supplies
        // no managed credential. It never loads, refreshes, or prompts for one.
        crate::search::providers::resolve_with(config, false)
    }
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
        Arc::new(SelectedConfigResolver(Arc::clone(&resolver_calls))),
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
        Arc::new(SelectedConfigResolver(Arc::clone(&resolver_calls))),
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
