//! [`ComposioClient`]'s core lifecycle plus its toolkit/connection/OAuth
//! surface — construction, `list_toolkits`, `list_connections`, `authorize`
//! (including the Gmail OAuth-scope merge helpers it needs), and
//! `delete_connection` (via the shared `raw_delete` HTTP helper).

use std::sync::Arc;

use anyhow::Result;
use serde_json::{json, Value};

use crate::integrations::IntegrationClient;

use super::super::types::{
    ComposioAuthorizeResponse, ComposioConnectionsResponse, ComposioDeleteResponse,
    ComposioToolkitsResponse,
};

const AUTHORIZE_OAUTH_SCOPES_FIELD: &str = "oauth_scopes";
const GMAIL_REQUIRED_OAUTH_SCOPES: &[&str] = &["https://www.googleapis.com/auth/gmail.readonly"];

/// High-level client for all backend-proxied Composio operations.
#[derive(Clone)]
pub struct ComposioClient {
    pub(super) inner: Arc<IntegrationClient>,
}

impl ComposioClient {
    pub fn new(inner: Arc<IntegrationClient>) -> Self {
        Self { inner }
    }

    /// Access the underlying integration client (useful for tests or for
    /// callers that need to reuse the same reqwest pool for bespoke calls).
    pub fn inner(&self) -> &Arc<IntegrationClient> {
        &self.inner
    }

    // ── Toolkits ────────────────────────────────────────────────────

    /// `GET /agent-integrations/composio/toolkits` — server-enforced
    /// allowlist of toolkits that composio calls may target.
    pub async fn list_toolkits(&self) -> Result<ComposioToolkitsResponse> {
        tracing::debug!("[composio] list_toolkits");
        self.inner
            .get::<ComposioToolkitsResponse>("/agent-integrations/composio/toolkits")
            .await
    }

    // ── Connections ─────────────────────────────────────────────────

    /// `GET /agent-integrations/composio/connections` — active connected
    /// accounts for the authenticated user, filtered to the allowlist.
    pub async fn list_connections(&self) -> Result<ComposioConnectionsResponse> {
        tracing::debug!("[composio] list_connections");
        self.inner
            .get::<ComposioConnectionsResponse>("/agent-integrations/composio/connections")
            .await
    }

    /// `POST /agent-integrations/composio/authorize` — begin an OAuth
    /// handoff for `toolkit` and return the hosted `connectUrl` the user
    /// must open in a browser.
    ///
    /// `extra_params` is an optional JSON object whose key/value pairs are
    /// merged into the request body. Some toolkits (e.g. `whatsapp`) require
    /// additional fields (e.g. `waba_id`) that Composio will reject the
    /// authorization without.
    pub async fn authorize(
        &self,
        toolkit: &str,
        extra_params: Option<serde_json::Value>,
    ) -> Result<ComposioAuthorizeResponse> {
        let toolkit = toolkit.trim();
        if toolkit.is_empty() {
            anyhow::bail!("composio.authorize: toolkit must not be empty");
        }
        tracing::debug!(toolkit = %toolkit, has_extra_params = extra_params.is_some(), "[composio] authorize");
        let mut body = serde_json::json!({ "toolkit": toolkit });
        if let Some(extra) = extra_params {
            const RESERVED: &[&str] = &["toolkit", "toolkit_version", "auth", "client_id"];
            let extra_obj = extra.as_object().ok_or_else(|| {
                anyhow::anyhow!("composio.authorize: extra_params must be a JSON object")
            })?;
            let obj = body.as_object_mut().ok_or_else(|| {
                anyhow::anyhow!("composio.authorize: internal payload must be an object")
            })?;
            for (k, v) in extra_obj {
                if RESERVED.contains(&k.as_str()) {
                    anyhow::bail!(
                        "composio.authorize: extra_params cannot override reserved key '{k}'"
                    );
                }
                obj.insert(k.clone(), v.clone());
            }
        }
        merge_required_oauth_scopes(&mut body, toolkit)?;
        self.inner
            .post::<ComposioAuthorizeResponse>("/agent-integrations/composio/authorize", &body)
            .await
    }

    /// `DELETE /agent-integrations/composio/connections/{id}`.
    ///
    /// The backend verifies that the caller owns the connection before
    /// deleting it. We call this via `POST` with a synthetic `_method`
    /// body because [`IntegrationClient`] does not currently expose a
    /// generic `delete()` — the backend accepts the method override.
    pub async fn delete_connection(&self, connection_id: &str) -> Result<ComposioDeleteResponse> {
        let connection_id = connection_id.trim();
        if connection_id.is_empty() {
            anyhow::bail!("composio.delete_connection: connectionId must not be empty");
        }
        tracing::debug!(connection_id = %connection_id, "[composio] delete_connection");
        // Fall through to the reusable raw HTTP delete helper below.
        self.raw_delete::<ComposioDeleteResponse>(&format!(
            "/agent-integrations/composio/connections/{connection_id}"
        ))
        .await
    }

    // ── Raw DELETE ──────────────────────────────────────────────────

    /// Perform an HTTP DELETE and parse the standard backend envelope.
    ///
    /// [`IntegrationClient`] only exposes `get` / `post` today, and the
    /// composio route actually requires a DELETE. We re-implement the
    /// envelope handling here so we don't have to widen the shared
    /// client's public surface just for one caller.
    pub(super) async fn raw_delete<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        #[derive(serde::Deserialize)]
        struct Envelope<T> {
            #[serde(default)]
            success: bool,
            data: Option<T>,
            #[serde(default)]
            error: Option<String>,
        }

        let url = crate::api::config::api_url(&self.inner.backend_url, path);
        tracing::debug!("[composio] DELETE {}", url);

        // Build a fresh lightweight reqwest client for this DELETE.
        // Note: this allocates a *new* connection pool — it does NOT
        // reuse the pool inside `self.inner`. To reuse the shared pool
        // we'd need to clone or expose the existing `reqwest::Client`
        // from `IntegrationClient`, which we intentionally avoid so the
        // public surface of that type doesn't widen for one caller.
        //
        // Mirror the TLS settings of the shared client so this path has the
        // same connection behaviour as the other backend calls.
        // Platform-appropriate TLS backend — see [`crate::util::tls`].
        let http_client = crate::util::tls::tls_client_builder()
            .http1_only()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()?;

        let resp = http_client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.inner.auth_token))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            let detail = crate::integrations::client::extract_error_detail(
                &body_text,
                crate::integrations::client::MAX_ERROR_BODY_LEN,
            );
            // Use the same UTF-8-safe truncation for the debug-log preview
            // — direct byte-slicing (`&body_text[..len.min(300)]`) panics
            // when the cutoff lands inside a multibyte codepoint.
            let logged_body = crate::integrations::client::extract_error_detail(&body_text, 300);
            tracing::debug!(
                "[composio] DELETE {} → {} body={}",
                url,
                status,
                logged_body
            );
            let status_str = status.as_u16().to_string();
            // Mirrors the integrations post()/get() sites — see
            // OPENHUMAN-TAURI-BC. 4xx user-input / auth-state shapes
            // demote via the observability classifier; 5xx and
            // non-transient 4xx still surface as actionable events.
            crate::core::observability::report_error_or_expected(
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
            // Mirrors the integrations envelope-error sites — route through
            // the observability classifier so user-state envelope failures
            // (composio "Toolkit X is not enabled" / "Trigger type …
            // not found" / "Missing required fields: …" — OPENHUMAN-TAURI-3R
            // / -3S / -34 / -97) demote to a breadcrumb instead of firing
            // a Sentry event. Genuine backend bugs still surface.
            crate::core::observability::report_error_or_expected(
                msg.as_str(),
                "composio",
                "delete",
                &[("path", path), ("failure", "envelope_error")],
            );
            anyhow::bail!("Backend error for DELETE {}: {}", url, msg);
        }
        envelope.data.ok_or_else(|| {
            anyhow::anyhow!("Backend returned success but no data for DELETE {}", url)
        })
    }
}

fn required_oauth_scopes_for_toolkit(toolkit: &str) -> &'static [&'static str] {
    match toolkit.trim().to_ascii_lowercase().as_str() {
        // GMAIL_NEW_GMAIL_MESSAGE and the native Gmail sync path need read access
        // to messages. Without this hint fresh OAuth handoffs can complete with a
        // profile-only Google token and trigger enable fails with 403 insufficient
        // authentication scopes (#2186).
        "gmail" => GMAIL_REQUIRED_OAUTH_SCOPES,
        _ => &[],
    }
}

fn merge_required_oauth_scopes(body: &mut Value, toolkit: &str) -> anyhow::Result<()> {
    let required = required_oauth_scopes_for_toolkit(toolkit);
    if required.is_empty() {
        return Ok(());
    }

    let obj = body
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("composio.authorize: internal payload must be an object"))?;
    match obj.get_mut(AUTHORIZE_OAUTH_SCOPES_FIELD) {
        Some(existing) => append_missing_oauth_scopes(existing, required)?,
        None => {
            obj.insert(AUTHORIZE_OAUTH_SCOPES_FIELD.to_string(), json!(required));
        }
    }
    Ok(())
}

fn append_missing_oauth_scopes(value: &mut Value, required: &[&str]) -> anyhow::Result<()> {
    let mut scopes = match value {
        Value::Null => Vec::new(),
        Value::String(raw) => raw
            .split(|ch: char| ch == ',' || ch.is_whitespace())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .collect(),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len() + required.len());
            for item in items {
                let Some(scope) = item.as_str() else {
                    anyhow::bail!(
                        "composio.authorize: {AUTHORIZE_OAUTH_SCOPES_FIELD} entries must be strings"
                    );
                };
                let scope = scope.trim();
                if !scope.is_empty() {
                    out.push(scope.to_string());
                }
            }
            out
        }
        _ => {
            anyhow::bail!(
                "composio.authorize: {AUTHORIZE_OAUTH_SCOPES_FIELD} must be a string or array"
            );
        }
    };

    for scope in required {
        if !scopes.iter().any(|existing| existing == scope) {
            scopes.push((*scope).to_string());
        }
    }
    *value = json!(scopes);
    Ok(())
}
