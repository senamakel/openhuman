use super::*;
use crate::storage::test_support::memory_backend;
use tinyagents_harness::store::Store;

fn stores() -> TenantSessionStores {
    TenantSessionStores::new(memory_backend(), false).expect("bridge")
}

#[test]
fn an_unprofiled_key_routes_to_the_local_tenant() {
    let router = stores();
    let (provider, agent) = router.route("asha");
    assert_eq!(agent, "asha");
    let (same_provider, same_agent) = router.route("asha");
    assert!(Arc::ptr_eq(&provider, &same_provider));
    assert_eq!(same_agent, "asha");
    let (profile_provider, _) = router.route("u~asha");
    assert!(!Arc::ptr_eq(&provider, &profile_provider));
}

#[test]
fn a_profile_key_routes_to_the_profile_tenant() {
    let router = stores();
    let key = crate::core::runtime::session_key(&crate::core::runtime::Tenant {
        profile: Some("alice".into()),
        agent: Some("asha".into()),
    });
    let (provider, agent) = router.route(&key);
    assert_eq!(agent, "asha");
    let (same_provider, same_agent) = router.route(&key);
    assert!(Arc::ptr_eq(&provider, &same_provider));
    assert_eq!(same_agent, "asha");
}

#[test]
fn escaped_keys_round_trip_to_the_same_tenant_and_agent() {
    let router = stores();
    let key = crate::core::runtime::session_key(&crate::core::runtime::Tenant {
        profile: Some("a~b%c".into()),
        agent: None,
    });
    let (provider, agent) = router.route(&key);
    assert_eq!(agent, "default");
    let (same_provider, _) = router.route(&key);
    assert!(Arc::ptr_eq(&provider, &same_provider));
}

#[test]
fn one_provider_serves_every_agent_of_a_tenant() {
    let router = stores();
    let (first, _) = router.route("one");
    let (second, _) = router.route("two");
    assert!(Arc::ptr_eq(&first, &second));
    let (other, _) = router.route("u~two");
    assert!(!Arc::ptr_eq(&first, &other));
}

#[tokio::test]
async fn the_same_agent_key_is_isolated_between_operator_and_profile_scopes() {
    let router = stores();
    let operator = router.for_agent("asha");
    let profile_key = crate::core::runtime::session_key(&crate::core::runtime::Tenant {
        profile: Some("alice".into()),
        agent: Some("asha".into()),
    });
    let profile = router.for_agent(&profile_key);
    let escaped_key = crate::core::runtime::session_key(&crate::core::runtime::Tenant {
        profile: Some("a~b%c".into()),
        agent: Some("asha".into()),
    });
    let escaped_profile = router.for_agent(&escaped_key);

    operator
        .kv
        .put("scope-test", "owner", serde_json::json!("operator"))
        .await
        .unwrap();
    profile
        .kv
        .put("scope-test", "owner", serde_json::json!("alice"))
        .await
        .unwrap();
    escaped_profile
        .kv
        .put("scope-test", "owner", serde_json::json!("escaped"))
        .await
        .unwrap();

    assert_eq!(
        operator.kv.get("scope-test", "owner").await.unwrap(),
        Some(serde_json::json!("operator"))
    );
    assert_eq!(
        profile.kv.get("scope-test", "owner").await.unwrap(),
        Some(serde_json::json!("alice"))
    );
    assert_eq!(
        escaped_profile.kv.get("scope-test", "owner").await.unwrap(),
        Some(serde_json::json!("escaped"))
    );
}
