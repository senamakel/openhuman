//! Serde types exchanged with the frontend: the persisted local app-state
//! blob, the patch applied to it, and the polled `app_state_snapshot` shape.

use crate::inference::LocalAiStatus;
use crate::platform::service::ServiceStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub onboarding_tasks: Option<StoredOnboardingTasks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyring_consent: Option<crate::security::keyring_consent::ConsentPreference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateSnapshot {
    pub auth: crate::security::credentials::responses::AuthStateResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_user: Option<Value>,
    pub onboarding_completed: bool,
    /// Deprecated — the welcome agent has been removed. Retained in the
    /// snapshot for backward compatibility with frontend code that still
    /// reads it. This value may be `false` in newer configs; routing no
    /// longer depends on this field.
    pub chat_onboarding_completed: bool,
    pub analytics_enabled: bool,
    pub local_state: StoredAppState,
    pub keyring_status: crate::security::keyring_consent::KeyringStatus,
    pub runtime: RuntimeSnapshot,
    /// Process + component health, folded into this snapshot so the frontend
    /// hydrates the daemon-health store from the same poll instead of running a
    /// second `health_snapshot` poller. Fields stay snake_case (the type has no
    /// camelCase rename) to match the frontend's existing health parser.
    pub health: crate::platform::health::HealthSnapshot,
    /// `true` when this session's config loader had to recover a corrupted
    /// `config.toml` (renamed to `.corrupted.<ts>` and reset to defaults / a
    /// backup). Latched at boot so it stays reported even after the file is
    /// healed; the frontend raises a one-shot "settings were reset" notice
    /// (#5167). Serialized as `configRecovered`.
    pub config_recovered: bool,
    /// `true` when `current_user` came from the stored snapshot because the
    /// backend could not be refreshed — the plan tier, credit balance and
    /// feature flags in it may be out of date (#5930).
    ///
    /// The frontend can warn on this; the core deliberately does not decide
    /// what "significantly out of date" means, because that threshold belongs
    /// to whatever surface is presenting the number.
    pub current_user_stale: bool,
    /// Seconds since the backend last answered `auth_get_me` in this process.
    ///
    /// Absent when it never has — the stored snapshot then came off disk and
    /// its real age is not knowable here, which is a different statement from
    /// "zero seconds old" and is why this is an `Option`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_user_stale_seconds: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSnapshot {
    pub local_ai: LocalAiStatus,
    pub service: ServiceStatus,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StoredAppStatePatch {
    #[serde(default)]
    pub encryption_key: Option<Option<String>>,
    #[serde(default)]
    pub onboarding_tasks: Option<Option<StoredOnboardingTasks>>,
    #[serde(default)]
    pub keyring_consent: Option<Option<crate::security::keyring_consent::ConsentPreference>>,
}
