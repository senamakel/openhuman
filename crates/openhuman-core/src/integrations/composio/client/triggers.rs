//! [`ComposioClient`]'s GitHub-repo listing and trigger-management surface:
//! `list_github_repos`, `create_trigger`, `list_available_triggers`,
//! `list_active_triggers`, `enable_trigger`, and `disable_trigger` (the
//! latter via the shared `raw_delete` helper in [`super::connections`]).

use anyhow::Result;
use serde_json::json;

use super::super::types::{
    ComposioActiveTriggersResponse, ComposioAvailableTriggersResponse,
    ComposioCreateTriggerResponse, ComposioDisableTriggerResponse, ComposioEnableTriggerResponse,
    ComposioGithubReposResponse,
};
use super::connections::ComposioClient;

impl ComposioClient {
    pub async fn list_github_repos(
        &self,
        connection_id: Option<&str>,
    ) -> Result<ComposioGithubReposResponse> {
        let path = match connection_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => format!("/agent-integrations/composio/github/repos?connectionId={id}"),
            None => "/agent-integrations/composio/github/repos".to_string(),
        };
        tracing::debug!(path = %path, "[composio] list_github_repos");
        self.inner.get::<ComposioGithubReposResponse>(&path).await
    }

    /// `POST /agent-integrations/composio/triggers` — create a trigger
    /// instance for the authenticated user.
    pub async fn create_trigger(
        &self,
        slug: &str,
        connection_id: Option<&str>,
        trigger_config: Option<serde_json::Value>,
    ) -> Result<ComposioCreateTriggerResponse> {
        let slug = slug.trim();
        if slug.is_empty() {
            anyhow::bail!("composio.create_trigger: slug must not be empty");
        }
        let mut body = json!({ "slug": slug });
        if let Some(connection_id) = connection_id.map(str::trim).filter(|id| !id.is_empty()) {
            body["connectionId"] = json!(connection_id);
        }
        if let Some(config) = trigger_config {
            body["triggerConfig"] = config;
        }
        tracing::debug!(slug = %slug, "[composio] create_trigger");
        self.inner
            .post::<ComposioCreateTriggerResponse>("/agent-integrations/composio/triggers", &body)
            .await
    }

    // ── Trigger management (PR #671) ────────────────────────────────

    /// `GET /agent-integrations/composio/triggers/available` — catalog of
    /// triggers the user could enable for a toolkit. For GitHub the
    /// backend fans out into per-repo entries scoped by `connection_id`.
    pub async fn list_available_triggers(
        &self,
        toolkit: &str,
        connection_id: Option<&str>,
    ) -> Result<ComposioAvailableTriggersResponse> {
        let toolkit = toolkit.trim();
        if toolkit.is_empty() {
            anyhow::bail!("composio.list_available_triggers: toolkit must not be empty");
        }
        let toolkit_q = urlencoding::encode(toolkit);
        let path = match connection_id.map(str::trim).filter(|id| !id.is_empty()) {
            Some(id) => format!(
                "/agent-integrations/composio/triggers/available?toolkit={toolkit_q}&connectionId={}",
                urlencoding::encode(id)
            ),
            None => format!(
                "/agent-integrations/composio/triggers/available?toolkit={toolkit_q}"
            ),
        };
        tracing::debug!(path = %path, "[composio] list_available_triggers");
        self.inner
            .get::<ComposioAvailableTriggersResponse>(&path)
            .await
    }

    /// `GET /agent-integrations/composio/triggers` — currently enabled
    /// triggers for the user, optionally filtered to a toolkit.
    pub async fn list_active_triggers(
        &self,
        toolkit: Option<&str>,
    ) -> Result<ComposioActiveTriggersResponse> {
        let path = match toolkit.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => format!(
                "/agent-integrations/composio/triggers?toolkit={}",
                urlencoding::encode(t)
            ),
            None => "/agent-integrations/composio/triggers".to_string(),
        };
        tracing::debug!(path = %path, "[composio] list_active_triggers");
        self.inner
            .get::<ComposioActiveTriggersResponse>(&path)
            .await
    }

    /// `POST /agent-integrations/composio/triggers` — enable a single
    /// trigger on a connection the caller owns.
    pub async fn enable_trigger(
        &self,
        connection_id: &str,
        slug: &str,
        trigger_config: Option<serde_json::Value>,
    ) -> Result<ComposioEnableTriggerResponse> {
        let connection_id = connection_id.trim();
        let slug = slug.trim();
        if connection_id.is_empty() {
            anyhow::bail!("composio.enable_trigger: connectionId must not be empty");
        }
        if slug.is_empty() {
            anyhow::bail!("composio.enable_trigger: slug must not be empty");
        }
        let mut body = json!({ "connectionId": connection_id, "slug": slug });
        if let Some(config) = trigger_config {
            body["triggerConfig"] = config;
        }
        tracing::debug!(slug = %slug, connection_id = %connection_id, "[composio] enable_trigger");
        self.inner
            .post::<ComposioEnableTriggerResponse>("/agent-integrations/composio/triggers", &body)
            .await
    }

    /// `DELETE /agent-integrations/composio/triggers/:triggerId`.
    pub async fn disable_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<ComposioDisableTriggerResponse> {
        let trigger_id = trigger_id.trim();
        if trigger_id.is_empty() {
            anyhow::bail!("composio.disable_trigger: triggerId must not be empty");
        }
        tracing::debug!(trigger_id = %trigger_id, "[composio] disable_trigger");
        self.raw_delete::<ComposioDisableTriggerResponse>(&format!(
            "/agent-integrations/composio/triggers/{}",
            urlencoding::encode(trigger_id)
        ))
        .await
    }
}
