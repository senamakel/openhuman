//! Response-parsing helpers shared by every Composio HTTP call path:
//! turning a failed `reqwest::Response` into a redacted error string, and
//! pulling a redirect URL out of a connect-flow JSON body.

pub(super) async fn response_error(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if body.trim().is_empty() {
        return format!("HTTP {}", status.as_u16());
    }

    if let Some(api_error) = extract_api_error_message(&body) {
        return format!(
            "HTTP {}: {}",
            status.as_u16(),
            sanitize_error_message(&api_error)
        );
    }

    format!("HTTP {}", status.as_u16())
}

pub(super) fn sanitize_error_message(message: &str) -> String {
    let mut sanitized = message.replace('\n', " ");
    for marker in [
        "connected_account_id",
        "connectedAccountId",
        "entity_id",
        "entityId",
        "user_id",
        "userId",
    ] {
        sanitized = sanitized.replace(marker, "[redacted]");
    }

    crate::util::truncate_with_ellipsis(&sanitized, 240)
}

pub(super) fn extract_api_error_message(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed
        .get("error")
        .and_then(|v| v.get("message"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| {
            parsed
                .get("message")
                .and_then(|v| v.as_str())
                .map(ToString::to_string)
        })
}

pub(super) fn extract_redirect_url(result: &serde_json::Value) -> Option<String> {
    result
        .get("redirect_url")
        .and_then(|v| v.as_str())
        .or_else(|| result.get("redirectUrl").and_then(|v| v.as_str()))
        .or_else(|| {
            result
                .get("data")
                .and_then(|v| v.get("redirect_url"))
                .and_then(|v| v.as_str())
        })
        .map(ToString::to_string)
}
