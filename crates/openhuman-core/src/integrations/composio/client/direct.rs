//! Direct-mode response reshapers: `direct_authorize`, `direct_execute`,
//! `direct_list_connections`, and `direct_list_tools`. Mirror the
//! backend-proxied [`super::connections::ComposioClient`] methods but call
//! Composio's v3 API directly (via a bound [`crate::tools::ComposioTool`])
//! and reshape the v3 response into the same envelope types, so downstream
//! callers in `ops.rs` / `tools.rs` don't have to branch on mode.

use std::sync::Arc;

use super::super::direct_auth;
use super::super::types::{
    ComposioAuthorizeResponse, ComposioConnection, ComposioConnectionsResponse,
    ComposioExecuteResponse, ComposioToolsResponse,
};

/// Direct-mode counterpart to [`ComposioClient::authorize`]. Calls
/// Composio v3 `/connected_accounts/link` via
/// [`crate::tools::ComposioTool::get_connection_url`] and
/// reshapes the response into the [`ComposioAuthorizeResponse`] the
/// backend-proxied path emits.
///
/// The v3 endpoint returns a redirect URL but does NOT (currently)
/// surface a stable `connection_id` in the same call — the connection
/// row is created lazily when the user completes OAuth on Composio's
/// hosted page. To preserve the response contract the frontend already
/// consumes, we emit an empty `connection_id` for now. The 5 s
/// `list_connections` poll (now live in direct mode too — see
/// [`direct_list_connections`]) is what ultimately surfaces the new
/// row to the UI.
pub(crate) async fn direct_authorize(
    direct: &Arc<crate::tools::ComposioTool>,
    toolkit: &str,
    entity_id: &str,
) -> anyhow::Result<ComposioAuthorizeResponse> {
    let toolkit = toolkit.trim();
    if toolkit.is_empty() {
        anyhow::bail!("composio direct authorize: toolkit must not be empty");
    }
    let entity_id = entity_id.trim();
    let entity_id = if entity_id.is_empty() {
        "default"
    } else {
        entity_id
    };
    tracing::debug!(
        toolkit = %toolkit,
        entity_id = %entity_id,
        "[composio-direct] authorize: requesting hosted connect URL"
    );
    let connect_url = direct
        .get_connection_url(Some(toolkit), None, entity_id)
        .await?;
    tracing::debug!(
        toolkit = %toolkit,
        url_len = connect_url.len(),
        "[composio-direct] authorize: got connect url (redacted)"
    );
    Ok(ComposioAuthorizeResponse {
        connect_url,
        // No stable connection id in the v3 link response — see fn-level
        // doc. The frontend uses `connectUrl` to open the browser and
        // `listConnections` polling to detect the resulting row.
        connection_id: String::new(),
    })
}

/// Direct-mode counterpart to [`ComposioClient::execute_tool`]. Mirrors
/// the v3 `/tools/{slug}/execute` envelope into [`ComposioExecuteResponse`]
/// so the caller doesn't branch on mode for the
/// `ComposioActionExecuted` event-bus payload or the
/// markdown-vs-JSON-body preference.
///
/// Direct mode runs without the backend's billing margin, so `cost_usd`
/// is reported as `0.0`. The backend's `markdownFormatted` field is
/// likewise specific to the backend-proxied path and remains `None` for
/// direct callers, which fall back to the raw JSON envelope.
pub async fn direct_execute(
    direct: &Arc<crate::tools::ComposioTool>,
    tool: &str,
    arguments: Option<serde_json::Value>,
    entity_id: &str,
    connection_id: Option<&str>,
) -> anyhow::Result<ComposioExecuteResponse> {
    let tool = tool.trim();
    if tool.is_empty() {
        anyhow::bail!("composio direct_execute: tool slug must not be empty");
    }
    let params = arguments.unwrap_or_else(|| serde_json::Value::Object(Default::default()));
    let entity_id = entity_id.trim();
    let entity_id_opt = (!entity_id.is_empty()).then_some(entity_id);
    let conn_id = connection_id.map(str::trim).filter(|s| !s.is_empty());
    tracing::debug!(
        tool = %tool,
        has_entity = entity_id_opt.is_some(),
        connection_id = ?conn_id,
        "[composio-direct] execute: invoking v3 /tools/{{slug}}/execute"
    );
    let raw = direct
        .execute_action(tool, params, entity_id_opt, conn_id)
        .await?;
    // v3 surfaces `successful` + `data` + `error` at the top level. If
    // none are present, treat the call as success so callers see the
    // raw payload instead of an empty error envelope.
    let successful = raw
        .get("successful")
        .and_then(serde_json::Value::as_bool)
        .or_else(|| raw.get("success").and_then(serde_json::Value::as_bool))
        .unwrap_or(true);
    let error = raw
        .get("error")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let data = raw.get("data").cloned().unwrap_or(raw);
    Ok(ComposioExecuteResponse {
        data,
        successful,
        error,
        cost_usd: 0.0,
        markdown_formatted: None,
    })
}

/// Direct-mode counterpart to [`ComposioClient::list_connections`].
///
/// Calls Composio v3 `/connected_accounts` (via
/// [`crate::tools::ComposioTool::list_connected_accounts`])
/// and maps each item to the canonical [`ComposioConnection`] so the
/// existing frontend type contract and the 5 s UI poll keep working
/// unchanged.
///
/// Toolkit slug, status, and `created_at` are extracted defensively —
/// missing or unparseable fields fall back to empty strings / `None`
/// rather than dropping the row. The status filter applied downstream
/// (`ComposioConnection::is_active`) treats empty status as inactive,
/// so a malformed row will simply not be presented as connected — the
/// fail-safe shape the user expects.
pub async fn direct_list_connections(
    direct: &Arc<crate::tools::ComposioTool>,
) -> anyhow::Result<ComposioConnectionsResponse> {
    tracing::debug!("[composio-direct] list_connections: GET v3 /connected_accounts");
    let key_id = direct.auth_key_fingerprint();
    if let Some(error) = direct_auth::direct_auth_backoff_error(key_id) {
        tracing::warn!(
            "[composio-direct] list_connections: direct API key backoff gate open; \
             skipping v3 /connected_accounts"
        );
        anyhow::bail!("{error}");
    }

    let items = match direct.list_connected_accounts().await {
        Ok(items) => {
            direct_auth::record_direct_auth_success(key_id);
            items
        }
        Err(error) => {
            let rendered = format!("{error:#}");
            match direct_auth::record_direct_auth_failure(key_id, &rendered) {
                direct_auth::DirectAuthFailureDecision::NotAuthFailure => {}
                direct_auth::DirectAuthFailureDecision::RetryAllowed { consecutive } => {
                    tracing::warn!(
                        consecutive,
                        threshold = direct_auth::DIRECT_INVALID_API_KEY_THRESHOLD,
                        "[composio-direct] list_connections: direct API key rejected"
                    );
                }
                direct_auth::DirectAuthFailureDecision::CircuitOpened { consecutive } => {
                    let backoff = direct_auth::invalid_api_key_backoff_message(consecutive);
                    tracing::warn!(
                        consecutive,
                        threshold = direct_auth::DIRECT_INVALID_API_KEY_THRESHOLD,
                        "[composio-direct] list_connections: direct API key backoff gate opened"
                    );
                    anyhow::bail!("{backoff}");
                }
            }
            return Err(error);
        }
    };
    let connections: Vec<ComposioConnection> = items
        .into_iter()
        .filter_map(|item| {
            let id = item.id.trim().to_string();
            if id.is_empty() {
                return None;
            }
            let toolkit = item.toolkit_slug().unwrap_or_default();
            let status = item.status.clone().unwrap_or_default();
            Some(ComposioConnection {
                id,
                toolkit,
                status,
                created_at: item.created_at.clone(),
                // Identity fields are populated by
                // `enrich_connections_with_identity` in ops.rs after
                // the full list is fetched, using cached profile data.
                account_email: None,
                workspace: None,
                username: None,
            })
        })
        .collect();
    tracing::debug!(
        count = connections.len(),
        "[composio-direct] list_connections: mapped v3 connected accounts"
    );
    Ok(ComposioConnectionsResponse { connections })
}

/// Direct-mode counterpart to [`ComposioClient::list_tools`]. Calls
/// Composio v3 `/tools?toolkits=<csv>&tags=<a>&tags=<b>` via
/// [`crate::tools::ComposioTool::list_tool_schemas_v3`] and
/// reshapes each item into the same [`ComposioToolSchema`] envelope the
/// backend-proxied path returns.
///
/// `toolkits` may be empty (full direct-tenant catalogue) or scoped to
/// the user's connected toolkits (preferred — keeps response size bounded
/// and skips schemas the agent can't actually call). `composio_list_tools`'s
/// direct branch passes `direct_list_connections`'s active set.
///
/// `tags` mirrors the backend path's tag filter so a self-key user's
/// `composio_list_tools(..., tags)` request narrows by Composio action tag
/// in direct mode too (previously the tag filter was silently dropped on
/// the direct branch). The caller is expected to have already applied
/// [`crate::integrations::composio::ops::should_forward_tags`] before passing `tags` here.
///
/// Schemas surfaced here are tenant-agnostic — Composio's action
/// definitions are the same across tenants, so direct-mode users get
/// the same model-callable shape backend-mode does. Downstream curated-
/// whitelist filtering (`evaluate_tool_visibility` / `find_curated`)
/// still applies at the `ops::composio_list_tools` layer.
///
/// `pub(crate)` (widened from `pub(super)`) so
/// `catalog::fetch_raw_toolkit_tools` can call this directly for
/// the LIVE (uncurated) tool-contract catalog the Workflow builder grounds
/// against — that caller deliberately bypasses `composio_list_tools`'s
/// curated-whitelist filter (`filter_list_tools_response_for_direct`),
/// which this function never applies itself; the filter is layered on by
/// its `composio_list_tools` caller, not baked in here.
pub(crate) async fn direct_list_tools(
    direct: &Arc<crate::tools::ComposioTool>,
    toolkits: &[String],
    tags: Option<&[String]>,
) -> anyhow::Result<ComposioToolsResponse> {
    let toolkit_refs: Vec<&str> = toolkits.iter().map(|s| s.as_str()).collect();
    let tag_refs: Option<Vec<&str>> = tags.map(|t| t.iter().map(|s| s.as_str()).collect());
    tracing::debug!(
        toolkits = toolkit_refs.len(),
        tags = tag_refs.as_ref().map(Vec::len).unwrap_or(0),
        "[composio-direct] list_tools: GET v3 /tools"
    );
    let items = direct
        .list_tool_schemas_v3(&toolkit_refs, tag_refs.as_deref())
        .await?;
    let tools: Vec<super::super::types::ComposioToolSchema> = items
        .into_iter()
        .filter(|item| !item.slug.is_empty())
        .map(|item| super::super::types::ComposioToolSchema {
            kind: "function".to_string(),
            function: super::super::types::ComposioToolFunction {
                name: item.slug,
                description: item.description,
                parameters: item.input_parameters,
                output_parameters: item.output_parameters,
            },
        })
        .collect();
    tracing::debug!(
        count = tools.len(),
        "[composio-direct] list_tools: mapped v3 tool schemas"
    );
    Ok(ComposioToolsResponse { tools })
}
