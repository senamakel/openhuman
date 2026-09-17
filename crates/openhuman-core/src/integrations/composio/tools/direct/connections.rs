//! OAuth connect-URL issuance and connected-account listing.

use super::http_errors::{extract_redirect_url, response_error};
use super::types::ComposioTool;
use anyhow::Context;
use serde::Deserialize;
use serde_json::json;

impl ComposioTool {
    /// Get the OAuth connection URL for a specific app/toolkit or auth config.
    ///
    /// Uses v3 endpoint first and falls back to v2 for compatibility.
    pub async fn get_connection_url(
        &self,
        app_name: Option<&str>,
        auth_config_id: Option<&str>,
        entity_id: &str,
    ) -> anyhow::Result<String> {
        let v3 = self
            .get_connection_url_v3(app_name, auth_config_id, entity_id)
            .await;
        match v3 {
            Ok(url) => Ok(url),
            Err(v3_err) => {
                let app = app_name.ok_or_else(|| {
                    anyhow::anyhow!(
                        "Composio v3 connect failed ({v3_err}) and v2 fallback requires 'app'"
                    )
                })?;
                match self.get_connection_url_v2(app, entity_id).await {
                    Ok(url) => Ok(url),
                    Err(v2_err) => anyhow::bail!(
                        "Composio connect failed on v3 ({v3_err}) and v2 fallback ({v2_err})"
                    ),
                }
            }
        }
    }

    async fn get_connection_url_v3(
        &self,
        app_name: Option<&str>,
        auth_config_id: Option<&str>,
        entity_id: &str,
    ) -> anyhow::Result<String> {
        let auth_config_id = match auth_config_id {
            Some(id) => id.to_string(),
            None => {
                let app = app_name.ok_or_else(|| {
                    anyhow::anyhow!("Missing 'app' or 'auth_config_id' for v3 connect")
                })?;
                self.resolve_auth_config_id(app).await?
            }
        };

        let url = format!("{}/connected_accounts/link", self.base_v3);
        let body = json!({
            "auth_config_id": auth_config_id,
            "user_id": entity_id,
        });

        let resp = self
            .client()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 connect failed: {err}");
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v3 connect response")?;
        extract_redirect_url(&result)
            .ok_or_else(|| anyhow::anyhow!("No redirect URL in Composio v3 response"))
    }

    async fn get_connection_url_v2(
        &self,
        app_name: &str,
        entity_id: &str,
    ) -> anyhow::Result<String> {
        let url = format!("{}/connectedAccounts", self.base_v2);

        let body = json!({
            "integrationId": app_name,
            "entityId": entity_id,
        });

        let resp = self
            .client()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v2 connect failed: {err}");
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v2 connect response")?;
        extract_redirect_url(&result)
            .ok_or_else(|| anyhow::anyhow!("No redirect URL in Composio v2 response"))
    }

    /// List the user's connected accounts on Composio v3.
    ///
    /// GET `https://backend.composio.dev/api/v3/connected_accounts` with
    /// `x-api-key: <user_key>`. Returns the raw item list; reshaping
    /// into [`super::super::super::composio::types::ComposioConnection`]
    /// happens at the call site in `composio/client.rs::direct_list_connections`.
    ///
    /// The v3 envelope is `{ items: [{ id, status, toolkit, created_at, ... }] }`.
    /// Toolkit may arrive as either a plain string slug or as a nested
    /// object — we tolerate both via [`ComposioConnectedAccount::toolkit_slug`].
    /// This matches the same upstream shape drift handled by
    /// `de_string_or_object` in `composio/types.rs`.
    pub async fn list_connected_accounts(&self) -> anyhow::Result<Vec<ComposioConnectedAccount>> {
        let url = format!("{}/connected_accounts", self.base_v3);
        self.ensure_request_url(&url)?;

        let resp = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            // Composio paginates; pull a generous page size so most
            // users see their full list in one round trip. If a user has
            // > 200 connected accounts (extremely rare for an individual
            // tenant) the rest will be missing until we add explicit
            // pagination — note for the follow-up.
            .query(&[("limit", "200")])
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 connected_accounts failed: {err}");
        }

        let mut body: ComposioConnectedAccountsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 connected_accounts response")?;
        // Drop rows with a blank id — serde_default means id can be ""
        // if the upstream response is malformed. An empty connectionId
        // propagated downstream causes invalid v3 API calls.
        body.items.retain(|item| !item.id.trim().is_empty());
        tracing::debug!(
            count = body.items.len(),
            "[composio-direct] list_connected_accounts: fetched connected accounts"
        );
        Ok(body.items)
    }

    async fn resolve_auth_config_id(&self, app_name: &str) -> anyhow::Result<String> {
        let url = format!("{}/auth_configs", self.base_v3);

        let resp = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            .query(&[
                ("toolkit_slug", app_name),
                ("show_disabled", "true"),
                ("limit", "25"),
            ])
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 auth config lookup failed: {err}");
        }

        let body: ComposioAuthConfigsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 auth configs response")?;

        if body.items.is_empty() {
            anyhow::bail!(
                "No auth config found for toolkit '{app_name}'. Create one in Composio first."
            );
        }

        let preferred = body
            .items
            .iter()
            .find(|cfg| cfg.is_enabled())
            .or_else(|| body.items.first())
            .context("No usable auth config returned by Composio")?;

        Ok(preferred.id.clone())
    }
}

#[derive(Debug, Deserialize)]
struct ComposioAuthConfigsResponse {
    #[serde(default)]
    items: Vec<ComposioAuthConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ComposioAuthConfig {
    pub(super) id: String,
    #[serde(default)]
    pub(super) status: Option<String>,
    #[serde(default)]
    pub(super) enabled: Option<bool>,
}

impl ComposioAuthConfig {
    pub(super) fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(false)
            || self
                .status
                .as_deref()
                .is_some_and(|v| v.eq_ignore_ascii_case("enabled"))
    }
}

// ── v3 /connected_accounts envelope ─────────────────────────────────
//
// Public so the `composio/client.rs::direct_list_connections` helper
// in the domain layer can reshape it into the canonical
// `ComposioConnection` type. Kept distinct from `ComposioConnection`
// itself (which is the backend-proxied envelope) so the two paths
// don't get coupled — Composio v3 may add or rename fields and we'd
// rather adjust the mapping than reshuffle the public type.

#[derive(Debug, Deserialize)]
struct ComposioConnectedAccountsResponse {
    #[serde(default)]
    items: Vec<ComposioConnectedAccount>,
}

/// One v3 connected-account row.
///
/// Field shapes follow Composio's v3 docs as of May 2026. `toolkit` may
/// be either a string slug (older payloads) or a nested object with a
/// `slug` field (newer payloads); [`Self::toolkit_slug`] extracts the
/// canonical slug from either shape.
#[derive(Debug, Clone, Deserialize)]
pub struct ComposioConnectedAccount {
    #[serde(default)]
    pub id: String,
    /// `"ACTIVE"`, `"INITIATED"`, `"FAILED"`, … — passed through as-is
    /// so the caller's status filter (`ComposioConnection::is_active`)
    /// applies uniformly across both backend-proxied and direct paths.
    #[serde(default)]
    pub status: Option<String>,
    /// Composio uses `created_at` (snake_case) at v3. We keep both
    /// spellings to tolerate any upstream drift back to `createdAt`.
    #[serde(default, alias = "createdAt")]
    pub created_at: Option<String>,
    /// Toolkit may be a plain string slug or a nested
    /// `ComposioToolkitRef`. Extracted via [`Self::toolkit_slug`].
    #[serde(default)]
    pub(super) toolkit: Option<serde_json::Value>,
    /// Older payload shape — a top-level `app_name` string. Used as
    /// a fallback when `toolkit` is absent or unparseable.
    #[serde(default, rename = "appName", alias = "app_name")]
    pub(super) app_name: Option<String>,
}

impl ComposioConnectedAccount {
    /// Best-effort extract of the toolkit slug from the
    /// possibly-polymorphic `toolkit` field, falling back to
    /// `app_name`. Returns `None` only when no recognizable slug
    /// representation is present.
    pub fn toolkit_slug(&self) -> Option<String> {
        if let Some(value) = &self.toolkit {
            match value {
                serde_json::Value::String(s) => {
                    let t = s.trim();
                    if !t.is_empty() {
                        return Some(t.to_string());
                    }
                }
                serde_json::Value::Object(map) => {
                    for key in ["slug", "id", "name", "key"] {
                        if let Some(serde_json::Value::String(s)) = map.get(key) {
                            let t = s.trim();
                            if !t.is_empty() {
                                return Some(t.to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        self.app_name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }
}
