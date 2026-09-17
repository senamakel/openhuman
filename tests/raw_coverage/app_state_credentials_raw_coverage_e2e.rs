use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use chrono::Utc;
use openhuman_core::config::rpc as config_rpc;
use openhuman_core::desktop::app_state::{
    snapshot, update_local_state, StoredAppStatePatch, StoredOnboardingTasks,
};
use openhuman_core::security::credentials::identity::peek_credential_user_identity;
use openhuman_core::security::credentials::ops::{clear_credential, set_credential, SetCredentialRequest};
use openhuman_core::security::credentials::session_support::CredentialKind;
use openhuman_core::security::credentials::profiles::{
    AuthProfile, AuthProfileKind, AuthProfilesStore, TokenSet,
};
use openhuman_core::security::credentials::{
    list_provider_credentials_by_prefix, AuthService, APP_SESSION_PROVIDER,
    DEFAULT_AUTH_PROFILE_NAME,
};
use serde_json::{json, Value};
use tempfile::{Builder, TempDir};

static ROUND14_ENV_LOCK: &OnceLock<Mutex<()>> = &crate::SHARED_ENV_LOCK;

struct EnvGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, old }
    }

    fn set_to_path(key: &'static str, path: &Path) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, path.as_os_str());
        Self { key, old }
    }

    fn unset(key: &'static str) -> Self {
        let old = std::env::var(key).ok();
        std::env::remove_var(key);
        Self { key, old }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.old {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct Harness {
    _tmp: TempDir,
    root: PathBuf,
    _guards: Vec<EnvGuard>,
}

impl Harness {
    async fn config(&self) -> openhuman_core::config::Config {
        config_rpc::load_config_with_timeout()
            .await
            .expect("isolated config should load")
    }

    fn state_file(&self) -> PathBuf {
        self.root.join("workspace/state/app-state.json")
    }
}

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ROUND14_ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tempdir() -> TempDir {
    std::fs::create_dir_all("target").expect("create target");
    Builder::new()
        .prefix("app-state-credentials-round14-")
        .tempdir_in("target")
        .expect("round14 tempdir")
}

fn write_min_config(root: &Path, api_url: &str) {
    std::fs::create_dir_all(root).expect("create config root");
    let cfg = format!(
        r#"api_url = "{api_url}"
default_model = "round14-coverage-model"
default_temperature = 0.2
onboarding_completed = true
chat_onboarding_completed = false

[observability]
analytics_enabled = true

[secrets]
encrypt = false

[meet]
auto_orchestrator_handoff = true

[local_ai]
enabled = false
runtime_enabled = false
opt_in_confirmed = false

[memory]
provider = "none"
embedding_provider = "none"
embedding_model = "none"
embedding_dimensions = 0
auto_save = false

[memory_tree]
embedding_strict = false
"#
    );
    std::fs::write(root.join("config.toml"), &cfg).expect("write config.toml");
    let _: openhuman_core::config::Config =
        toml::from_str(&cfg).expect("round14 config must match schema");
}

fn setup(api_url: &str) -> Harness {
    let tmp = tempdir();
    let root = tmp.path().join("openhuman");
    write_min_config(&root, api_url);
    let guards = vec![
        EnvGuard::set_to_path("OPENHUMAN_WORKSPACE", &root),
        EnvGuard::set_to_path("HOME", tmp.path()),
        EnvGuard::unset("BACKEND_URL"),
        EnvGuard::unset("VITE_BACKEND_URL"),
        EnvGuard::unset("OPENHUMAN_API_URL"),
        EnvGuard::unset("OPENHUMAN_CORE_RPC_URL"),
        EnvGuard::unset("OPENHUMAN_CORE_PORT"),
        EnvGuard::set("OPENHUMAN_KEYRING_BACKEND", "file"),
        EnvGuard::set("OPENHUMAN_MEMORY_EMBED_STRICT", "false"),
        EnvGuard::set("OPENHUMAN_MEMORY_EMBED_ENDPOINT", ""),
        EnvGuard::set("OPENHUMAN_MEMORY_EMBED_MODEL", ""),
    ];

    Harness {
        _tmp: tmp,
        root,
        _guards: guards,
    }
}

fn setup_default_paths(api_url: &str) -> Harness {
    let tmp = tempdir();
    let guards = vec![
        EnvGuard::set_to_path("HOME", tmp.path()),
        EnvGuard::unset("OPENHUMAN_WORKSPACE"),
        EnvGuard::unset("BACKEND_URL"),
        EnvGuard::unset("VITE_BACKEND_URL"),
        EnvGuard::unset("OPENHUMAN_API_URL"),
        EnvGuard::unset("OPENHUMAN_CORE_RPC_URL"),
        EnvGuard::unset("OPENHUMAN_CORE_PORT"),
        EnvGuard::set("OPENHUMAN_KEYRING_BACKEND", "file"),
        EnvGuard::set("OPENHUMAN_MEMORY_EMBED_STRICT", "false"),
        EnvGuard::set("OPENHUMAN_MEMORY_EMBED_ENDPOINT", ""),
        EnvGuard::set("OPENHUMAN_MEMORY_EMBED_MODEL", ""),
    ];
    let default_root = openhuman_core::config::default_root_openhuman_dir()
        .expect("default openhuman root");
    let root = openhuman_core::config::pre_login_user_dir(&default_root);
    write_min_config(&root, api_url);

    Harness {
        _tmp: tmp,
        root,
        _guards: guards,
    }
}

/// A backend that answers every request with 500. It must never be reached:
/// the core neither validates a handed-over credential nor refreshes the
/// current user, so any hit here is a regression.
async fn never_called_backend() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use axum::routing::any;
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    let app = axum::Router::new().route(
        "/{*path}",
        any(move || {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind never-called backend");
    let addr = listener.local_addr().expect("backend addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), hits)
}

/// A handed-over session is installed without validation, the snapshot reports
/// the handed-over user, and rich local state round-trips alongside it.
#[tokio::test]
async fn round14_snapshot_reports_the_handed_over_user_and_rich_local_state() {
    let _lock = env_lock();
    let (api_url, backend_hits) = never_called_backend().await;
    let harness = setup(&api_url);
    let config = harness.config().await;

    let installed = set_credential(
        &config,
        SetCredentialRequest {
            token: "round14.header.payload".to_string(),
            kind: Some(CredentialKind::Session.as_str().to_string()),
            user_id: Some("stored-user".to_string()),
            user: Some(json!({
                "id": "stored-user",
                "name": "Stored User",
                "email": "stored@example.test"
            })),
        },
    )
    .await
    .expect("install handed-over session")
    .value;
    assert!(installed.is_authenticated);
    assert_eq!(installed.credential.as_deref(), Some("session"));

    let updated = update_local_state(StoredAppStatePatch {
        keyring_consent: None,
        encryption_key: Some(Some("  round14-key  ".to_string())),
        onboarding_tasks: Some(Some(StoredOnboardingTasks {
            accessibility_permission_granted: true,
            local_model_consent_given: true,
            local_model_download_started: true,
            enabled_tools: vec!["gmail".to_string(), "calendar".to_string()],
            connected_sources: vec!["slack".to_string()],
            updated_at_ms: Some(123_456),
        })),
    })
    .await
    .expect("write local state")
    .value;
    assert_eq!(updated.encryption_key.as_deref(), Some("round14-key"));
    assert_eq!(
        updated
            .onboarding_tasks
            .as_ref()
            .expect("tasks")
            .enabled_tools,
        vec!["gmail", "calendar"]
    );

    let snap = snapshot().await.expect("snapshot").value;
    assert!(snap.auth.is_authenticated);
    assert_eq!(
        snap.session_token.as_deref(),
        Some("round14.header.payload")
    );
    assert_eq!(snap.auth.user_id.as_deref(), Some("stored-user"));
    assert_eq!(
        snap.current_user.as_ref().and_then(|v| v.get("id")),
        Some(&json!("stored-user"))
    );
    assert!(snap.onboarding_completed);
    assert!(snap.analytics_enabled);
    assert_eq!(
        snap.local_state.encryption_key.as_deref(),
        Some("round14-key")
    );
    let identity = peek_credential_user_identity().expect("identity seeded by set_credential");
    assert_eq!(identity.id.as_deref(), Some("stored-user"));
    assert_eq!(identity.name.as_deref(), Some("Stored User"));
    assert_eq!(identity.email.as_deref(), Some("stored@example.test"));
    assert_eq!(
        backend_hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "neither set_credential nor the snapshot may call the backend"
    );

    let raw = std::fs::read_to_string(harness.state_file()).expect("state file");
    let persisted: Value = serde_json::from_str(&raw).expect("valid state json");
    assert_eq!(persisted["encryptionKey"], "round14-key");
    assert_eq!(
        persisted["onboardingTasks"]["connectedSources"],
        json!(["slack"])
    );

    let cleared = clear_credential(&config, Some(CredentialKind::Session))
        .await
        .expect("clear session")
        .value;
    assert_eq!(cleared["removedSession"], json!(true));
    assert!(peek_credential_user_identity().is_none());
    let snap = snapshot().await.expect("snapshot after clear").value;
    assert!(!snap.auth.is_authenticated);
    assert!(snap.current_user.is_none());
}

/// A pending payload (`pendingBackendValidation`) is what a host stores when
/// the backend was unreachable at login; it carries no identity, and the
/// snapshot must not invent one. A second `set_credential` with the same token
/// and user — the host's later `/auth/me` answer — replaces it in place.
#[tokio::test]
async fn round14_pending_payload_is_reported_then_replaced_by_a_refresh() {
    let _lock = env_lock();
    let (api_url, backend_hits) = never_called_backend().await;
    let harness = setup(&api_url);
    let config = harness.config().await;

    set_credential(
        &config,
        SetCredentialRequest {
            token: "round14.pending".to_string(),
            kind: None,
            user_id: Some("pending-user".to_string()),
            user: Some(json!({ "pendingBackendValidation": true })),
        },
    )
    .await
    .expect("install pending session");
    let snap = snapshot().await.expect("snapshot pending").value;
    assert_eq!(snap.auth.user_id.as_deref(), Some("pending-user"));
    assert_eq!(
        snap.current_user
            .as_ref()
            .and_then(|v| v.get("pendingBackendValidation")),
        Some(&json!(true))
    );
    assert!(
        peek_credential_user_identity().is_none(),
        "a placeholder payload carries no identity"
    );

    let refreshed = set_credential(
        &config,
        SetCredentialRequest {
            token: "round14.pending".to_string(),
            kind: None,
            user_id: Some("pending-user".to_string()),
            user: Some(json!({ "id": "pending-user", "email": "confirmed@example.test" })),
        },
    )
    .await
    .expect("refresh pending session");
    assert!(refreshed.logs.iter().any(|l| l.contains("credential refreshed")));
    let snap = snapshot().await.expect("snapshot confirmed").value;
    assert_eq!(
        snap.current_user.as_ref().and_then(|v| v.get("email")),
        Some(&json!("confirmed@example.test"))
    );
    assert!(snap
        .current_user
        .as_ref()
        .and_then(|v| v.get("pendingBackendValidation"))
        .is_none());
    assert_eq!(backend_hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn round14_profiles_cover_oauth_token_selection_schema_and_quarantine_edges() {
    let _lock = env_lock();
    let harness = setup("http://127.0.0.1:9");
    let state_dir = harness.root.join("profile-store");
    let store = AuthProfilesStore::new(&state_dir, false);

    let token_profile = AuthProfile::new_token("channel:slack:bot", "default", "xoxb-token".into());
    store
        .upsert_profile(token_profile.clone(), true)
        .expect("insert token profile");

    let mut oauth = AuthProfile::new_oauth(
        "github",
        "work",
        TokenSet {
            access_token: "gh-access".into(),
            refresh_token: Some("gh-refresh".into()),
            id_token: Some("gh-id".into()),
            expires_at: Some(Utc::now() + chrono::Duration::minutes(30)),
            token_type: Some("Bearer".into()),
            scope: Some("repo user".into()),
        },
    );
    oauth.account_id = Some("acct-gh".into());
    oauth.workspace_id = Some("workspace-gh".into());
    oauth.metadata = BTreeMap::from([("team".to_string(), "core".to_string())]);
    store
        .upsert_profile(oauth.clone(), false)
        .expect("insert oauth");
    store
        .set_active_profile("github", &oauth.id)
        .expect("activate oauth");

    let data = store.load().expect("load profiles");
    let loaded_oauth = data.profiles.get(&oauth.id).expect("loaded oauth");
    assert_eq!(loaded_oauth.kind, AuthProfileKind::OAuth);
    assert_eq!(
        loaded_oauth
            .token_set
            .as_ref()
            .map(|tokens| tokens.access_token.as_str()),
        Some("gh-access")
    );
    assert_eq!(loaded_oauth.workspace_id.as_deref(), Some("workspace-gh"));
    assert_eq!(data.active_profiles.get("github"), Some(&oauth.id));

    let service = AuthService::new(&state_dir, false);
    assert_eq!(
        service
            .get_provider_bearer_token("github", None)
            .expect("active github token")
            .as_deref(),
        Some("gh-access")
    );
    assert_eq!(
        service
            .get_provider_bearer_token("channel:slack:bot", None)
            .expect("active channel token")
            .as_deref(),
        Some("xoxb-token")
    );
    let err = service
        .set_active_profile("github", &token_profile.id)
        .expect_err("wrong-provider activation should fail")
        .to_string();
    assert!(err.contains("belongs to provider"));

    let path = store.path().to_path_buf();
    let mut raw: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("profile json"))
            .expect("valid profile json");
    raw["schema_version"] = json!(0);
    raw["profiles"]["legacy-empty"] = json!({
        "provider": "legacy",
        "profile_name": "empty",
        "kind": "token",
        "token": "",
        "created_at": "not-a-date",
        "updated_at": "also-not-a-date",
        "metadata": { "note": "kept" }
    });
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&raw).expect("serialize"),
    )
    .expect("write schema-zero profile json");

    let migrated = store.load().expect("schema 0 should migrate in memory");
    assert_eq!(migrated.schema_version, 1);
    // Since #5432 the migration re-keys every profile to its canonical
    // `<provider>:<profile_name>` id, so the raw `legacy-empty` key is
    // rewritten to `legacy:empty` rather than kept verbatim.
    assert!(
        !migrated.profiles.contains_key("legacy-empty"),
        "a non-canonical profile id must be re-keyed by the migration"
    );
    let legacy = migrated
        .profiles
        .get("legacy:empty")
        .expect("legacy profile re-keyed to its canonical id");
    assert_eq!(legacy.id, "legacy:empty");
    assert_eq!(legacy.provider, "legacy");
    // The empty legacy token surfaces as `Some("")` from the file-backed store
    // and as `None` where the secret lives in a keychain (CI has none), so
    // accept either — what matters is that no non-empty token appears.
    assert!(
        legacy.token.as_deref().is_none_or(str::is_empty),
        "legacy token must be empty, got {:?}",
        legacy.token
    );

    raw["schema_version"] = json!(999);
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&raw).expect("serialize"),
    )
    .expect("write future schema");
    let err = store
        .load()
        .expect_err("future schema should fail")
        .to_string();
    assert!(err.contains("Unsupported auth profile schema version 999"));

    std::fs::write(&path, "{not-json").expect("write corrupt profile store");
    let empty = store.load().expect("corrupt profile store quarantined");
    assert!(empty.profiles.is_empty());
    assert!(
        !path.exists(),
        "corrupt auth-profiles.json should be renamed"
    );
    let quarantined = std::fs::read_dir(path.parent().expect("profile parent"))
        .expect("profile dir")
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"));
    assert!(quarantined, "corrupt profile store should leave artifact");
}

#[tokio::test]
async fn round14_credentials_prefix_listing_and_composio_direct_edges() {
    let _lock = env_lock();
    let harness = setup("http://127.0.0.1:9");
    let config = harness.config().await;

    let empty =
        openhuman_core::security::credentials::store_composio_api_key(&config, "   ").await;
    assert_eq!(
        empty.expect_err("empty composio key rejected"),
        "composio api_key must not be empty"
    );

    openhuman_core::security::credentials::store_composio_api_key(
        &config,
        "  composio-round14-key  ",
    )
    .await
    .expect("store composio key");
    assert_eq!(
        openhuman_core::security::credentials::get_composio_api_key(&config)
            .expect("get composio key")
            .as_deref(),
        Some("composio-round14-key")
    );

    let auth = AuthService::from_config(&config);
    auth.store_provider_token(
        "channel:telegram:managed_dm",
        "primary",
        "telegram-token",
        HashMap::from([("chat_id".to_string(), "123".to_string())]),
        true,
    )
    .expect("store telegram channel token");
    auth.store_provider_token(
        "channel:discord:bot",
        "primary",
        "discord-token",
        HashMap::new(),
        true,
    )
    .expect("store discord channel token");
    auth.store_provider_token("other", "primary", "other-token", HashMap::new(), true)
        .expect("store other token");

    let channels = list_provider_credentials_by_prefix(&config, "channel:")
        .await
        .expect("prefix list");
    let providers = channels
        .iter()
        .map(|profile| profile.provider.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        providers,
        vec!["channel:discord:bot", "channel:telegram:managed_dm"]
    );
    assert!(channels
        .iter()
        .any(|profile| profile.metadata_keys == vec!["chat_id"]));

    let cleared = openhuman_core::security::credentials::clear_composio_api_key(&config)
        .await
        .expect("clear composio key");
    assert_eq!(cleared.value["removed"], true);
    assert_eq!(
        openhuman_core::security::credentials::get_composio_api_key(&config)
            .expect("get cleared composio key"),
        None
    );
    let cleared_again = openhuman_core::security::credentials::clear_composio_api_key(&config)
        .await
        .expect("clear composio key idempotent");
    assert_eq!(cleared_again.value["removed"], false);
}

