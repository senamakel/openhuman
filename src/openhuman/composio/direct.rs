//! Direct Composio REST API client used in BYO (Bring Your Own
//! API key) mode.
//!
//! When `config.composio.byo_api_key` is set, the core stops talking to
//! the openhuman backend's `/agent-integrations/composio/*` proxy and
//! instead hits Composio's public REST API at
//! `https://backend.composio.dev/api/v3/*` directly using the user's
//! API key (`X-API-Key` header).
//!
//! ## What is supported
//!
//! - **Toolkits / tools / connections / execute** — read-only catalog
//!   and tool execution work end-to-end with the user's own key.
//!
//! ## What is NOT supported in BYO mode
//!
//! - **Triggers** — Composio delivers triggers via HMAC-verified
//!   webhooks that must terminate at a publicly reachable URL. The
//!   openhuman backend owns that endpoint and fans events out over
//!   Socket.IO to user sessions. A desktop app cannot receive inbound
//!   HTTP, so trigger creation / listing / disable all return
//!   [`ByoUnsupportedError`].
//! - **OAuth `authorize` handoff** — Composio's hosted OAuth needs an
//!   `auth_config_id` pre-provisioned in the user's Composio dashboard
//!   plus a redirect URL the user controls. BYO users connect accounts
//!   in their Composio dashboard; the core only executes against
//!   already-connected accounts.
//! - **GitHub repo listing** — currently a custom helper on the
//!   openhuman backend (not a 1:1 Composio API). Returns
//!   [`ByoUnsupportedError`] in BYO mode.
//!
//! All BYO-unsupported paths return errors whose `Display` starts with
//! the stable `composio_byo_unsupported:` prefix so RPC handlers and
//! UI surfaces can match on it without parsing free-form text.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde_json::Value;

use super::types::{
    ComposioActiveTriggersResponse, ComposioConnection, ComposioConnectionsResponse,
    ComposioExecuteResponse, ComposioToolFunction, ComposioToolSchema, ComposioToolkitsResponse,
    ComposioToolsResponse,
};

/// Stable error prefix returned for any operation the BYO client
/// cannot service. Match on this in RPC error mapping so the UI can
/// show a uniform "switch off BYO to use this" banner.
pub const BYO_UNSUPPORTED_PREFIX: &str = "composio_byo_unsupported:";

/// Construct an `anyhow::Error` for a BYO-unsupported operation.
pub fn byo_unsupported(op: &str) -> anyhow::Error {
    anyhow!(
        "{prefix} operation `{op}` is not available with a user-provided Composio API key. \
         Disable BYO mode in Settings → Composio to use the hosted backend for this feature.",
        prefix = BYO_UNSUPPORTED_PREFIX,
        op = op,
    )
}

/// True if `err` is a BYO-unsupported sentinel error.
pub fn is_byo_unsupported(err: &anyhow::Error) -> bool {
    err.to_string().starts_with(BYO_UNSUPPORTED_PREFIX)
}

/// Default base URL for Composio's public REST API.
pub const DEFAULT_COMPOSIO_BASE_URL: &str = "https://backend.composio.dev";

/// Direct-mode Composio client. Cheap to clone — the inner reqwest
/// `Client` already shares its connection pool through `Arc`.
#[derive(Clone)]
pub struct DirectComposioClient {
    inner: Arc<DirectInner>,
}

struct DirectInner {
    base_url: String,
    api_key: String,
    entity_id: String,
    http: Client,
}

impl DirectComposioClient {
    /// Build a new direct client. `entity_id` is forwarded as the
    /// Composio `user_id` field — defaults to `"default"` per
    /// [`crate::openhuman::config::schema::ComposioConfig`].
    pub fn new(api_key: String, entity_id: String) -> Result<Self> {
        Self::with_base_url(api_key, entity_id, DEFAULT_COMPOSIO_BASE_URL.to_string())
    }

    /// Like [`new`] but accepts a custom base URL — used by tests that
    /// stand up a mock HTTP server.
    pub fn with_base_url(api_key: String, entity_id: String, base_url: String) -> Result<Self> {
        let api_key = api_key.trim().to_string();
        if api_key.is_empty() {
            anyhow::bail!("composio.direct: api_key must not be empty");
        }
        let entity_id = {
            let trimmed = entity_id.trim();
            if trimmed.is_empty() {
                "default".to_string()
            } else {
                trimmed.to_string()
            }
        };
        let http = Client::builder()
            .use_rustls_tls()
            .http1_only()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self {
            inner: Arc::new(DirectInner {
                base_url: base_url.trim_end_matches('/').to_string(),
                api_key,
                entity_id,
                http,
            }),
        })
    }

    /// Entity id used for `user_id` query/body fields.
    pub fn entity_id(&self) -> &str {
        &self.inner.entity_id
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.inner.base_url, path)
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let url = self.url(path);
        tracing::debug!(url = %url, "[composio:direct] GET");
        let resp = self
            .inner
            .http
            .get(&url)
            .header("X-API-Key", &self.inner.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Composio direct GET {url} returned {status}: {detail}",
                url = url,
                status = status,
                detail = truncate_for_log(&body, 400)
            );
        }
        serde_json::from_str(&body)
            .map_err(|e| anyhow!("composio.direct GET {url}: invalid JSON: {e}"))
    }

    async fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let url = self.url(path);
        tracing::debug!(url = %url, "[composio:direct] POST");
        let resp = self
            .inner
            .http
            .post(&url)
            .header("X-API-Key", &self.inner.api_key)
            .header("Accept", "application/json")
            .json(body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Composio direct POST {url} returned {status}: {detail}",
                url = url,
                status = status,
                detail = truncate_for_log(&text, 400)
            );
        }
        serde_json::from_str(&text)
            .map_err(|e| anyhow!("composio.direct POST {url}: invalid JSON: {e}"))
    }

    async fn delete(&self, path: &str) -> Result<()> {
        let url = self.url(path);
        tracing::debug!(url = %url, "[composio:direct] DELETE");
        let resp = self
            .inner
            .http
            .delete(&url)
            .header("X-API-Key", &self.inner.api_key)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Composio direct DELETE {url} returned {status}: {detail}",
                url = url,
                status = status,
                detail = truncate_for_log(&body, 400)
            );
        }
        Ok(())
    }

    // ── Toolkits ────────────────────────────────────────────────────

    /// `GET /api/v3/toolkits` — list all toolkit slugs available to the
    /// user. In BYO mode there is no openhuman-side allowlist; the
    /// caller gets Composio's full catalog.
    pub async fn list_toolkits(&self) -> Result<ComposioToolkitsResponse> {
        let v = self.get_json("/api/v3/toolkits").await?;
        let items = extract_items(&v);
        let mut slugs = Vec::with_capacity(items.len());
        for item in items {
            if let Some(slug) = item
                .get("slug")
                .or_else(|| item.get("name"))
                .or_else(|| item.get("id"))
                .and_then(Value::as_str)
            {
                slugs.push(slug.to_string());
            }
        }
        Ok(ComposioToolkitsResponse { toolkits: slugs })
    }

    // ── Connections ─────────────────────────────────────────────────

    /// `GET /api/v3/connected_accounts?user_ids=<entity>` — list the
    /// connected accounts the BYO key has access to for this entity.
    pub async fn list_connections(&self) -> Result<ComposioConnectionsResponse> {
        let path = format!(
            "/api/v3/connected_accounts?user_ids={}",
            urlencoding::encode(&self.inner.entity_id)
        );
        let v = self.get_json(&path).await?;
        let items = extract_items(&v);
        let mut connections = Vec::with_capacity(items.len());
        for item in items {
            let id = item.get("id").and_then(Value::as_str).map(str::to_string);
            let toolkit = item
                .get("toolkit")
                .and_then(|t| {
                    t.as_str()
                        .map(str::to_string)
                        .or_else(|| t.get("slug").and_then(Value::as_str).map(str::to_string))
                })
                .or_else(|| {
                    item.get("toolkit_slug")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            let status = item
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| "UNKNOWN".to_string());
            let created_at = item
                .get("created_at")
                .or_else(|| item.get("createdAt"))
                .and_then(Value::as_str)
                .map(str::to_string);
            if let (Some(id), Some(toolkit)) = (id, toolkit) {
                connections.push(ComposioConnection {
                    id,
                    toolkit,
                    status,
                    created_at,
                });
            }
        }
        Ok(ComposioConnectionsResponse { connections })
    }

    /// `DELETE /api/v3/connected_accounts/{id}`.
    pub async fn delete_connection(
        &self,
        connection_id: &str,
    ) -> Result<super::types::ComposioDeleteResponse> {
        let path = format!(
            "/api/v3/connected_accounts/{}",
            urlencoding::encode(connection_id)
        );
        self.delete(&path).await?;
        Ok(super::types::ComposioDeleteResponse { deleted: true })
    }

    // ── Tools ───────────────────────────────────────────────────────

    /// `GET /api/v3/tools` (optionally filtered by toolkit slugs).
    pub async fn list_tools(&self, toolkits: Option<&[String]>) -> Result<ComposioToolsResponse> {
        let path = match toolkits {
            Some(list) if !list.is_empty() => {
                let joined = list
                    .iter()
                    .map(|t| t.trim())
                    .filter(|t| !t.is_empty())
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "/api/v3/tools?toolkit_slugs={}",
                    urlencoding::encode(&joined)
                )
            }
            _ => "/api/v3/tools".to_string(),
        };
        let v = self.get_json(&path).await?;
        let items = extract_items(&v);
        let mut tools = Vec::with_capacity(items.len());
        for item in items {
            let Some(name) = item
                .get("slug")
                .or_else(|| item.get("name"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let description = item
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string);
            let parameters = item
                .get("input_parameters")
                .or_else(|| item.get("parameters"))
                .cloned();
            tools.push(ComposioToolSchema {
                kind: "function".to_string(),
                function: ComposioToolFunction {
                    name: name.to_string(),
                    description,
                    parameters,
                },
            });
        }
        Ok(ComposioToolsResponse { tools })
    }

    // ── Execute ─────────────────────────────────────────────────────

    /// `POST /api/v3/tools/execute/{slug}` — run a Composio action with
    /// the user's BYO key. The Composio API returns
    /// `{ data, successful, error, ... }` directly; this method maps it
    /// onto the same [`ComposioExecuteResponse`] shape the proxy
    /// returns so callers stay agnostic.
    pub async fn execute_tool(
        &self,
        tool: &str,
        arguments: Option<Value>,
    ) -> Result<ComposioExecuteResponse> {
        let path = format!("/api/v3/tools/execute/{}", urlencoding::encode(tool));
        let body = serde_json::json!({
            "user_id": self.inner.entity_id,
            "arguments": arguments.unwrap_or_else(|| Value::Object(Default::default())),
        });
        let v = self.post_json(&path, &body).await?;
        let data = v.get("data").cloned().unwrap_or(Value::Null);
        let successful = v
            .get("successful")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let error = v.get("error").and_then(Value::as_str).map(str::to_string);
        Ok(ComposioExecuteResponse {
            data,
            successful,
            error,
            cost_usd: 0.0,
            markdown_formatted: None,
        })
    }

    // ── BYO-unsupported surfaces ────────────────────────────────────
    //
    // These mirror the proxy methods so callers can branch on the
    // backend variant inside `ComposioClient` and surface a uniform
    // error. Keeping the methods here (rather than only in
    // `ComposioClient`) keeps the BYO contract co-located with the
    // direct client.

    pub async fn authorize(&self) -> Result<super::types::ComposioAuthorizeResponse> {
        Err(byo_unsupported("authorize"))
    }

    pub async fn list_github_repos(&self) -> Result<super::types::ComposioGithubReposResponse> {
        Err(byo_unsupported("github.list_repos"))
    }

    pub async fn create_trigger(&self) -> Result<super::types::ComposioCreateTriggerResponse> {
        Err(byo_unsupported("triggers.create"))
    }

    pub async fn list_available_triggers(
        &self,
    ) -> Result<super::types::ComposioAvailableTriggersResponse> {
        Err(byo_unsupported("triggers.list_available"))
    }

    pub async fn list_active_triggers(&self) -> Result<ComposioActiveTriggersResponse> {
        // Return an empty list rather than an error here: many UI
        // surfaces call `list_active_triggers` just to render an
        // "enabled triggers" section. In BYO mode the answer is
        // legitimately "none" — the user can't enable any. The Settings
        // panel separately surfaces the unsupported-status banner.
        Ok(ComposioActiveTriggersResponse {
            triggers: Vec::new(),
        })
    }

    pub async fn enable_trigger(&self) -> Result<super::types::ComposioEnableTriggerResponse> {
        Err(byo_unsupported("triggers.enable"))
    }

    pub async fn disable_trigger(&self) -> Result<super::types::ComposioDisableTriggerResponse> {
        Err(byo_unsupported("triggers.disable"))
    }
}

/// Composio v3 list endpoints wrap results in `{ "items": [...] }`.
/// Some older shapes return a bare array. Tolerate both.
fn extract_items(v: &Value) -> Vec<&Value> {
    if let Some(arr) = v.get("items").and_then(Value::as_array) {
        return arr.iter().collect();
    }
    if let Some(arr) = v.as_array() {
        return arr.iter().collect();
    }
    Vec::new()
}

fn truncate_for_log(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::{delete as axum_delete, get, post},
        Json, Router,
    };
    use serde_json::json;

    async fn start_mock(app: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    fn client(base: String) -> DirectComposioClient {
        DirectComposioClient::with_base_url("k".into(), "default".into(), base).unwrap()
    }

    #[test]
    fn empty_api_key_rejected() {
        let res = DirectComposioClient::new("  ".into(), "default".into());
        let err = match res {
            Ok(_) => panic!("expected error for empty api_key"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("api_key must not be empty"));
    }

    #[test]
    fn byo_unsupported_prefix_stable() {
        let err = byo_unsupported("triggers.create");
        assert!(err.to_string().starts_with(BYO_UNSUPPORTED_PREFIX));
        assert!(is_byo_unsupported(&err));
    }

    #[tokio::test]
    async fn list_toolkits_parses_items_envelope() {
        let app = Router::new().route(
            "/api/v3/toolkits",
            get(|| async {
                Json(json!({ "items": [
                    { "slug": "gmail" },
                    { "slug": "notion" }
                ]}))
            }),
        );
        let base = start_mock(app).await;
        let c = client(base);
        let res = c.list_toolkits().await.unwrap();
        assert_eq!(res.toolkits, vec!["gmail", "notion"]);
    }

    #[tokio::test]
    async fn list_connections_maps_v3_fields() {
        let app = Router::new().route(
            "/api/v3/connected_accounts",
            get(|| async {
                Json(json!({ "items": [
                    {
                        "id": "conn_1",
                        "toolkit": { "slug": "gmail" },
                        "status": "ACTIVE",
                        "created_at": "2026-01-01T00:00:00Z"
                    }
                ]}))
            }),
        );
        let base = start_mock(app).await;
        let c = client(base);
        let res = c.list_connections().await.unwrap();
        assert_eq!(res.connections.len(), 1);
        assert_eq!(res.connections[0].toolkit, "gmail");
        assert!(res.connections[0].is_active());
    }

    #[tokio::test]
    async fn execute_tool_returns_composio_envelope() {
        let app = Router::new().route(
            "/api/v3/tools/execute/GMAIL_SEND_EMAIL",
            post(|| async {
                Json(json!({
                    "data": { "messageId": "abc" },
                    "successful": true,
                    "error": null
                }))
            }),
        );
        let base = start_mock(app).await;
        let c = client(base);
        let res = c
            .execute_tool("GMAIL_SEND_EMAIL", Some(json!({ "to": "x@y" })))
            .await
            .unwrap();
        assert!(res.successful);
        assert_eq!(res.data["messageId"], "abc");
    }

    #[tokio::test]
    async fn delete_connection_calls_v3() {
        let app = Router::new().route(
            "/api/v3/connected_accounts/conn_1",
            axum_delete(|| async { axum::http::StatusCode::NO_CONTENT }),
        );
        let base = start_mock(app).await;
        let c = client(base);
        let res = c.delete_connection("conn_1").await.unwrap();
        assert!(res.deleted);
    }

    #[tokio::test]
    async fn authorize_is_byo_unsupported() {
        let c = DirectComposioClient::new("k".into(), "default".into()).unwrap();
        let err = c.authorize().await.unwrap_err();
        assert!(is_byo_unsupported(&err));
    }
}
