use super::*;
use crate::config::ComposioHostCredential;
use crate::core::runtime::{CoreContext, DomainSet, SaasConfig};
use crate::profiles::{ProfileHost, ProfileId, ProfileIdMode};

fn id(name: &str) -> ProfileId {
    ProfileId::for_user(name, ProfileIdMode::Raw).unwrap()
}

fn host(root: &std::path::Path, max_open: usize, idle_secs: u64) -> ProfileHost {
    let mut saas = SaasConfig::new(root);
    saas.max_profiles_open = max_open;
    saas.idle_evict_secs = idle_secs;
    ProfileHost::new(saas, CoreContext::for_test(DomainSet::full(), None))
}

// Real ProfileHost contexts supply ownership and layout. Only the fake service
// identity and cached response are fixtures; these tests make no backend calls.
fn seed(state: &crate::profiles::host::Profile, toolkit: &str) -> (Config, String) {
    let mut config = state.context().embedder_config().unwrap();
    config.composio.pin_host_credential(
        ComposioHostCredential::direct("ck_profile_lifecycle_fixture")
            .entity_id("profile-lifecycle-fixture"),
    );
    let key = CoreContext::sync_scope(Arc::clone(state.context()), || cache_key(&config));
    let epoch = cache_generation(&key);
    assert!(store_fetched_integrations(
        key.clone(),
        &epoch,
        epoch.load(Ordering::SeqCst),
        vec![ConnectedIntegration {
            toolkit: toolkit.into(),
            description: String::new(),
            tools: vec![],
            gated_tools: vec![],
            connected: true,
            connections: vec![],
            non_active_status: None,
        }],
    ));
    (config, key)
}

fn present(key: &str) -> bool {
    INTEGRATIONS_CACHE.read().unwrap().contains_key(key)
}

#[tokio::test]
async fn profile_release_reopens_with_a_cold_catalog_and_preserves_bob() {
    let _serial = composio_cache_test_lock_async().await;
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), 4, 3600);
    let (alice, bob) = (id("release-alice"), id("release-bob"));
    host.provision(&alice).await.unwrap();
    host.provision(&bob).await.unwrap();
    let a = host.open(&alice).await.unwrap();
    let b = host.open(&bob).await.unwrap();
    let (config, a_key) = seed(&a, "gmail");
    let (_, b_key) = seed(&b, "slack");
    drop(a);
    // Lifecycle operations run as the operator, not under Alice's task scope.
    assert!(host.release(&alice).await.unwrap());
    assert!(!present(&a_key), "retired profile snapshot must be removed");
    assert!(present(&b_key), "Alice's retirement must preserve Bob");
    let reopened = host.open(&alice).await.unwrap();
    CoreContext::sync_scope(Arc::clone(reopened.context()), || {
        assert_eq!(
            cache_key(&config),
            a_key,
            "same profile and credential identity"
        );
        assert!(
            cached_active_integrations(&config).is_none(),
            "reopening must not reuse an earlier holder's connections"
        );
    });
}

#[tokio::test]
async fn profile_idle_eviction_removes_only_the_retired_catalog() {
    let _serial = composio_cache_test_lock_async().await;
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), 4, 0);
    let (alice, bob) = (id("idle-alice"), id("idle-bob"));
    host.provision(&alice).await.unwrap();
    host.provision(&bob).await.unwrap();
    let a = host.open(&alice).await.unwrap();
    let b = host.open(&bob).await.unwrap();
    let (_, a_key) = seed(&a, "gmail");
    let (_, b_key) = seed(&b, "slack");
    drop(a);
    host.evict_idle().await;
    assert!(!host.is_open(&alice));
    assert!(host.is_open(&bob), "a held profile cannot be evicted");
    assert!(!present(&a_key));
    assert!(present(&b_key));
}

#[tokio::test]
async fn profile_budget_eviction_removes_only_the_retired_catalog() {
    let _serial = composio_cache_test_lock_async().await;
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), 2, 3600);
    let (alice, bob, carol) = (id("budget-alice"), id("budget-bob"), id("budget-carol"));
    for profile in [&alice, &bob, &carol] {
        host.provision(profile).await.unwrap();
    }
    let a = host.open(&alice).await.unwrap();
    let b = host.open(&bob).await.unwrap();
    let (_, a_key) = seed(&a, "gmail");
    let (_, b_key) = seed(&b, "slack");
    drop(a);
    let _c = host.open(&carol).await.unwrap();
    assert!(!host.is_open(&alice));
    assert!(host.is_open(&bob));
    assert!(!present(&a_key));
    assert!(present(&b_key));
}

#[tokio::test]
async fn profile_shutdown_removes_idle_catalogs_and_preserves_the_held_profile() {
    let _serial = composio_cache_test_lock_async().await;
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), 4, 3600);
    let (alice, bob) = (id("shutdown-alice"), id("shutdown-bob"));
    host.provision(&alice).await.unwrap();
    host.provision(&bob).await.unwrap();
    let a = host.open(&alice).await.unwrap();
    let b = host.open(&bob).await.unwrap();
    let (_, a_key) = seed(&a, "gmail");
    let (_, b_key) = seed(&b, "slack");
    drop(a);
    host.release_idle_on_shutdown().await;
    assert!(!host.is_open(&alice));
    assert!(host.is_open(&bob));
    assert!(!present(&a_key));
    assert!(present(&b_key));
}

#[tokio::test]
async fn profile_deprovision_removes_its_cached_connections() {
    let _serial = composio_cache_test_lock_async().await;
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), 4, 3600);
    let alice = id("deprovision-alice");
    host.provision(&alice).await.unwrap();
    let a = host.open(&alice).await.unwrap();
    let (_, a_key) = seed(&a, "gmail");
    drop(a);
    assert!(host.deprovision(&alice).await.unwrap());
    assert!(!host.is_open(&alice));
    assert!(!present(&a_key));
}

#[tokio::test]
async fn exact_profile_fence_rejects_an_already_started_warm_and_preserves_bob() {
    let _serial = composio_cache_test_lock_async().await;
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path(), 4, 3600);
    let (alice, bob) = (id("fence-alice"), id("fence-bob"));
    host.provision(&alice).await.unwrap();
    host.provision(&bob).await.unwrap();
    let a = host.open(&alice).await.unwrap();
    let b = host.open(&bob).await.unwrap();
    let (_, a_key) = seed(&a, "gmail");
    let (_, b_key) = seed(&b, "slack");
    let epoch = cache_generation(&a_key);
    let generation = epoch.load(Ordering::SeqCst);
    let grant = host
        .grants()
        .into_iter()
        .find(|(id, _)| id == &alice)
        .unwrap()
        .1;
    host.fence(&alice, &grant, "deterministic cache retirement fixture")
        .await;
    assert!(!host.is_open(&alice));
    assert!(a.is_fenced());
    assert!(!present(&a_key));
    assert!(present(&b_key));
    assert_ne!(epoch.load(Ordering::SeqCst), generation);
    assert!(
        !store_fetched_integrations(a_key.clone(), &epoch, generation, vec![]),
        "an already-started warm cannot restore a retired holder's snapshot"
    );
    assert!(!present(&a_key));
}
