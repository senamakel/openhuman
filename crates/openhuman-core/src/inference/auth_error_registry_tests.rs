use super::*;
use std::sync::Mutex;

// The registry is process-global, so these tests would otherwise race:
// one test's `reset_for_tests` can wipe another's entries mid-run. Serialize
// them on a shared lock (poison-tolerant) so each runs against a clean,
// exclusive registry.
static TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn record_returns_true_only_on_first_episode() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    assert!(record("openrouter", 401), "first record is a new episode");
    assert!(
        !record("openrouter", 401),
        "repeat record of same provider does not re-notify"
    );
    // A different provider is its own episode.
    assert!(record("openai", 403));
    reset_for_tests();
}

#[test]
fn clear_removes_entry_and_rearms_latch() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    assert!(record("openrouter", 401));
    assert!(clear("openrouter"), "entry was present");
    assert!(!clear("openrouter"), "second clear is a no-op");
    // After clear, the next failure is a new episode again.
    assert!(record("openrouter", 401), "latch re-armed after clear");
    reset_for_tests();
}

#[test]
fn snapshot_is_sorted_and_carries_actionable_message() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    record("openrouter", 401);
    record("anthropic", 401);
    let snap = snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].provider, "anthropic");
    assert_eq!(snap[1].provider, "openrouter");
    assert!(snap[1].message.contains("openrouter"));
    assert!(snap[1].message.contains("Connections → API keys → LLM"));
    reset_for_tests();
}

#[tokio::test]
async fn a_workspace_only_origin_is_not_assigned_to_a_different_config_root() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    let workspace = tempfile::tempdir().expect("shared workspace");
    let config_root_a = tempfile::tempdir().expect("config root A");
    let config_root_b = tempfile::tempdir().expect("config root B");
    let config_path_b = config_root_b.path().join("config.toml");
    let context = crate::core::runtime::CoreContext::for_test(
        crate::core::runtime::DomainSet::full(),
        Some(workspace.path().to_path_buf()),
    );

    crate::core::runtime::CoreContext::scope(context, async {
        assert!(crate::core::runtime::CoreContext::current_embedder_config().is_none());
        assert!(record("workspace-only-provider", 401));
    })
    .await;

    let (selected_errors, has_unattributed_error) =
        snapshot_for_scope(workspace.path(), &config_path_b, None);
    assert!(
        selected_errors.is_empty(),
        "a workspace-only observation cannot be assigned to config root B"
    );
    assert!(
        has_unattributed_error,
        "the existing process error remains visible as unattributed"
    );

    // Keep both roots live through the assertion; their distinct paths are
    // the ambiguity this regression protects.
    assert_ne!(config_root_a.path(), config_root_b.path());
    reset_for_tests();
}

#[tokio::test]
async fn a_normal_boot_binding_attributes_errors_to_its_resolved_config_root() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    let workspace = tempfile::tempdir().expect("shared workspace");
    let config_root_a = tempfile::tempdir().expect("resolved config root A");
    let config_root_b = tempfile::tempdir().expect("selected config root B");
    let config_path_a = config_root_a.path().join("config.toml");
    let config_path_b = config_root_b.path().join("config.toml");
    let context = crate::core::runtime::CoreContext::for_test_with_boot_config_path(
        crate::core::runtime::DomainSet::full(),
        workspace.path().to_path_buf(),
        config_path_a.clone(),
    );

    crate::core::runtime::CoreContext::scope(context, async {
        assert!(crate::core::runtime::CoreContext::current_embedder_config().is_none());
        assert!(record("normal-boot-provider", 401));
    })
    .await;

    let (errors_for_a, unattributed_for_a) =
        snapshot_for_scope(workspace.path(), &config_path_a, None);
    let (errors_for_b, unattributed_for_b) =
        snapshot_for_scope(workspace.path(), &config_path_b, None);
    assert_eq!(errors_for_a.len(), 1);
    assert!(!unattributed_for_a);
    assert!(errors_for_b.is_empty());
    assert!(!unattributed_for_b);
    reset_for_tests();
}

#[tokio::test]
async fn workspace_rebind_updates_or_clears_the_config_origin_hint_locally() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    reset_for_tests();
    let workspace = tempfile::tempdir().expect("shared workspace");
    let config_root_a = tempfile::tempdir().expect("config root A");
    let config_root_b = tempfile::tempdir().expect("config root B");
    let config_path_a = config_root_a.path().join("config.toml");
    let config_path_b = config_root_b.path().join("config.toml");
    let context_a = crate::core::runtime::CoreContext::for_test_with_boot_config_path(
        crate::core::runtime::DomainSet::full(),
        workspace.path().to_path_buf(),
        config_path_a.clone(),
    );
    let context_stays_a = crate::core::runtime::CoreContext::for_test_with_boot_config_path(
        crate::core::runtime::DomainSet::full(),
        workspace.path().to_path_buf(),
        config_path_a.clone(),
    );

    crate::core::runtime::CoreContext::scope(context_a.clone(), async {
        assert!(record("config-a-provider", 401));
    })
    .await;
    context_a
        .rebind_workspace(workspace.path(), Some(&config_path_b))
        .expect("rebind config origin");
    crate::core::runtime::CoreContext::scope(context_a.clone(), async {
        assert!(record("config-b-provider", 403));
    })
    .await;
    crate::core::runtime::CoreContext::scope(context_stays_a, async {
        assert!(record("still-config-a-provider", 401));
    })
    .await;

    let (errors_for_a, _) = snapshot_for_scope(workspace.path(), &config_path_a, None);
    let (errors_for_b, _) = snapshot_for_scope(workspace.path(), &config_path_b, None);
    assert_eq!(errors_for_a.len(), 2);
    assert_eq!(errors_for_b.len(), 1);

    context_a
        .rebind_workspace(workspace.path(), None)
        .expect("clear unknown config origin");
    crate::core::runtime::CoreContext::scope(context_a, async {
        assert!(record("unknown-config-provider", 401));
    })
    .await;
    let (errors_for_a, has_unattributed) =
        snapshot_for_scope(workspace.path(), &config_path_a, None);
    assert_eq!(errors_for_a.len(), 2);
    assert!(has_unattributed);
    reset_for_tests();
}
