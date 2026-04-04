use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use log::{debug, warn};
use reqwest::{header::AUTHORIZATION, Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::api::config::effective_api_url;
use crate::api::jwt::{bearer_authorization_value, get_session_token};
use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::config::Config;
use crate::openhuman::credentials::session_support::build_session_state;
use crate::rpc::RpcOutcome;

const LOG_PREFIX: &str = "[app_state]";
const APP_STATE_FILENAME: &str = "app-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredOnboardingTasks {
    #[serde(default)]
    pub accessibility_permission_granted: bool,
    #[serde(default)]
    pub local_model_consent_given: bool,
    #[serde(default)]
    pub local_model_download_started: bool,
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub connected_sources: Vec<String>,
    #[serde(default)]
    pub updated_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredAppState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_wallet_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onboarding_tasks: Option<StoredOnboardingTasks>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateSnapshot {
    pub auth: crate::openhuman::credentials::responses::AuthStateResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_user: Option<Value>,
    pub onboarding_completed: bool,
    pub analytics_enabled: bool,
    pub local_state: StoredAppState,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredAppStatePatch {
    #[serde(default)]
    pub encryption_key: Option<Option<String>>,
    #[serde(default)]
    pub primary_wallet_address: Option<Option<String>>,
    #[serde(default)]
    pub onboarding_tasks: Option<Option<StoredOnboardingTasks>>,
}

fn app_state_path(config: &Config) -> Result<PathBuf, String> {
    let state_dir = config.workspace_dir.join("state");
    fs::create_dir_all(&state_dir)
        .map_err(|e| format!("failed to create workspace state dir {}: {e}", state_dir.display()))?;
    Ok(state_dir.join(APP_STATE_FILENAME))
}

fn load_stored_app_state(config: &Config) -> Result<StoredAppState, String> {
    let path = app_state_path(config)?;
    if !path.exists() {
        return Ok(StoredAppState::default());
    }

    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_json::from_str::<StoredAppState>(&raw)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn save_stored_app_state(config: &Config, state: &StoredAppState) -> Result<(), String> {
    let path = app_state_path(config)?;
    let payload = serde_json::to_string_pretty(state)
        .map_err(|e| format!("failed to serialize app state: {e}"))?;
    fs::write(&path, payload).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn build_client() -> Result<Client, String> {
    Client::builder()
        .use_rustls_tls()
        .http1_only()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

fn resolve_base(config: &Config) -> Result<Url, String> {
    let base = effective_api_url(&config.api_url);
    Url::parse(base.trim()).map_err(|e| format!("invalid api_url '{}': {e}", base))
}

async fn fetch_current_user(config: &Config, token: &str) -> Result<Option<Value>, String> {
    let client = build_client()?;
    let base = resolve_base(config)?;
    let url = base
        .join("telegram/me")
        .map_err(|e| format!("build URL failed: {e}"))?;
    let response = client
        .request(Method::GET, url.clone())
        .header(AUTHORIZATION, bearer_authorization_value(token))
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("failed to read backend response body: {e}"))?;

    debug!("{LOG_PREFIX} GET /telegram/me -> {}", status);

    if !status.is_success() {
        warn!("{LOG_PREFIX} current user fetch failed: {} {}", status, text);
        return Ok(None);
    }

    let raw: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.to_string()));
    let user = raw
        .as_object()
        .and_then(|obj| obj.get("data"))
        .cloned()
        .unwrap_or(raw);
    Ok(Some(user))
}

pub async fn snapshot() -> Result<RpcOutcome<AppStateSnapshot>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let auth = build_session_state(&config)?;
    let session_token = get_session_token(&config)?;
    let current_user = if let Some(token) = session_token.clone().filter(|t| !t.trim().is_empty()) {
        fetch_current_user(&config, &token).await?
    } else {
        None
    };
    let local_state = load_stored_app_state(&config)?;

    debug!(
        "{LOG_PREFIX} snapshot auth={} onboarding={} analytics={} wallet_present={}",
        auth.is_authenticated,
        config.onboarding_completed,
        config.observability.analytics_enabled,
        local_state.primary_wallet_address.is_some()
    );

    Ok(RpcOutcome::new(
        AppStateSnapshot {
            auth,
            session_token,
            current_user,
            onboarding_completed: config.onboarding_completed,
            analytics_enabled: config.observability.analytics_enabled,
            local_state,
        },
        vec!["core app state snapshot fetched".to_string()],
    ))
}

pub async fn update_local_state(
    patch: StoredAppStatePatch,
) -> Result<RpcOutcome<StoredAppState>, String> {
    let config = config_rpc::load_config_with_timeout().await?;
    let mut current = load_stored_app_state(&config)?;

    if let Some(encryption_key) = patch.encryption_key {
        current.encryption_key = encryption_key.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
    }

    if let Some(primary_wallet_address) = patch.primary_wallet_address {
        current.primary_wallet_address = primary_wallet_address.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
    }

    if let Some(onboarding_tasks) = patch.onboarding_tasks {
        current.onboarding_tasks = onboarding_tasks;
    }

    save_stored_app_state(&config, &current)?;

    debug!(
        "{LOG_PREFIX} local state updated encryption_key={} wallet={} onboarding_tasks={}",
        current.encryption_key.is_some(),
        current.primary_wallet_address.is_some(),
        current.onboarding_tasks.is_some()
    );

    Ok(RpcOutcome::new(
        current,
        vec!["core local app state updated".to_string()],
    ))
}
