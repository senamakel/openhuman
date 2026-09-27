//! Test-only `reqwest` transport so the core's wiremock unit tests exercise
//! [`BackendClient`](crate::backend::BackendClient) and
//! `IntegrationClient` without any host crate. Never compiled into a
//! production build: [`resolve_backend_transport`](super::resolve_backend_transport)
//! only reaches it under `cfg(test)`.
//!
//! Deliberately *not* a copy of the SDK transport: it applies no route policy
//! and sends no `x-sdk-client`. The `openhuman-tinyhumans` crate carries a
//! parity test pinning the headers and envelope behaviour both share.

use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::CONTENT_TYPE;
use reqwest::Method;
use serde_json::Value;

use super::{
    compose_url, credential_headers, parse_body_text, unwrap_envelope, BackendRequest,
    BackendTransport, BackendTransportError, BaseUrlPurpose, TransportProfile,
};

/// Attribution header the test transport stamps, mirroring the host's.
pub const TEST_PRODUCT_HEADER: &str = "x-sdk-name";
/// Product identity the test transport reports and stamps.
pub const TEST_PRODUCT_IDENTITY: &str = "openhuman";
/// Base URL for a test that configured none: the discard port on loopback.
pub const TEST_FALLBACK_BASE_URL: &str = "http://127.0.0.1:9";

fn plain_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("plain test client")
}

pub struct PlainHttpTransport {
    api: reqwest::Client,
    integrations: reqwest::Client,
}

impl PlainHttpTransport {
    pub fn new() -> Self {
        Self {
            api: plain_client(),
            integrations: plain_client(),
        }
    }

    /// A fresh instance (building two `reqwest::Client`s is cheap).
    pub fn fresh() -> Arc<dyn BackendTransport> {
        Arc::new(Self::new())
    }

    fn client(&self, profile: TransportProfile) -> &reqwest::Client {
        match profile {
            TransportProfile::Api => &self.api,
            TransportProfile::Integrations => &self.integrations,
        }
    }

    async fn finish(
        response: reqwest::Response,
        unwrap: bool,
    ) -> Result<Value, BackendTransportError> {
        let status = response.status();
        let value = parse_body_text(response.text().await?);
        if !status.is_success() {
            return Err(BackendTransportError::Status {
                status: status.as_u16(),
                body: value,
            });
        }
        if unwrap {
            unwrap_envelope(value)
        } else {
            Ok(value)
        }
    }
}

impl Default for PlainHttpTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BackendTransport for PlainHttpTransport {
    async fn send_json(&self, req: BackendRequest<'_>) -> Result<Value, BackendTransportError> {
        let url = compose_url(req.base_url, req.path, req.query)?;
        let mut request = self
            .client(req.profile)
            .request(req.method, url)
            .header(TEST_PRODUCT_HEADER, TEST_PRODUCT_IDENTITY)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json");
        if let Some(credential) = req.credential {
            request = request.headers(credential_headers(credential)?);
        }
        if let Some(body) = req.body {
            request = request.json(body);
        }
        Self::finish(request.send().await?, req.unwrap_envelope).await
    }

    async fn send_multipart(
        &self,
        req: BackendRequest<'_>,
        form: reqwest::multipart::Form,
    ) -> Result<Value, BackendTransportError> {
        let url = compose_url(req.base_url, req.path, &[])?;
        let mut request = self
            .client(req.profile)
            .request(Method::POST, url)
            .header(TEST_PRODUCT_HEADER, TEST_PRODUCT_IDENTITY)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(credential) = req.credential {
            request = request.headers(credential_headers(credential)?);
        }
        Self::finish(request.multipart(form).send().await?, true).await
    }

    fn http_client(&self, profile: TransportProfile) -> reqwest::Client {
        self.client(profile).clone()
    }

    fn base_url(&self, configured: Option<&str>, _purpose: BaseUrlPurpose) -> String {
        // No defaults here: a test that reaches the backend points
        // `api_url` at its mock server. The fallback is a discard port so an
        // unconfigured test can never reach a real host.
        configured
            .map(crate::util::url::normalize_backend_api_base_url)
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| TEST_FALLBACK_BASE_URL.to_string())
    }

    fn product_identity(&self) -> String {
        TEST_PRODUCT_IDENTITY.to_string()
    }

    fn attribution_headers(&self) -> reqwest::header::HeaderMap {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            TEST_PRODUCT_HEADER,
            reqwest::header::HeaderValue::from_static(TEST_PRODUCT_IDENTITY),
        );
        headers
    }

    fn name(&self) -> &'static str {
        "plain-test"
    }
}
