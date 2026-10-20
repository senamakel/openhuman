use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use crate::config::Config;
use crate::doctor::profile::{
    DesktopProfileError, load_desktop_profile, resolve_desktop_profile_for_doctor,
};
use crate::doctor::{
    DoctorCheck, DoctorCheckSource, DoctorConfigSource, DoctorCtx, DoctorOptions, DoctorTier,
    Finding, FindingSeverity,
};
use crate::profiles::ProfileLayout;
use crate::security::credentials::{
    APP_SESSION_PROVIDER, AuthService, DEFAULT_AUTH_PROFILE_NAME,
    session_support::get_session_token,
};

fn config_for(layout: &ProfileLayout) -> Config {
    Config {
        config_path: layout.config_path.clone(),
        workspace_dir: layout.workspace_dir.clone(),
        action_dir: layout.sandbox_dir.clone(),
        ..Config::default()
    }
}

struct WorkspaceOverrideGuard(Option<OsString>);

impl WorkspaceOverrideGuard {
    fn set(path: &std::path::Path) -> Self {
        let previous = std::env::var_os("OPENHUMAN_WORKSPACE");
        std::env::set_var("OPENHUMAN_WORKSPACE", path);
        Self(previous)
    }

    fn clear() -> Self {
        let previous = std::env::var_os("OPENHUMAN_WORKSPACE");
        std::env::remove_var("OPENHUMAN_WORKSPACE");
        Self(previous)
    }
}

impl Drop for WorkspaceOverrideGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            std::env::set_var("OPENHUMAN_WORKSPACE", previous);
        } else {
            std::env::remove_var("OPENHUMAN_WORKSPACE");
        }
    }
}

struct SecretBackendOverrideGuard;

impl Drop for SecretBackendOverrideGuard {
    fn drop(&mut self) {
        crate::security::secret_storage::clear_secret_backend();
    }
}

#[tokio::test]
async fn explicit_profile_reads_its_own_session_without_switching_active_user() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    tokio::fs::create_dir_all(root).await.unwrap();
    let active_user_path = root.join("active_user.toml");
    tokio::fs::write(&active_user_path, "user_id = \"account-a\"\n")
        .await
        .unwrap();

    let account_a = ProfileLayout::new(root, "account-a");
    tokio::fs::create_dir_all(&account_a.dir).await.unwrap();
    tokio::fs::write(
        &account_a.config_path,
        "default_model = \"account-a-model\"\n",
    )
    .await
    .unwrap();
    AuthService::from_config(&config_for(&account_a))
        .store_provider_token(
            APP_SESSION_PROVIDER,
            DEFAULT_AUTH_PROFILE_NAME,
            "account-a-session",
            HashMap::new(),
            true,
        )
        .unwrap();

    let account_b = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&account_b.dir).await.unwrap();
    tokio::fs::write(
        &account_b.config_path,
        "default_model = \"account-b-model\"\n",
    )
    .await
    .unwrap();

    let active_user_before = tokio::fs::read(&active_user_path).await.unwrap();
    let _workspace_override = WorkspaceOverrideGuard::set(&account_a.workspace_dir);
    let selected = load_desktop_profile(root, "account-b").await.unwrap();

    assert_eq!(selected.config.config_path, account_b.config_path);
    assert_eq!(selected.config.workspace_dir, account_b.workspace_dir);
    assert_eq!(selected.layout.version, account_b.version);
    assert_eq!(
        selected.config.default_model.as_deref(),
        Some("account-b-model")
    );
    assert_eq!(get_session_token(&selected.config).unwrap(), None);
    assert_eq!(
        get_session_token(&config_for(&account_a))
            .unwrap()
            .as_deref(),
        Some("account-a-session")
    );
    assert_eq!(
        tokio::fs::read(&active_user_path).await.unwrap(),
        active_user_before
    );
}

#[tokio::test]
async fn inspecting_selected_profile_credentials_does_not_quarantine_corrupt_auth_store()
-> Result<(), String> {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    let temp = TempDir::new().unwrap();
    let layout = ProfileLayout::new(temp.path(), "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(&layout.config_path, "secrets.encrypt = false\n")
        .await
        .unwrap();
    let profiles_path = layout.dir.join("auth-profiles.json");
    let original = b"not valid auth profile json";
    tokio::fs::write(&profiles_path, original).await.unwrap();

    let selected = load_desktop_profile(temp.path(), "account-b")
        .await
        .unwrap();
    let result = crate::security::credentials::session_support::load_app_session_profile_read_only(
        &selected.config,
    );
    let after = tokio::fs::read(&profiles_path).await;
    if result.is_ok() {
        return Err("a corrupt credential store must be reported as unavailable".into());
    }
    let after = after.map_err(|_| "doctor removed the corrupt credential store".to_string())?;
    if after != original {
        return Err("doctor modified the corrupt credential store".into());
    }
    Ok(())
}

#[tokio::test]
async fn selected_profile_auth_snapshot_reads_its_backend_without_mutating_profile_files() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    #[cfg(target_os = "macos")]
    let temp = TempDir::new_in("/private/tmp").unwrap();
    #[cfg(not(target_os = "macos"))]
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let account_a = ProfileLayout::new(root, "account-a");
    let account_b = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&account_a.dir).await.unwrap();
    tokio::fs::create_dir_all(&account_b.dir).await.unwrap();
    tokio::fs::create_dir_all(&account_a.workspace_dir)
        .await
        .unwrap();
    tokio::fs::create_dir_all(&account_b.workspace_dir)
        .await
        .unwrap();
    tokio::fs::write(&account_b.config_path, "secrets.encrypt = true\n")
        .await
        .unwrap();
    let profiles_path = account_b.dir.join("auth-profiles.json");
    tokio::fs::write(
        &profiles_path,
        r#"{"schema_version":1,"active_profiles":{"app-session":"app-session:default"},"profiles":{"app-session:default":{"provider":"app-session","profile_name":"default","kind":"token","metadata":{"user_id":"account-b"}}}}"#,
    )
    .await
    .unwrap();

    let account_a_backend =
        crate::security::secret_storage::EncryptedFileBackend::new(&account_a.workspace_dir);
    let account_b_backend =
        crate::security::secret_storage::EncryptedFileBackend::new(&account_b.workspace_dir);
    use crate::security::secret_storage::SecretBackend;
    account_a_backend
        .set(
            &crate::security::secret_storage::ops::namespaced_key(
                "account-a",
                &crate::security::credentials::profiles::auth_profile_secret_key(
                    "app-session:default",
                ),
            ),
            r#"{"token":"account-a-session"}"#,
        )
        .unwrap();
    account_b_backend
        .set(
            &crate::security::secret_storage::ops::namespaced_key(
                "account-b",
                &crate::security::credentials::profiles::auth_profile_secret_key(
                    "app-session:default",
                ),
            ),
            r#"{"token":"account-b-session"}"#,
        )
        .unwrap();
    let _backend_override = SecretBackendOverrideGuard;
    crate::security::secret_storage::install_secret_backend(std::sync::Arc::new(account_a_backend));

    let selected = load_desktop_profile(root, "account-b").await.unwrap();
    let before = snapshot_tree(&account_b.dir);
    let profile =
        crate::security::credentials::session_support::load_app_session_profile_read_only(
            &selected.config,
        )
        .unwrap()
        .unwrap();
    let token =
        crate::security::credentials::session_support::session_token_from_profile(Some(&profile));
    let after = snapshot_tree(&account_b.dir);

    assert_eq!(token.as_deref(), Some("account-b-session"));
    assert_eq!(
        before, after,
        "doctor credential inspection must not mutate the selected profile"
    );
}

#[tokio::test]
async fn selected_profile_runs_its_context_scoped_endpoint_enricher() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(
        &layout.config_path,
        "api_url = \"https://account-b.example\"\n",
    )
    .await
    .unwrap();

    let context = crate::core::runtime::CoreContext::for_test(
        crate::core::runtime::DomainSet::full(),
        Some(layout.workspace_dir.clone()),
    );
    crate::config::register_enricher_in(
        context.extensions(),
        "doctor-selected-profile",
        |config| {
            if let Some(api_url) = &config.api_url {
                config.resolved_endpoints.control_plane_url = Some(api_url.clone());
            }
        },
    );
    let selected = crate::core::runtime::CoreContext::scope(context.clone(), async {
        resolve_desktop_profile_for_doctor(root, Some("account-b")).await
    })
    .await
    .unwrap();

    let mut profile = crate::security::credentials::profiles::AuthProfile::new_token(
        crate::security::credentials::APP_SESSION_PROVIDER,
        crate::security::credentials::DEFAULT_AUTH_PROFILE_NAME,
        "session-token".into(),
    );
    profile.metadata.insert(
        crate::security::credentials::session_support::SESSION_ISSUING_BACKEND_META.into(),
        "https://account-b.example".into(),
    );

    assert_eq!(
        selected
            .config
            .resolved_endpoints
            .control_plane_url
            .as_deref(),
        Some("https://account-b.example")
    );
    assert!(
        crate::security::credentials::session_support::check_session_backend(
            &selected.config,
            Some(&profile)
        )
        .is_ok()
    );
}

fn snapshot_tree(root: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        let entries = std::fs::read_dir(directory).unwrap();
        for entry in entries {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[tokio::test]
async fn invalid_explicit_profile_returns_one_safe_profile_exists_error() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "2\n")
        .await
        .unwrap();
    tokio::fs::write(&layout.config_path, "default_model = [\"private-value\"\n")
        .await
        .unwrap();
    let active_config = Config {
        config_path: root.join("users/account-a/config.toml"),
        ..Config::default()
    };

    let outcome = crate::doctor::ops::doctor_report_with_options(
        &active_config,
        DoctorOptions {
            tier: DoctorTier::Connect,
            profile: Some("account-b".into()),
            ..DoctorOptions::default()
        },
    )
    .await
    .unwrap();
    let report = outcome.value;

    assert_eq!(report.profile.as_deref(), Some("account-b"));
    assert_eq!(
        report.config_source,
        crate::doctor::DoctorConfigSource::Profile
    );
    assert_eq!(report.checks_run, ["profile/exists"]);
    assert!(report.checks_skipped.is_empty());
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].check_id, "profile/exists");
    assert_eq!(
        report.findings[0].severity,
        crate::doctor::FindingSeverity::Error
    );
    assert!(!report.findings[0].message.contains("private-value"));
    assert_eq!(report.summary.error, 1);
    assert!(!report.ok);
}

#[tokio::test]
async fn malformed_default_profile_returns_only_a_safe_config_parse_error() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let active_user = root.join("active_user.toml");
    tokio::fs::write(&active_user, "user_id = \"account-b\"\n")
        .await
        .unwrap();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    let invalid_contents = b"default_temperature = \"private-value\"\n";
    tokio::fs::write(&layout.config_path, invalid_contents)
        .await
        .unwrap();
    let before = snapshot_tree(root);

    let selection = resolve_desktop_profile_for_doctor(root, None)
        .await
        .unwrap();
    let outcome = crate::doctor::ops::doctor_report_for_desktop_selection(
        selection,
        DoctorOptions {
            tier: DoctorTier::Static,
            ..DoctorOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.value.findings.len(), 1);
    assert_eq!(outcome.value.findings[0].check_id, "config/parse");
    assert_eq!(outcome.value.findings[0].severity, FindingSeverity::Error);
    assert_eq!(
        outcome.value.findings[0].config_path.as_deref(),
        Some("default_temperature")
    );
    assert!(!outcome.value.findings[0].message.contains("private-value"));
    assert_eq!(before, snapshot_tree(root));
}

#[tokio::test]
async fn explicit_dispatch_selection_does_not_load_the_ambient_config() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let active_config_path = root.join("users/account-a/config.toml");
    tokio::fs::create_dir_all(&active_config_path)
        .await
        .unwrap();
    let selected = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&selected.dir).await.unwrap();
    tokio::fs::write(
        &selected.config_path,
        "default_model = \"account-b-model\"\n",
    )
    .await
    .unwrap();
    let active_config = Config {
        config_path: active_config_path,
        workspace_dir: root.join("users/account-a/workspace"),
        ..Config::default()
    };
    let core = crate::core::runtime::CoreContext::for_test_with_config(
        crate::core::runtime::DomainSet::full(),
        active_config,
    );
    let dispatch = crate::core::runtime::DispatchCtx::new(core);

    let outcome = crate::doctor::ops::doctor_report_for_dispatch(
        &dispatch,
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec!["profile/exists".into()],
            profile: Some("account-b".into()),
            ..DoctorOptions::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(outcome.value.profile.as_deref(), Some("account-b"));
    assert_eq!(outcome.value.checks_run, ["profile/exists"]);
    assert_eq!(outcome.value.findings[0].severity, FindingSeverity::Pass);
}

#[tokio::test]
async fn selected_profile_context_pins_id_layout_source_and_components() {
    let temp = TempDir::new().unwrap();
    let layout = ProfileLayout::new(temp.path(), "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(&layout.config_path, "[components]\nmemory = false\n")
        .await
        .unwrap();
    let selected = resolve_desktop_profile_for_doctor(temp.path(), Some("account-b"))
        .await
        .unwrap();
    let components = crate::core::runtime::ComponentSet::from_config(&selected.config.components);
    let version = selected.layout_version;
    let profile_id = selected.profile_id.clone().unwrap();
    let resolution_source = selected.resolution_source.clone();
    let status = selected.profile_status;
    let mut ctx = DoctorCtx::from_config_with_components(selected.config, components)
        .with_profile_metadata(version, Some(resolution_source));
    ctx = ctx.with_profile(profile_id, DoctorConfigSource::Profile);
    if let Some(status) = status {
        ctx = ctx.with_profile_status(status);
    }

    assert_eq!(ctx.profile(), Some("account-b"));
    assert_eq!(ctx.config_source(), DoctorConfigSource::Profile);
    assert_eq!(ctx.profile_layout_version(), version);
    assert_eq!(ctx.profile_resolution_source(), Some("--profile"));
    assert!(!ctx.components().memory);

    let check = super::built_in_checks().pop().unwrap();
    let findings = check.detect(&ctx).await;
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, FindingSeverity::Pass);
}

#[tokio::test]
async fn report_serializes_active_profile_layout_and_resolution_source() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let active_user = root.join("active_user.toml");
    tokio::fs::write(&active_user, "user_id = \"account-b\"\n")
        .await
        .unwrap();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "2\n")
        .await
        .unwrap();
    tokio::fs::write(&layout.config_path, "[components]\nmemory = false\n")
        .await
        .unwrap();

    let selection = resolve_desktop_profile_for_doctor(root, None)
        .await
        .unwrap();
    let outcome = crate::doctor::ops::doctor_report_for_desktop_selection(
        selection,
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec!["profile/exists".into()],
            ..DoctorOptions::default()
        },
    )
    .await
    .unwrap();
    let json = serde_json::to_value(outcome.value).unwrap();

    assert_eq!(
        json.get("profile_layout_version")
            .and_then(serde_json::Value::as_u64),
        Some(2)
    );
    assert_eq!(
        json.get("config_resolution_source")
            .and_then(serde_json::Value::as_str),
        Some("active_user.toml")
    );
}

#[tokio::test]
async fn selected_profile_reports_preserve_parent_check_sources_without_using_parent_state() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let account_a = ProfileLayout::new(root, "account-a");
    let account_b = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&account_a.dir).await.unwrap();
    tokio::fs::create_dir_all(&account_b.dir).await.unwrap();
    tokio::fs::write(
        &account_a.config_path,
        "api_url = \"https://account-a.example\"\n",
    )
    .await
    .unwrap();
    tokio::fs::write(
        &account_b.config_path,
        "api_url = \"https://account-b.example\"\n[components]\nmemory = false\n",
    )
    .await
    .unwrap();

    let mut account_a_config = Config {
        config_path: account_a.config_path.clone(),
        workspace_dir: account_a.workspace_dir.clone(),
        api_url: Some("https://account-a.example".into()),
        ..Config::default()
    };
    account_a_config.components.memory = true;
    let parent = crate::core::runtime::CoreContext::for_test_with_config(
        crate::core::runtime::DomainSet::kernel(),
        account_a_config.clone(),
    )
    .derive_with(
        crate::core::runtime::ContextOverlay::new(
            account_a_config,
            crate::core::runtime::DomainSet::kernel(),
            crate::tools::toolpacks::ToolGroups::none(),
        )
        .without_user_skill_roots(),
    );

    let observed = Arc::new(Mutex::new(Vec::new()));
    crate::doctor::register_check_source_in(
        parent.extensions(),
        SelectedProfileContextSource(observed.clone()),
    );
    let before = snapshot_tree(root);
    let explicit_outcome = crate::core::runtime::CoreContext::scope(parent.clone(), async {
        crate::doctor::ops::doctor_report_for_selected_profile(
            root,
            "account-b",
            selected_profile_context_options(),
        )
        .await
    })
    .await
    .unwrap();
    let selection = crate::core::runtime::CoreContext::scope(parent.clone(), async {
        resolve_desktop_profile_for_doctor(root, Some("account-b")).await
    })
    .await
    .unwrap();
    let selection_outcome = crate::core::runtime::CoreContext::scope(parent.clone(), async {
        crate::doctor::ops::doctor_report_for_desktop_selection(
            selection,
            selected_profile_context_options(),
        )
        .await
    })
    .await
    .unwrap();

    for report in [explicit_outcome.value, selection_outcome.value] {
        assert!(
            !report
                .findings
                .iter()
                .any(|finding| finding.check_id == "doctor/selection")
        );
        assert!(
            report
                .checks_run
                .iter()
                .any(|id| id == "hosted/selected-profile-context")
        );
        assert_eq!(report.profile.as_deref(), Some("account-b"));
    }
    let observations = observed.lock().unwrap();
    assert_eq!(observations.len(), 2);
    for observation in observations.iter() {
        assert_eq!(observation.doctor_workspace, account_b.workspace_dir);
        assert_eq!(observation.doctor_config_path, account_b.config_path);
        assert_eq!(
            observation.doctor_api_url.as_deref(),
            Some("https://account-b.example")
        );
        assert_eq!(observation.ambient_workspace, account_b.workspace_dir);
        assert_eq!(
            observation.domains,
            crate::core::runtime::DomainSet::kernel()
        );
        assert_eq!(
            observation.tool_groups,
            crate::tools::toolpacks::ToolGroups::none()
        );
        assert!(!observation.user_skill_roots);
        assert!(!observation.components.memory);
    }
    assert_eq!(before, snapshot_tree(root));
}

#[tokio::test]
async fn selected_profile_is_ambient_for_process_sources_without_a_parent_context() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    assert!(crate::core::runtime::CoreContext::current().is_none());

    let temp = TempDir::new().unwrap();
    let layout = ProfileLayout::new(temp.path(), "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(&layout.config_path, "api_url = \"https://account-b.example\"\n")
        .await
        .unwrap();

    let observed = Arc::new(Mutex::new(Vec::new()));
    let source_id = "doctor-selected-profile-process-context";
    crate::doctor::register_check_source(ProcessSelectedProfileContextSource(observed.clone()));
    let outcome = crate::doctor::ops::doctor_report_for_selected_profile(
        temp.path(),
        "account-b",
        selected_profile_context_options(),
    )
    .await;
    crate::doctor::registry::unregister_check_source(source_id);
    let report = outcome.unwrap().value;

    assert!(
        report
            .checks_run
            .iter()
            .any(|id| id == "hosted/process-selected-profile-context")
    );
    let observations = observed.lock().unwrap();
    assert_eq!(observations.len(), 1);
    assert_eq!(observations[0].ambient_workspace.as_deref(), Some(layout.workspace_dir.as_path()));
}

struct ProcessSelectedProfileContextSource(Arc<Mutex<Vec<ProcessSelectedProfileObservation>>>);

struct ProcessSelectedProfileObservation {
    ambient_workspace: Option<std::path::PathBuf>,
}

struct ProcessSelectedProfileContextCheck(Arc<Mutex<Vec<ProcessSelectedProfileObservation>>>);

#[async_trait::async_trait]
impl DoctorCheck for ProcessSelectedProfileContextCheck {
    fn id(&self) -> &str {
        "hosted/process-selected-profile-context"
    }

    fn category(&self) -> &str {
        "hosted"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    async fn detect(&self, _ctx: &DoctorCtx) -> Vec<Finding> {
        let ambient_workspace = crate::core::runtime::CoreContext::current_embedder_config()
            .map(|config| config.workspace_dir);
        self.0
            .lock()
            .unwrap()
            .push(ProcessSelectedProfileObservation { ambient_workspace });
        vec![Finding::pass(self.id(), "The selected profile is ambient to host checks.")]
    }
}

impl DoctorCheckSource for ProcessSelectedProfileContextSource {
    fn id(&self) -> &str {
        "doctor-selected-profile-process-context"
    }

    fn checks(&self) -> Vec<Arc<dyn DoctorCheck>> {
        vec![Arc::new(ProcessSelectedProfileContextCheck(self.0.clone()))]
    }
}

fn selected_profile_context_options() -> DoctorOptions {
    DoctorOptions {
        tier: DoctorTier::Static,
        only: vec!["hosted/selected-profile-context".into()],
        ..DoctorOptions::default()
    }
}

struct SelectedProfileContextObservation {
    doctor_workspace: std::path::PathBuf,
    doctor_config_path: std::path::PathBuf,
    doctor_api_url: Option<String>,
    ambient_workspace: std::path::PathBuf,
    domains: crate::core::runtime::DomainSet,
    tool_groups: crate::tools::toolpacks::ToolGroups,
    user_skill_roots: bool,
    components: crate::core::runtime::ComponentSet,
}

struct SelectedProfileContextCheck(Arc<Mutex<Vec<SelectedProfileContextObservation>>>);

#[async_trait::async_trait]
impl DoctorCheck for SelectedProfileContextCheck {
    fn id(&self) -> &str {
        "hosted/selected-profile-context"
    }

    fn category(&self) -> &str {
        "hosted"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        let current = crate::core::runtime::CoreContext::current().unwrap();
        let ambient = crate::core::runtime::CoreContext::current_embedder_config().unwrap();
        self.0
            .lock()
            .unwrap()
            .push(SelectedProfileContextObservation {
                doctor_workspace: ctx.config().workspace_dir.clone(),
                doctor_config_path: ctx.config().config_path.clone(),
                doctor_api_url: ctx.config().api_url.clone(),
                ambient_workspace: ambient.workspace_dir,
                domains: current.domains(),
                tool_groups: current.tool_groups(),
                user_skill_roots: current.user_skill_roots(),
                components: ctx.components(),
            });
        vec![Finding::pass(
            self.id(),
            "The selected profile context and host check source are available.",
        )]
    }
}

struct SelectedProfileContextSource(Arc<Mutex<Vec<SelectedProfileContextObservation>>>);

impl DoctorCheckSource for SelectedProfileContextSource {
    fn id(&self) -> &str {
        "doctor-selected-profile-context"
    }

    fn checks(&self) -> Vec<Arc<dyn DoctorCheck>> {
        vec![Arc::new(SelectedProfileContextCheck(self.0.clone()))]
    }
}

#[tokio::test]
async fn selected_profile_applies_in_memory_legacy_migrations_without_saving_them() {
    let temp = TempDir::new().unwrap();
    let layout = ProfileLayout::new(temp.path(), "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    let contents = "api_url = \"https://custom.example/v1/chat/completions\"\n";
    tokio::fs::write(&layout.config_path, contents)
        .await
        .unwrap();

    let selected = load_desktop_profile(temp.path(), "account-b")
        .await
        .unwrap();

    assert_eq!(selected.config.api_url, None);
    assert_eq!(
        selected.config.inference_url.as_deref(),
        Some("https://custom.example/v1/chat/completions")
    );
    assert!(
        selected
            .config
            .load_diagnostics()
            .migrated_keys
            .contains(&"api_url".to_string())
    );
    assert_eq!(
        tokio::fs::read_to_string(&layout.config_path)
            .await
            .unwrap(),
        contents
    );
}

#[tokio::test]
async fn default_profile_resolution_reads_active_marker_without_initializing_layout() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let _workspace_override = WorkspaceOverrideGuard::clear();
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let marker = root.join("active_user.toml");
    tokio::fs::create_dir_all(root).await.unwrap();
    tokio::fs::write(&marker, "user_id = \"account-a\"\n")
        .await
        .unwrap();
    let layout = ProfileLayout::new(root, "account-a");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(&layout.config_path, "default_model = \"account-a-model\"\n")
        .await
        .unwrap();
    let marker_before = tokio::fs::read(&marker).await.unwrap();

    let selected = resolve_desktop_profile_for_doctor(root, None)
        .await
        .unwrap();

    assert_eq!(selected.profile_id.as_deref(), Some("account-a"));
    assert_eq!(selected.layout_version, Some(layout.version));
    assert_eq!(selected.resolution_source, "active_user.toml");
    assert_eq!(
        selected.config.default_model.as_deref(),
        Some("account-a-model")
    );
    assert!(!layout.dir.join(".layout").exists());
    assert_eq!(tokio::fs::read(&marker).await.unwrap(), marker_before);
}

#[tokio::test]
async fn default_profile_resolution_prefers_workspace_override_without_switching_user() {
    let _env_guard = crate::config::TEST_ENV_LOCK.lock().await;
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    tokio::fs::create_dir_all(root).await.unwrap();
    let active_user = root.join("active_user.toml");
    tokio::fs::write(&active_user, "user_id = \"account-a\"\n")
        .await
        .unwrap();
    let account_b = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&account_b.dir).await.unwrap();
    tokio::fs::write(account_b.dir.join(".layout"), "2\n")
        .await
        .unwrap();
    tokio::fs::write(
        &account_b.config_path,
        "default_model = \"account-b-model\"\n",
    )
    .await
    .unwrap();
    let marker_before = tokio::fs::read(&active_user).await.unwrap();
    let _workspace_override = WorkspaceOverrideGuard::set(&account_b.files_dir);

    let selected = resolve_desktop_profile_for_doctor(root, None)
        .await
        .unwrap();

    assert_eq!(selected.profile_id.as_deref(), Some("account-b"));
    assert_eq!(
        selected.layout_version,
        Some(crate::config::schema::profile_layout::LayoutVersion::V2)
    );
    assert_eq!(selected.resolution_source, "OPENHUMAN_WORKSPACE");
    assert_eq!(selected.config.workspace_dir, account_b.files_dir);
    assert_eq!(
        selected.config.default_model.as_deref(),
        Some("account-b-model")
    );
    assert_eq!(tokio::fs::read(&active_user).await.unwrap(), marker_before);
    assert_eq!(
        tokio::fs::read_to_string(account_b.dir.join(".layout"))
            .await
            .unwrap(),
        "2\n"
    );
}

#[tokio::test]
async fn missing_profile_is_reported_without_creating_or_switching_a_profile() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let active_user_path = root.join("active_user.toml");
    tokio::fs::create_dir_all(root).await.unwrap();
    tokio::fs::write(&active_user_path, "user_id = \"account-a\"\n")
        .await
        .unwrap();
    let marker_before = tokio::fs::read(&active_user_path).await.unwrap();

    let result = load_desktop_profile(root, "missing").await;

    assert!(matches!(result, Err(DesktopProfileError::Missing)));
    assert!(!root.join("users/missing").exists());
    assert_eq!(
        tokio::fs::read(&active_user_path).await.unwrap(),
        marker_before
    );
}

#[tokio::test]
async fn unknown_layout_marker_is_rejected_without_repairing_it() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "99\n")
        .await
        .unwrap();
    tokio::fs::write(&layout.config_path, "default_model = \"account-b-model\"\n")
        .await
        .unwrap();

    let result = load_desktop_profile(root, "account-b").await;

    assert!(matches!(
        result,
        Err(DesktopProfileError::UnknownLayoutMarker)
    ));
    assert_eq!(
        tokio::fs::read_to_string(layout.dir.join(".layout"))
            .await
            .unwrap(),
        "99\n"
    );
}

#[tokio::test]
async fn invalid_profile_config_is_rejected_without_persisting_recovery() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();
    tokio::fs::write(layout.dir.join(".layout"), "2\n")
        .await
        .unwrap();
    let invalid = "default_model = [\"unfinished\"\n";
    tokio::fs::write(&layout.config_path, invalid)
        .await
        .unwrap();
    tokio::fs::write(layout.config_path.with_extension("toml.bak"), "")
        .await
        .unwrap();

    let result = load_desktop_profile(root, "account-b").await;

    assert!(matches!(result, Err(DesktopProfileError::InvalidConfig)));
    assert_eq!(
        tokio::fs::read_to_string(&layout.config_path)
            .await
            .unwrap(),
        invalid
    );
    assert_eq!(
        tokio::fs::read_to_string(layout.config_path.with_extension("toml.bak"))
            .await
            .unwrap(),
        ""
    );
}

#[tokio::test]
async fn empty_profile_resolution_does_not_write_a_layout_marker() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let layout = ProfileLayout::new(root, "account-b");
    tokio::fs::create_dir_all(&layout.dir).await.unwrap();

    let _ = load_desktop_profile(root, "account-b").await;

    assert!(!layout.dir.join(".layout").exists());
}
