//! Action execution: `execute_action` (v3-with-v2-fallback) and its two
//! version-specific request builders/senders.

use super::http_errors::response_error;
use super::types::{ComposioTool, COMPOSIO_API_BASE_V3};
use anyhow::Context;
use serde_json::json;

impl ComposioTool {
    /// Execute a Composio action/tool with given parameters.
    ///
    /// Uses v3 endpoint first and falls back to v2 for compatibility.
    pub async fn execute_action(
        &self,
        action_name: &str,
        params: serde_json::Value,
        entity_id: Option<&str>,
        connected_account_ref: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        // The Composio v3 action-execute contract keys off the UPPERCASE_SNAKE
        // *action* slug (e.g. `GMAIL_SEND_EMAIL`) at `/tools/execute/{slug}`.
        // The previous code lowercased + dashed it into the *toolkit* slug
        // (`gmail-send-email`) and posted to the wrong `/tools/{slug}/execute`
        // path, so every direct-mode execute 404'd (issue #3219). Pass the
        // action slug through verbatim (trimmed only); the v2 fallback already
        // used the same untransformed name.
        let action_slug = action_name.trim();

        match self
            .execute_action_v3(
                action_slug,
                params.clone(),
                entity_id,
                connected_account_ref,
            )
            .await
        {
            Ok(result) => Ok(result),
            Err(v3_err) => match self.execute_action_v2(action_name, params, entity_id).await {
                Ok(result) => Ok(result),
                Err(v2_err) => anyhow::bail!(
                    "Composio execute failed on v3 ({v3_err}) and v2 fallback ({v2_err})"
                ),
            },
        }
    }

    pub(crate) fn build_execute_action_v3_request(
        action_slug: &str,
        params: serde_json::Value,
        entity_id: Option<&str>,
        connected_account_ref: Option<&str>,
    ) -> (String, serde_json::Value) {
        // POST /api/v3/tools/execute/{ACTION_SLUG} — the action slug stays
        // UPPERCASE_SNAKE (see `execute_action`). Path is `/tools/execute/{slug}`,
        // NOT `/tools/{slug}/execute` (issue #3219).
        let url = format!("{COMPOSIO_API_BASE_V3}/tools/execute/{action_slug}");
        let account_ref = connected_account_ref.and_then(|candidate| {
            let trimmed_candidate = candidate.trim();
            (!trimmed_candidate.is_empty()).then_some(trimmed_candidate)
        });

        let mut body = json!({
            "arguments": params,
        });

        if let Some(entity) = entity_id {
            body["user_id"] = json!(entity);
        }
        if let Some(account_ref) = account_ref {
            body["connected_account_id"] = json!(account_ref);
        }

        (url, body)
    }

    async fn execute_action_v3(
        &self,
        action_slug: &str,
        params: serde_json::Value,
        entity_id: Option<&str>,
        connected_account_ref: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let (_default_url, body) = Self::build_execute_action_v3_request(
            action_slug,
            params,
            entity_id,
            connected_account_ref,
        );
        let url = format!("{}/tools/execute/{action_slug}", self.base_v3);

        self.ensure_request_url(&url)?;

        let resp = self
            .client()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v3 action execution failed: {err}");
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v3 execute response")?;
        Ok(result)
    }

    async fn execute_action_v2(
        &self,
        action_name: &str,
        params: serde_json::Value,
        entity_id: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let url = format!("{}/actions/{action_name}/execute", self.base_v2);

        let mut body = json!({
            "input": params,
        });

        if let Some(entity) = entity_id {
            body["entityId"] = json!(entity);
        }

        let resp = self
            .client()
            .post(&url)
            .header("x-api-key", &self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = response_error(resp).await;
            anyhow::bail!("Composio v2 action execution failed: {err}");
        }

        let result: serde_json::Value = resp
            .json()
            .await
            .context("Failed to decode Composio v2 execute response")?;
        Ok(result)
    }
}
