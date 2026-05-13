//! Composio HTTP client.
//!
//! Routes every call through one of two backends:
//!
//! - **Proxy** (default) — talks to the openhuman backend's
//!   `/agent-integrations/composio/*` endpoints. The backend owns the
//!   shared Composio API key, the toolkit allowlist, billing/margin,
//!   HMAC webhook verification, and Socket.IO trigger fan-out. This is
//!   the original behaviour and still kicks in for any user who has
//!   not set `config.composio.byo_api_key`.
//!
//! - **Direct** ("BYO" — Bring Your Own key) — talks to
//!   `https://backend.composio.dev/api/v3/*` directly using the user's
//!   own Composio API key. See [`super::direct`] for the supported
//!   surface and the BYO-unsupported error contract.
//!
//! Public method signatures are stable across both backends so the
//! domain modules ([`super::ops`], [`super::tools`], [`super::schemas`])
//! do not need to know which backend is active.
//!
//! Logging uses the `[composio]` grep-prefix on the entry side and the
//! `[composio:direct]` / `[composio:proxy]` prefixes inside the
//! per-backend implementations so the sidecar logs can be filtered
//! either at the domain level or per backend.

use std::sync::Arc;

use anyhow::Result;
use serde_json::json;

use crate::openhuman::integrations::IntegrationClient;

use super::direct::DirectComposioClient;
use super::types::{
    ComposioActiveTriggersResponse, ComposioAuthorizeResponse, ComposioAvailableTriggersResponse,
    ComposioConnectionsResponse, ComposioCreateTriggerResponse, ComposioDeleteResponse,
    ComposioDisableTriggerResponse, ComposioEnableTriggerResponse, ComposioExecuteResponse,
    ComposioGithubReposResponse, ComposioToolkitsResponse, ComposioToolsResponse,
};

/// Internal dispatch enum. Held inside `ComposioClient`, never exposed
/// directly — public method signatures stay stable for all callers.
#[derive(Clone)]
enum ComposioBackend {
    Proxy(Arc<IntegrationClient>),
    Direct(DirectComposioClient),
}

/// High-level Composio client. Cheap to clone — both variants share
/// their connection pool via `Arc`.
#[derive(Clone)]
pub struct ComposioClient {
    backend: ComposioBackend,
}

impl ComposioClient {
    /// Build a proxy-backed client (the historical constructor).
    /// Equivalent to the previous behaviour: every call hits the
    /// openhuman backend's `/agent-integrations/composio/*` routes.
    pub fn new(inner: Arc<IntegrationClient>) -> Self {
        Self {
            backend: ComposioBackend::Proxy(inner),
        }
    }

    /// Build a BYO direct-backed client.
    pub fn direct(direct: DirectComposioClient) -> Self {
        Self {
            backend: ComposioBackend::Direct(direct),
        }
    }

    /// True when this client is talking to Composio directly using the
    /// user's BYO API key (as opposed to the openhuman backend proxy).
    pub fn is_byo(&self) -> bool {
        matches!(self.backend, ComposioBackend::Direct(_))
    }

    /// Stable, human-readable backend label for diagnostics.
    pub fn backend_label(&self) -> &'static str {
        match self.backend {
            ComposioBackend::Proxy(_) => "proxy",
            ComposioBackend::Direct(_) => "direct",
        }
    }

    /// Access the underlying proxy integration client. Panics in
    /// BYO/Direct mode — callers that need raw access (tests, the
    /// legacy `raw_delete` path) only run on the proxy variant.
    pub fn inner(&self) -> &Arc<IntegrationClient> {
        match &self.backend {
            ComposioBackend::Proxy(c) => c,
            ComposioBackend::Direct(_) => {
                panic!("ComposioClient::inner() called on a BYO/Direct-mode client")
            }
        }
    }

    // ── Toolkits ────────────────────────────────────────────────────

    pub async fn list_toolkits(&self) -> Result<ComposioToolkitsResponse> {
        tracing::debug!(backend = self.backend_label(), "[composio] list_toolkits");
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                inner
                    .get::<ComposioToolkitsResponse>("/agent-integrations/composio/toolkits")
                    .await
            }
            ComposioBackend::Direct(d) => d.list_toolkits().await,
        }
    }

    // ── Connections ─────────────────────────────────────────────────

    pub async fn list_connections(&self) -> Result<ComposioConnectionsResponse> {
        tracing::debug!(
            backend = self.backend_label(),
            "[composio] list_connections"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                inner
                    .get::<ComposioConnectionsResponse>("/agent-integrations/composio/connections")
                    .await
            }
            ComposioBackend::Direct(d) => d.list_connections().await,
        }
    }

    pub async fn authorize(
        &self,
        toolkit: &str,
        extra_params: Option<serde_json::Value>,
    ) -> Result<ComposioAuthorizeResponse> {
        let toolkit = toolkit.trim();
        if toolkit.is_empty() {
            anyhow::bail!("composio.authorize: toolkit must not be empty");
        }
        // Input validation for `extra_params` must run on both backends
        // — the proxy path enforces it server-side too, but failing fast
        // here gives identical error messages regardless of backend.
        if let Some(ref extra) = extra_params {
            const RESERVED: &[&str] = &["toolkit", "toolkit_version", "auth", "client_id"];
            let extra_obj = extra.as_object().ok_or_else(|| {
                anyhow::anyhow!("composio.authorize: extra_params must be a JSON object")
            })?;
            for k in extra_obj.keys() {
                if RESERVED.contains(&k.as_str()) {
                    anyhow::bail!(
                        "composio.authorize: extra_params cannot override reserved key '{k}'"
                    );
                }
            }
        }
        tracing::debug!(
            backend = self.backend_label(),
            toolkit = %toolkit,
            has_extra_params = extra_params.is_some(),
            "[composio] authorize"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let mut body = json!({ "toolkit": toolkit });
                if let Some(extra) = extra_params {
                    let extra_obj = extra.as_object().expect("validated above");
                    let obj = body.as_object_mut().expect("body is object");
                    for (k, v) in extra_obj {
                        obj.insert(k.clone(), v.clone());
                    }
                }
                inner
                    .post::<ComposioAuthorizeResponse>(
                        "/agent-integrations/composio/authorize",
                        &body,
                    )
                    .await
            }
            ComposioBackend::Direct(d) => d.authorize().await,
        }
    }

    pub async fn delete_connection(&self, connection_id: &str) -> Result<ComposioDeleteResponse> {
        let connection_id = connection_id.trim();
        if connection_id.is_empty() {
            anyhow::bail!("composio.delete_connection: connectionId must not be empty");
        }
        tracing::debug!(
            backend = self.backend_label(),
            connection_id = %connection_id,
            "[composio] delete_connection"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                raw_delete::<ComposioDeleteResponse>(
                    inner,
                    &format!("/agent-integrations/composio/connections/{connection_id}"),
                )
                .await
            }
            ComposioBackend::Direct(d) => d.delete_connection(connection_id).await,
        }
    }

    // ── Tools ───────────────────────────────────────────────────────

    pub async fn list_tools(&self, toolkits: Option<&[String]>) -> Result<ComposioToolsResponse> {
        tracing::debug!(backend = self.backend_label(), "[composio] list_tools");
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let path = match toolkits {
                    Some(list) if !list.is_empty() => {
                        let joined = list
                            .iter()
                            .map(|t| t.trim())
                            .filter(|t| !t.is_empty())
                            .collect::<Vec<_>>()
                            .join(",");
                        format!("/agent-integrations/composio/tools?toolkits={joined}")
                    }
                    _ => "/agent-integrations/composio/tools".to_string(),
                };
                inner.get::<ComposioToolsResponse>(&path).await
            }
            ComposioBackend::Direct(d) => d.list_tools(toolkits).await,
        }
    }

    // ── Execute ─────────────────────────────────────────────────────

    pub async fn execute_tool(
        &self,
        tool: &str,
        arguments: Option<serde_json::Value>,
    ) -> Result<ComposioExecuteResponse> {
        let tool = tool.trim();
        if tool.is_empty() {
            anyhow::bail!("composio.execute_tool: tool slug must not be empty");
        }
        tracing::debug!(
            backend = self.backend_label(),
            tool = %tool,
            "[composio] execute_tool"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let arguments = arguments.unwrap_or(serde_json::Value::Object(Default::default()));
                let body = json!({ "tool": tool, "arguments": arguments });
                inner
                    .post::<ComposioExecuteResponse>("/agent-integrations/composio/execute", &body)
                    .await
            }
            ComposioBackend::Direct(d) => d.execute_tool(tool, arguments).await,
        }
    }

    pub async fn list_github_repos(
        &self,
        connection_id: Option<&str>,
    ) -> Result<ComposioGithubReposResponse> {
        tracing::debug!(
            backend = self.backend_label(),
            "[composio] list_github_repos"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let path = match connection_id.map(str::trim).filter(|id| !id.is_empty()) {
                    Some(id) => {
                        format!("/agent-integrations/composio/github/repos?connectionId={id}")
                    }
                    None => "/agent-integrations/composio/github/repos".to_string(),
                };
                inner.get::<ComposioGithubReposResponse>(&path).await
            }
            ComposioBackend::Direct(d) => d.list_github_repos().await,
        }
    }

    // ── Triggers ────────────────────────────────────────────────────
    //
    // All trigger methods are BYO-unsupported (see `direct.rs`). They
    // still validate inputs identically on both backends so the error
    // surface is consistent.

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
        tracing::debug!(
            backend = self.backend_label(),
            slug = %slug,
            "[composio] create_trigger"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let mut body = json!({ "slug": slug });
                if let Some(connection_id) =
                    connection_id.map(str::trim).filter(|id| !id.is_empty())
                {
                    body["connectionId"] = json!(connection_id);
                }
                if let Some(config) = trigger_config {
                    body["triggerConfig"] = config;
                }
                inner
                    .post::<ComposioCreateTriggerResponse>(
                        "/agent-integrations/composio/triggers",
                        &body,
                    )
                    .await
            }
            ComposioBackend::Direct(d) => d.create_trigger().await,
        }
    }

    pub async fn list_available_triggers(
        &self,
        toolkit: &str,
        connection_id: Option<&str>,
    ) -> Result<ComposioAvailableTriggersResponse> {
        let toolkit = toolkit.trim();
        if toolkit.is_empty() {
            anyhow::bail!("composio.list_available_triggers: toolkit must not be empty");
        }
        tracing::debug!(
            backend = self.backend_label(),
            toolkit = %toolkit,
            "[composio] list_available_triggers"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
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
                inner.get::<ComposioAvailableTriggersResponse>(&path).await
            }
            ComposioBackend::Direct(d) => d.list_available_triggers().await,
        }
    }

    pub async fn list_active_triggers(
        &self,
        toolkit: Option<&str>,
    ) -> Result<ComposioActiveTriggersResponse> {
        tracing::debug!(
            backend = self.backend_label(),
            "[composio] list_active_triggers"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let path = match toolkit.map(str::trim).filter(|t| !t.is_empty()) {
                    Some(t) => format!(
                        "/agent-integrations/composio/triggers?toolkit={}",
                        urlencoding::encode(t)
                    ),
                    None => "/agent-integrations/composio/triggers".to_string(),
                };
                inner.get::<ComposioActiveTriggersResponse>(&path).await
            }
            ComposioBackend::Direct(d) => d.list_active_triggers().await,
        }
    }

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
        tracing::debug!(
            backend = self.backend_label(),
            slug = %slug,
            connection_id = %connection_id,
            "[composio] enable_trigger"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                let mut body = json!({ "connectionId": connection_id, "slug": slug });
                if let Some(config) = trigger_config {
                    body["triggerConfig"] = config;
                }
                inner
                    .post::<ComposioEnableTriggerResponse>(
                        "/agent-integrations/composio/triggers",
                        &body,
                    )
                    .await
            }
            ComposioBackend::Direct(d) => d.enable_trigger().await,
        }
    }

    pub async fn disable_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<ComposioDisableTriggerResponse> {
        let trigger_id = trigger_id.trim();
        if trigger_id.is_empty() {
            anyhow::bail!("composio.disable_trigger: triggerId must not be empty");
        }
        tracing::debug!(
            backend = self.backend_label(),
            trigger_id = %trigger_id,
            "[composio] disable_trigger"
        );
        match &self.backend {
            ComposioBackend::Proxy(inner) => {
                raw_delete::<ComposioDisableTriggerResponse>(
                    inner,
                    &format!(
                        "/agent-integrations/composio/triggers/{}",
                        urlencoding::encode(trigger_id)
                    ),
                )
                .await
            }
            ComposioBackend::Direct(d) => d.disable_trigger().await,
        }
    }
}

// ── Raw DELETE (proxy backend only) ─────────────────────────────────

/// Perform an HTTP DELETE through the backend proxy and parse the
/// standard backend envelope. [`IntegrationClient`] only exposes
/// `get` / `post`, so the proxy DELETE path re-implements envelope
/// handling here.
async fn raw_delete<T: serde::de::DeserializeOwned>(
    inner: &Arc<IntegrationClient>,
    path: &str,
) -> Result<T> {
    #[derive(serde::Deserialize)]
    struct Envelope<T> {
        #[serde(default)]
        success: bool,
        data: Option<T>,
        #[serde(default)]
        error: Option<String>,
    }

    let url = format!("{}{}", inner.backend_url, path);
    tracing::debug!("[composio] DELETE {}", url);

    let http_client = reqwest::Client::builder()
        .use_rustls_tls()
        .http1_only()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = http_client
        .delete(&url)
        .header("Authorization", format!("Bearer {}", inner.auth_token))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body_text = resp.text().await.unwrap_or_default();
        let detail = crate::openhuman::integrations::client::extract_error_detail(
            &body_text,
            crate::openhuman::integrations::client::MAX_ERROR_BODY_LEN,
        );
        let logged_body =
            crate::openhuman::integrations::client::extract_error_detail(&body_text, 300);
        tracing::debug!(
            "[composio] DELETE {} → {} body={}",
            url,
            status,
            logged_body
        );
        let status_str = status.as_u16().to_string();
        crate::core::observability::report_error(
            format!("Backend returned {status} for DELETE {url}: {detail}").as_str(),
            "composio",
            "delete",
            &[
                ("path", path),
                ("status", status_str.as_str()),
                ("failure", "non_2xx"),
            ],
        );
        anyhow::bail!("Backend returned {status} for DELETE {url}: {detail}");
    }

    let envelope: Envelope<T> = resp.json().await?;
    if !envelope.success {
        let msg = envelope
            .error
            .unwrap_or_else(|| "unknown backend error".into());
        crate::core::observability::report_error(
            msg.as_str(),
            "composio",
            "delete",
            &[("path", path), ("failure", "envelope_error")],
        );
        anyhow::bail!("Backend error for DELETE {}: {}", url, msg);
    }
    envelope
        .data
        .ok_or_else(|| anyhow::anyhow!("Backend returned success but no data for DELETE {}", url))
}

/// Build a [`ComposioClient`] from the root config.
///
/// Routing:
///
/// 1. If `config.composio.byo_api_key` is set (non-empty), return a
///    BYO/Direct client that talks to `api.composio.dev` using that
///    key. Does **not** require the user to be signed in to the
///    openhuman backend.
/// 2. Otherwise fall back to the original proxy-backed client built
///    through [`crate::openhuman::integrations::build_client`]. Returns
///    `None` only when the user has no app-session JWT (i.e. not
///    signed in).
pub fn build_composio_client(config: &crate::openhuman::config::Config) -> Option<ComposioClient> {
    if let Some(api_key) = config.composio.byo_api_key_trimmed() {
        match super::direct::DirectComposioClient::new(
            api_key.to_string(),
            config.composio.entity_id.clone(),
        ) {
            Ok(direct) => {
                tracing::info!("[composio] using BYO Direct client (composio.byo_api_key set)");
                return Some(ComposioClient::direct(direct));
            }
            Err(e) => {
                tracing::warn!(
                    "[composio] failed to build BYO Direct client: {e} — falling back to proxy"
                );
            }
        }
    }
    let inner = crate::openhuman::integrations::build_client(config)?;
    Some(ComposioClient::new(inner))
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
