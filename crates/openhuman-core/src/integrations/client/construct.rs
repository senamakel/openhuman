//! `IntegrationClient` type definition, backend-URL sanitisation, and the
//! constructors that wire up its HTTP transports.

use std::sync::Arc;
use std::time::Duration;
use tinyhumans_sdk::TinyHumansClient;

use crate::integrations::types::IntegrationPricing;

/// Strip any inference-style path that snuck into a backend URL before
/// it becomes the [`IntegrationClient::backend_url`] field. Idempotent —
/// returns the input unchanged when already clean.
///
/// See issue #2075 / Sentry `OPENHUMAN-TAURI-H6`, `-HN`: a misconfigured
/// `BACKEND_URL` env (e.g. `https://api.tinyhumans.ai/openai/v1/chat/completions`)
/// baked into a build silently produced 404 URLs like
/// `…/openai/v1/chat/completions/agent-integrations/composio/connections`
/// because every `IntegrationClient` method joins paths onto this field
/// via [`crate::api::config::api_url`].
pub(super) fn sanitize_backend_url(backend_url: &str) -> String {
    let cleaned = crate::api::config::normalize_backend_api_base_url(backend_url);
    let trimmed = backend_url.trim().trim_end_matches('/');
    if !cleaned.is_empty() && cleaned != trimmed {
        // Redact userinfo (username/password) before logging — a
        // misconfigured URL could carry credentials in the authority
        // segment. The helper preserves host/path for diagnosability
        // while scrubbing secrets.
        tracing::warn!(
            input = %crate::api::config::redact_url_for_log(trimmed),
            cleaned = %crate::api::config::redact_url_for_log(&cleaned),
            "[integrations] backend_url carried an inference / non-root path; \
             stripping before use (issue #2075)"
        );
    }
    if cleaned.is_empty() {
        backend_url.to_string()
    } else {
        cleaned
    }
}

/// Shared client for all integration tools. Holds backend URL, auth token,
/// a reusable `reqwest::Client`, and a lazily-fetched pricing cache.
pub struct IntegrationClient {
    pub backend_url: String,
    pub auth_token: String,
    pub(super) budget_config: Option<Arc<crate::config::Config>>,
    pub(super) sdk: TinyHumansClient,
    // Temporary compatibility exception: the SDK's binary primitive returns
    // bytes only, while file storage also consumes Content-Type and
    // Content-Disposition. Remove when the SDK exposes response metadata.
    pub(super) download_client: reqwest::Client,
    pub(super) pricing: tokio::sync::OnceCell<IntegrationPricing>,
}

impl IntegrationClient {
    pub fn new(backend_url: String, auth_token: String) -> Self {
        Self::new_inner(backend_url, auth_token, None)
    }

    pub fn new_with_budget_config(
        backend_url: String,
        auth_token: String,
        config: Arc<crate::config::Config>,
    ) -> Self {
        Self::new_inner(backend_url, auth_token, Some(config))
    }

    fn new_inner(
        backend_url: String,
        auth_token: String,
        budget_config: Option<Arc<crate::config::Config>>,
    ) -> Self {
        // Defense-in-depth (issue #2075 / Sentry OPENHUMAN-TAURI-H6, -HN):
        // every prod call site routes `backend_url` through
        // `effective_backend_api_url` which strips inference-style paths,
        // but any future caller that forgets that step would silently
        // produce 404 URLs like
        //   https://api.tinyhumans.ai/openai/v1/chat/completions/agent-integrations/composio/connections
        // (the inference path concatenated with every domain path). We
        // re-strip here so the field invariant — "backend_url has no
        // inference path" — holds locally, and `warn!` once when we have
        // to fix up the input so the regression is observable in logs.
        let backend_url = sanitize_backend_url(&backend_url);

        // Platform-appropriate TLS backend — see [`crate::util::tls`].
        // Windows uses schannel (native-tls) to honor the OS cert store;
        // macOS / Linux keep rustls which avoids the OpenSSL runtime dep and
        // has historically been more reliable on staging TLS handshakes.
        //
        // `/agent-integrations/*` is backend traffic like any other, so it
        // carries the same product identity as `BackendOAuthClient`. The SDK
        // merges its own default headers into every request, so `http_client`
        // needs nothing beyond `with_default_headers` below.
        let product_headers = crate::api::product::product_identity_headers();
        let http_client = crate::util::tls::tls_client_builder()
            .http1_only()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build integration HTTP client");
        let sdk = TinyHumansClient::new(&backend_url)
            .with_token(Some(auth_token.clone()))
            .with_http_client(http_client.clone())
            .with_default_headers(product_headers);
        // `download_client` deliberately does NOT carry the product identity.
        // Its one caller (`get_bytes`) fetches
        // `/agent-integrations/file-storage/files/{id}/download`, which answers
        // a 302 to a presigned S3 URL. reqwest follows redirects by default and
        // strips only *sensitive* headers (Authorization, Cookie, …) when the
        // host changes — a custom header like `x-sdk-name` survives the hop, so
        // tagging this transport would disclose the product identity to the
        // storage provider. Attaching it per-request would not help: redirected
        // requests carry the original request headers too.
        //
        // The lost attribution is deliberate and cheap: the header cannot be
        // scoped to the first hop without hand-rolling redirect following, and
        // any session that downloads a file has already made SDK-path calls that
        // are tagged.
        let download_client = crate::util::tls::tls_client_builder()
            .http1_only()
            .timeout(Duration::from_secs(15 * 60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build integration download HTTP client");

        Self {
            backend_url,
            auth_token,
            budget_config,
            sdk,
            download_client,
            pricing: tokio::sync::OnceCell::new(),
        }
    }
}
