//! Action/tool listing: `list_actions` (legacy flattened shape, v3-with-v2-fallback)
//! and `list_tool_schemas_v3` (full v3 schema, used by direct-mode
//! `composio_list_tools`), plus the wire types they decode into.

use super::http_errors::response_error;
use super::types::ComposioTool;
use anyhow::Context;
use serde::{Deserialize, Serialize};

impl ComposioTool {
    /// List available Composio apps/actions for the authenticated user.
    ///
    /// Uses v3 endpoint first and falls back to v2 for compatibility.
    pub async fn list_actions(
        &self,
        app_name: Option<&str>,
    ) -> anyhow::Result<Vec<ComposioAction>> {
        match self.list_actions_v3(app_name).await {
            Ok(items) => Ok(items),
            Err(v3_err) => {
                let v2 = self.list_actions_v2(app_name).await;
                match v2 {
                    Ok(items) => Ok(items),
                    Err(v2_err) => anyhow::bail!(
                        "Composio action listing failed on v3 ({v3_err}) and v2 fallback ({v2_err})"
                    ),
                }
            }
        }
    }

    async fn list_actions_v3(&self, app_name: Option<&str>) -> anyhow::Result<Vec<ComposioAction>> {
        let url = format!("{}/tools", self.base_v3);
        let mut req = self.client().get(&url).header("x-api-key", &self.api_key);

        // #3932: pin toolkit_versions=latest. Composio v3 otherwise defaults to
        // the 00000000_00 snapshot, which lists zero tools for any toolkit
        // published after it (Outlook and every other post-launch toolkit).
        req = req.query(&[("limit", "200"), ("toolkit_versions", "latest")]);
        if let Some(app) = app_name.map(str::trim).filter(|app| !app.is_empty()) {
            req = req.query(&[("toolkits", app), ("toolkit_slug", app)]);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 API error: {err}");
        }

        let body: ComposioToolsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 tools response")?;
        Ok(map_v3_tools_to_actions(body.items))
    }

    async fn list_actions_v2(&self, app_name: Option<&str>) -> anyhow::Result<Vec<ComposioAction>> {
        let mut url = format!("{}/actions", self.base_v2);
        if let Some(app) = app_name {
            url = format!("{url}?appNames={app}");
        }

        let resp = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v2 API error: {err}");
        }

        let body: ComposioActionsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v2 actions response")?;
        Ok(body.items)
    }

    /// Build the query-parameter pairs for the Composio v3 `GET /tools`
    /// listing used by [`Self::list_tool_schemas_v3`].
    ///
    /// `toolkits` is sent as a single comma-joined `toolkits=` param (the
    /// legacy plural the v3 backend tolerates; cf. `list_actions_v3` which
    /// sends both the plural and `toolkit_slug` singular forms). `tags` is
    /// encoded as **repeated** `tags=` params (`tags=a&tags=b`) — the shape
    /// Composio v3 `/tools` documents for tag filtering ("can be specified
    /// multiple times"), NOT the comma-joined form the backend proxy uses.
    /// Blank entries are trimmed and dropped; an empty `tags` slice yields
    /// no `tags` params (treated as no filter).
    ///
    /// Pure (no I/O) so the param shape is unit-testable without a live
    /// HTTP round trip — mirrors [`Self::build_execute_action_v3_request`].
    pub(super) fn build_list_tool_schemas_v3_query(
        toolkits: &[&str],
        tags: Option<&[&str]>,
    ) -> Vec<(&'static str, String)> {
        // #3932: pin toolkit_versions=latest. Without it Composio v3 defaults to
        // the 00000000_00 snapshot, which lists zero tools for any toolkit
        // published after it (Outlook and every other post-launch toolkit).
        let mut params: Vec<(&'static str, String)> = vec![
            ("limit", "200".to_string()),
            ("toolkit_versions", "latest".to_string()),
        ];

        let trimmed: Vec<&str> = toolkits
            .iter()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
            .collect();
        if !trimmed.is_empty() {
            params.push(("toolkits", trimmed.join(",")));
        }

        if let Some(tags) = tags {
            for tag in tags.iter().map(|t| t.trim()).filter(|t| !t.is_empty()) {
                params.push(("tags", tag.to_string()));
            }
        }

        params
    }

    /// List v3 tool definitions for one or more toolkits, preserving the
    /// raw `input_parameters` JSON schema each action carries.
    ///
    /// Sibling of [`Self::list_actions`] but kept distinct because
    /// `list_actions` flattens to `Vec<ComposioAction>` (no parameters)
    /// for the legacy agent-discovery shape, whereas
    /// `composio_list_tools`'s direct-mode branch needs the full schema
    /// so the LLM agent can supply valid arguments without a separate
    /// round trip.
    ///
    /// `toolkits` may contain one or many slugs; when non-empty they are
    /// sent as a comma-separated `toolkits=` filter to constrain the v3
    /// catalogue scan. Empty filter returns every action across every
    /// toolkit on the user's tenant (potentially large; callers should
    /// pass a non-empty filter in practice).
    ///
    /// `tags` narrows the result by Composio action tag (OR semantics —
    /// multiple tags broaden the result). This is the direct-mode (BYO
    /// key) counterpart to the backend proxy's `tags` query param wired
    /// in [`crate::integrations::composio::client::ComposioClient::list_tools`];
    /// without it a self-key user's `composio_list_tools(..., tags)`
    /// request would silently drop the tag filter. Blank/empty `tags`
    /// are treated as no filter.
    pub(crate) async fn list_tool_schemas_v3(
        &self,
        toolkits: &[&str],
        tags: Option<&[&str]>,
    ) -> anyhow::Result<Vec<ComposioToolSchemaV3>> {
        let url = format!("{}/tools", self.base_v3);
        let params = Self::build_list_tool_schemas_v3_query(toolkits, tags);
        tracing::debug!(
            toolkits = toolkits.len(),
            tags = tags.map(<[&str]>::len).unwrap_or(0),
            "[composio-direct] list_tool_schemas_v3: GET v3 /tools query built"
        );
        let req = self
            .client()
            .get(&url)
            .header("x-api-key", &self.api_key)
            .query(&params);

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 list_tool_schemas: {err}");
        }

        let body: ComposioToolsResponse = resp
            .json()
            .await
            .context("Failed to decode Composio v3 tools response")?;
        Ok(body
            .items
            .into_iter()
            .map(ComposioToolSchemaV3::from_v3_tool)
            .collect())
    }
}

pub(super) fn map_v3_tools_to_actions(items: Vec<ComposioV3Tool>) -> Vec<ComposioAction> {
    items
        .into_iter()
        .filter_map(|item| {
            let name = item.slug.or(item.name.clone())?;
            let app_name = item
                .toolkit
                .as_ref()
                .and_then(|toolkit| toolkit.slug.clone().or(toolkit.name.clone()))
                .or(item.app_name);
            let description = item.description.or(item.name);
            Some(ComposioAction {
                name,
                app_name,
                description,
                enabled: true,
            })
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub(super) struct ComposioActionsResponse {
    #[serde(default)]
    pub(super) items: Vec<ComposioAction>,
}

#[derive(Debug, Deserialize)]
pub(super) struct ComposioToolsResponse {
    #[serde(default)]
    pub(super) items: Vec<ComposioV3Tool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ComposioV3Tool {
    #[serde(default)]
    pub(super) slug: Option<String>,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) description: Option<String>,
    #[serde(rename = "appName", default)]
    pub(super) app_name: Option<String>,
    #[serde(default)]
    pub(super) toolkit: Option<ComposioToolkitRef>,
    /// JSON schema for the tool parameters. Composio v3 names this
    /// `input_parameters`; older payloads use `parameters`. Either
    /// shape deserialises into this field, and we re-emit it as
    /// `ComposioToolFunction::parameters` so direct-mode users get
    /// the same model-callable schema backend mode surfaces.
    #[serde(default, alias = "parameters")]
    pub(super) input_parameters: Option<serde_json::Value>,
    /// JSON schema for the tool's OUTPUT/return value, per Composio v3
    /// `/tools`'s `output_parameters` field ("Schema definition of return
    /// values from the tool" —
    /// <https://docs.composio.dev/reference/api-reference/tools/getTools>).
    /// Re-emitted as `ComposioToolFunction::output_parameters` so callers
    /// can ground a downstream binding in the tool's real output field
    /// names instead of guessing them.
    #[serde(default)]
    pub(super) output_parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ComposioToolkitRef {
    #[serde(default)]
    pub(super) slug: Option<String>,
    #[serde(default)]
    pub(super) name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposioAction {
    pub name: String,
    #[serde(rename = "appName")]
    pub app_name: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

/// Direct-mode tool definition lifted from Composio v3 `/tools`.
///
/// Carries the `input_parameters` JSON schema so the upstream
/// `composio_list_tools` direct branch can hand the LLM agent a
/// model-callable function shape — same fields backend mode surfaces
/// through `ComposioToolSchema`.
///
/// Kept distinct from `ComposioAction` (legacy flattened shape) so
/// new callers explicitly opt into the schema-preserving variant.
#[derive(Debug, Clone)]
pub struct ComposioToolSchemaV3 {
    pub slug: String,
    pub description: Option<String>,
    pub toolkit_slug: Option<String>,
    pub input_parameters: Option<serde_json::Value>,
    /// See [`ComposioV3Tool::output_parameters`] — Composio v3's schema for
    /// the action's return value, when published.
    pub output_parameters: Option<serde_json::Value>,
}

impl ComposioToolSchemaV3 {
    fn from_v3_tool(item: ComposioV3Tool) -> Self {
        let slug = item
            .slug
            .clone()
            .or_else(|| item.name.clone())
            .unwrap_or_default();
        let toolkit_slug = item
            .toolkit
            .as_ref()
            .and_then(|t| t.slug.clone().or(t.name.clone()))
            .or(item.app_name);
        Self {
            slug,
            description: item.description.or(item.name),
            toolkit_slug,
            input_parameters: item.input_parameters,
            output_parameters: item.output_parameters,
        }
    }
}
