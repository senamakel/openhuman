use super::*;
use crate::doctor::contract::{DoctorCtx, FindingSeverity};
use crate::integrations::composio::connected_integrations::fetch::FetchConnectedIntegrationsStatus;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct CachedSnapshot {
    expected_workspace: std::path::PathBuf,
    expected_entity: String,
    expected_key: String,
    result: Option<FetchConnectedIntegrationsStatus>,
    reads: Arc<AtomicUsize>,
    network_calls: Arc<AtomicUsize>,
    observed_config: std::sync::Mutex<Option<(std::path::PathBuf, String, String)>>,
}

#[async_trait::async_trait]
impl ComposioSnapshotProbe for CachedSnapshot {
    async fn cached_status(
        &self,
        config: &crate::config::Config,
    ) -> Option<FetchConnectedIntegrationsStatus> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let entity = config
            .composio
            .host_credential
            .as_ref()
            .map(|credential| credential.entity().to_owned())
            .unwrap_or_default();
        let key = config
            .composio
            .host_credential
            .as_ref()
            .map(|credential| credential.api_key().to_owned())
            .unwrap_or_default();
        *self.observed_config.lock().unwrap() =
            Some((config.workspace_dir.clone(), entity.clone(), key.clone()));
        if config.workspace_dir != self.expected_workspace
            || entity != self.expected_entity
            || key != self.expected_key
        {
            return None;
        }
        self.result.clone()
    }

    async fn fetch_uncached(
        &self,
        _config: &crate::config::Config,
    ) -> Option<FetchConnectedIntegrationsStatus> {
        self.network_calls.fetch_add(1, Ordering::SeqCst);
        None
    }
}

fn config_in(root: &std::path::Path, entity: &str, key: &str) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config.action_dir = root.join("actions");
    config.config_path = root.join("config.toml");
    config.composio.pin_host_credential(
        crate::config::schema::ComposioHostCredential::direct(key).entity_id(entity),
    );
    config
}

#[tokio::test]
async fn an_already_cached_composio_snapshot_passes_without_fetching_or_leaking_details() {
    let scratch = tempfile::tempdir().expect("create isolated config fixture");
    let config = config_in(scratch.path(), "tenant-b", "doctor-fake-key-b");
    let reads = Arc::new(AtomicUsize::new(0));
    let network_calls = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(CachedSnapshot {
        expected_workspace: config.workspace_dir.clone(),
        expected_entity: "tenant-b".into(),
        expected_key: "doctor-fake-key-b".into(),
        result: Some(FetchConnectedIntegrationsStatus::Authoritative(vec![])),
        reads: Arc::clone(&reads),
        network_calls: Arc::clone(&network_calls),
        observed_config: Default::default(),
    });
    let check = ComposioCheck::with_probe(probe);

    let finding = check
        .detect(&DoctorCtx::from_config(config))
        .await
        .into_iter()
        .next()
        .expect("cached status is represented");

    assert_eq!(finding.check_id, "integrations/composio");
    assert_eq!(finding.severity, FindingSeverity::Pass);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(network_calls.load(Ordering::SeqCst), 0);
    assert!(!finding.message.contains("doctor-fake-key-b"));
    assert!(!finding.message.contains("tenant-b"));
}

#[tokio::test]
async fn a_cache_entry_for_ambient_a_is_never_reused_for_selected_b() {
    let ambient = tempfile::tempdir().expect("create ambient config fixture");
    let selected = tempfile::tempdir().expect("create selected config fixture");
    let config_a = config_in(ambient.path(), "tenant-a", "doctor-fake-key-a");
    let config_b = config_in(selected.path(), "tenant-b", "doctor-fake-key-b");
    let reads = Arc::new(AtomicUsize::new(0));
    let network_calls = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(CachedSnapshot {
        expected_workspace: config_a.workspace_dir.clone(),
        expected_entity: "tenant-a".into(),
        expected_key: "doctor-fake-key-a".into(),
        result: Some(FetchConnectedIntegrationsStatus::Authoritative(vec![])),
        reads: Arc::clone(&reads),
        network_calls: Arc::clone(&network_calls),
        observed_config: Default::default(),
    });
    let check = ComposioCheck::with_probe(probe.clone());
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
            .expect("a missing selected-B snapshot is represented")
    })
    .await;

    assert_eq!(finding.check_id, "integrations/composio");
    assert_eq!(finding.severity, FindingSeverity::Skipped);
    assert!(finding.message.contains("selected configuration"));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(network_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        probe.observed_config.lock().unwrap().as_ref().unwrap(),
        &(config_b.workspace_dir, "tenant-b".into())
    );
}

#[tokio::test]
async fn a_same_workspace_cache_entry_with_a_different_credential_is_not_reused() {
    let scratch = tempfile::tempdir().expect("create isolated config fixture");
    let config_a = config_in(scratch.path(), "tenant", "doctor-fake-key-a");
    let config_b = config_in(scratch.path(), "tenant", "doctor-fake-key-b");
    let reads = Arc::new(AtomicUsize::new(0));
    let network_calls = Arc::new(AtomicUsize::new(0));
    let probe = Arc::new(CachedSnapshot {
        expected_workspace: config_a.workspace_dir.clone(),
        expected_entity: "tenant".into(),
        expected_key: "doctor-fake-key-a".into(),
        result: Some(FetchConnectedIntegrationsStatus::Authoritative(vec![])),
        reads: Arc::clone(&reads),
        network_calls: Arc::clone(&network_calls),
        observed_config: Default::default(),
    });
    let check = ComposioCheck::with_probe(probe);

    let finding = check
        .detect(&DoctorCtx::from_config(config_b))
        .await
        .into_iter()
        .next()
        .expect("credential-mismatched cache is represented as unavailable");

    assert_eq!(finding.check_id, "integrations/composio");
    assert_eq!(finding.severity, FindingSeverity::Skipped);
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert_eq!(network_calls.load(Ordering::SeqCst), 0);
}
