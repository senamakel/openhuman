//! Computer-use (OS-level) action validation and execution for
//! [`BrowserTool`].

use super::*;

impl BrowserTool {
    pub(super) fn validate_coordinate(
        &self,
        key: &str,
        value: i64,
        max: Option<i64>,
    ) -> anyhow::Result<()> {
        if value < 0 {
            anyhow::bail!("'{key}' must be >= 0")
        }
        if let Some(limit) = max {
            if limit < 0 {
                anyhow::bail!("Configured coordinate limit for '{key}' must be >= 0")
            }
            if value > limit {
                anyhow::bail!("'{key}'={value} exceeds configured limit {limit}")
            }
        }
        Ok(())
    }

    fn read_required_i64(
        &self,
        params: &serde_json::Map<String, Value>,
        key: &str,
    ) -> anyhow::Result<i64> {
        params
            .get(key)
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid '{key}' parameter"))
    }

    pub(super) fn validate_computer_use_action(
        &self,
        action: &str,
        params: &serde_json::Map<String, Value>,
    ) -> anyhow::Result<()> {
        match action {
            "open" => {
                let url = params
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Missing 'url' for open action"))?;
                self.validate_url(url)?;
            }
            "mouse_move" | "mouse_click" => {
                let x = self.read_required_i64(params, "x")?;
                let y = self.read_required_i64(params, "y")?;
                self.validate_coordinate("x", x, self.computer_use.max_coordinate_x)?;
                self.validate_coordinate("y", y, self.computer_use.max_coordinate_y)?;
            }
            "mouse_drag" => {
                let from_x = self.read_required_i64(params, "from_x")?;
                let from_y = self.read_required_i64(params, "from_y")?;
                let to_x = self.read_required_i64(params, "to_x")?;
                let to_y = self.read_required_i64(params, "to_y")?;
                self.validate_coordinate("from_x", from_x, self.computer_use.max_coordinate_x)?;
                self.validate_coordinate("to_x", to_x, self.computer_use.max_coordinate_x)?;
                self.validate_coordinate("from_y", from_y, self.computer_use.max_coordinate_y)?;
                self.validate_coordinate("to_y", to_y, self.computer_use.max_coordinate_y)?;
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) async fn execute_computer_use_action(
        &self,
        action: &str,
        args: &Value,
    ) -> anyhow::Result<ToolResult> {
        let endpoint = self.computer_use_endpoint_url()?;

        let mut params = args
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("browser args must be a JSON object"))?;
        params.remove("action");

        self.validate_computer_use_action(action, &params)?;

        let payload = json!({
            "action": action,
            "params": params,
            "policy": {
                "allowed_domains": self.allowed_domains,
                "window_allowlist": self.computer_use.window_allowlist,
                "max_coordinate_x": self.computer_use.max_coordinate_x,
                "max_coordinate_y": self.computer_use.max_coordinate_y,
            },
            "metadata": {
                "session_name": self.session_name,
                "source": "openhuman.browser",
                "version": env!("CARGO_PKG_VERSION"),
            }
        });

        let client = crate::config::build_runtime_proxy_client("tool.browser");
        let mut request = client
            .post(endpoint)
            .timeout(Duration::from_millis(self.computer_use.timeout_ms))
            .json(&payload);

        if let Some(api_key) = self.computer_use.api_key.as_deref() {
            let token = api_key.trim();
            if !token.is_empty() {
                request = request.bearer_auth(token);
            }
        }

        let response = request.send().await.with_context(|| {
            format!(
                "Failed to call computer-use sidecar at {}",
                self.computer_use.endpoint
            )
        })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read computer-use sidecar response body")?;

        if let Ok(parsed) = serde_json::from_str::<ComputerUseResponse>(&body) {
            if status.is_success() && parsed.success.unwrap_or(true) {
                let output = parsed
                    .data
                    .map(|data| serde_json::to_string_pretty(&data).unwrap_or_default())
                    .unwrap_or_else(|| {
                        serde_json::to_string_pretty(&json!({
                            "backend": "computer_use",
                            "action": action,
                            "ok": true,
                        }))
                        .unwrap_or_default()
                    });

                return Ok(ToolResult::success(output));
            }

            let error = parsed.error.or_else(|| {
                if status.is_success() && parsed.success == Some(false) {
                    Some("computer-use sidecar returned success=false".to_string())
                } else {
                    Some(format!(
                        "computer-use sidecar request failed with status {status}"
                    ))
                }
            });

            return Ok(ToolResult::error(error.unwrap_or_default()));
        }

        if status.is_success() {
            return Ok(ToolResult::success(body));
        }

        Ok(ToolResult::error(format!(
            "computer-use sidecar request failed with status {status}: {}",
            body.trim()
        )))
    }
}
