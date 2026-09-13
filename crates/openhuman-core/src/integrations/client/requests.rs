//! Outbound request plumbing for [`IntegrationClient`]: route guards, egress
//! disclosure/enforcement, budget gating, and the JSON verb methods
//! (`post`/`get`/`patch`/`delete`/`upload_multipart`).

use super::construct::IntegrationClient;

pub(super) fn managed_budget_applies_to_path(path: &str) -> bool {
    path != "/agent-integrations/pricing" && path.starts_with("/agent-integrations/")
}

fn reject_backend_webhook_path(method: &str, path: &str) -> anyhow::Result<()> {
    let route = path.split('?').next().unwrap_or(path);
    if route
        .split('/')
        .any(|segment| segment.eq_ignore_ascii_case("webhooks"))
    {
        anyhow::bail!(
            "route is intentionally not exposed by the SDK: {} {}",
            method,
            route
        );
    }
    Ok(())
}

/// Build the egress descriptor for a managed-backend round-trip. The query
/// string is stripped so only the endpoint (the "to where") is described, never
/// any data carried in the query.
pub(super) fn backend_egress_descriptor(path: &str) -> crate::security::egress::EgressDescriptor {
    let endpoint = path.split('?').next().unwrap_or(path);
    crate::security::egress::EgressDescriptor::integration(endpoint)
}

/// Egress spine (privacy epic S2, #4436): disclose an OpenHuman managed-backend
/// round-trip before it leaves the device. Fire-and-forget — never fails the
/// caller.
pub(super) fn emit_backend_egress(path: &str) {
    crate::security::egress::emit_external_transfer(backend_egress_descriptor(path));
}

/// Local-only enforcement (privacy epic S7, #4441): refuse a managed-backend
/// round-trip that ships user data when the live policy is `LocalOnly`. Returns
/// `Ok(())` for control-plane paths (session / team / billing / integration
/// connection-management + catalog) even under `LocalOnly` — blocking those
/// would break sign-in and the Connections UI for no privacy benefit. See
/// [`is_control_plane`](crate::security::egress). Call before
/// [`emit_backend_egress`] so a blocked call is neither disclosed nor sent.
pub(super) fn enforce_backend_egress(path: &str) -> anyhow::Result<()> {
    crate::security::egress::enforce_egress(&backend_egress_descriptor(path))
}

impl IntegrationClient {
    pub(super) async fn ensure_budget_available(&self, path: &str) -> anyhow::Result<()> {
        if !managed_budget_applies_to_path(path) {
            return Ok(());
        }
        if let Some(config) = &self.budget_config {
            if crate::hosted::team::managed_tool_budget_exhausted(config).await {
                anyhow::bail!(
                    "Managed cloud tools are disabled because your OpenHuman AI credits are exhausted. Add credits or route the task to user-supplied providers."
                );
            }
        }
        Ok(())
    }

    /// POST JSON to a backend endpoint and parse the response `data` field.
    pub async fn post<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        self.request_json(reqwest::Method::POST, path, Some(body))
            .await
    }

    /// GET from a backend endpoint and parse the response `data` field.
    pub async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        self.request_json(reqwest::Method::GET, path, None).await
    }

    pub(super) async fn request_json<T: serde::de::DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> anyhow::Result<T> {
        reject_backend_webhook_path(method.as_str(), path)?;
        enforce_backend_egress(path)?;
        emit_backend_egress(path);
        self.ensure_budget_available(path).await?;
        let url = crate::api::config::api_url(&self.backend_url, path);
        let method_name = method.as_str().to_ascii_lowercase();
        tracing::debug!("[integrations] {} {}", method.as_str(), url);
        let value = self
            .sdk
            .raw()
            .send(method, path, &[], body, false)
            .await
            .map_err(|error| Self::map_sdk_error(error, &method_name, path, &url))?;
        Self::parse_envelope(&method_name, path, &url, value)
    }

    /// PATCH JSON to a backend endpoint and parse the response `data` field.
    /// Mirrors [`Self::post`] (auth header, 401 → session-expiry, error
    /// classification).
    pub async fn patch<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> anyhow::Result<T> {
        self.request_json(reqwest::Method::PATCH, path, Some(body))
            .await
    }

    /// DELETE a backend resource and parse the response `data` field.
    /// Mirrors [`Self::post`] (auth header, 401 → session-expiry, error
    /// classification).
    pub async fn delete<T: serde::de::DeserializeOwned>(&self, path: &str) -> anyhow::Result<T> {
        self.request_json(reqwest::Method::DELETE, path, None).await
    }

    /// POST a `multipart/form-data` body to a backend endpoint and parse the
    /// response `data` field. Mirrors [`Self::post`] URL building, Bearer
    /// auth, 401 → session-expiry handling and error classification; the
    /// content type is set by reqwest from the form boundary.
    pub async fn upload_multipart<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> anyhow::Result<T> {
        reject_backend_webhook_path("POST", path)?;
        enforce_backend_egress(path)?;
        emit_backend_egress(path);
        self.ensure_budget_available(path).await?;
        let url = crate::api::config::api_url(&self.backend_url, path);
        tracing::debug!("[integrations] POST(multipart) {}", url);

        let value = self
            .sdk
            .raw()
            .post_multipart(path, form)
            .await
            .map_err(|error| Self::map_sdk_error(error, "post_multipart", path, &url))?;
        // The SDK unwraps successful `{success,data}` responses. Preserve
        // compatibility with endpoints that return their payload directly,
        // while still recognizing a `success:false` envelope.
        if value.get("success") == Some(&serde_json::Value::Bool(false)) {
            return Self::parse_envelope("post_multipart", path, &url, value);
        }
        Ok(serde_json::from_value(value)?)
    }
}
