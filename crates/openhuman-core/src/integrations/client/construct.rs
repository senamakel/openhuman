//! `IntegrationClient` type definition, backend-URL sanitisation, and the
//! constructors that wire up its HTTP transports.

use std::sync::Arc;
use std::time::Duration;

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
/// via [`crate::util::url::join_url`].
pub(super) fn sanitize_backend_url(backend_url: &str) -> String {
    let cleaned = crate::util::url::normalize_backend_api_base_url(backend_url);
    let trimmed = backend_url.trim().trim_end_matches('/');
    if !cleaned.is_empty() && cleaned != trimmed {
        // Redact userinfo (username/password) before logging — a
        // misconfigured URL could carry credentials in the authority
        // segment. The helper preserves host/path for diagnosability
        // while scrubbing secrets.
        tracing::warn!(
            input = %crate::util::redact_url_for_log(trimmed),
            cleaned = %crate::util::redact_url_for_log(&cleaned),
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
/// the download `reqwest::Client`, and a lazily-fetched pricing cache. JSON
/// traffic rides the process [`BackendTransport`](crate::backend::transport::BackendTransport).
pub struct IntegrationClient {
    pub backend_url: String,
    pub auth_token: String,
    /// `auth_token` in the shape the transport takes: a session JWT rides
    /// `Authorization: Bearer`, a TinyHumans API key rides `x-api-key` (see
    /// `api::transport::credential_headers`). The 401 handling in `errors.rs`
    /// branches on which one it is.
    pub(super) credential: crate::security::credentials::session_support::BackendCredential,
    pub(super) budget_config: Option<Arc<crate::config::Config>>,
    // The binary download path never rode the SDK: file storage also consumes
    // Content-Type and Content-Disposition, and the presigned-redirect hop
    // must not carry attribution headers (see `new_inner`).
    pub(super) download_client: reqwest::Client,
    /// Follows storage redirects without inheriting any backend credential.
    pub(super) presigned_client: reqwest::Client,
    pub(super) pricing: tokio::sync::OnceCell<IntegrationPricing>,
}

impl IntegrationClient {
    /// A client authenticating with a session JWT.
    pub fn new(backend_url: String, auth_token: String) -> Self {
        Self::new_inner(
            backend_url,
            crate::security::credentials::session_support::BackendCredential::Session(auth_token),
            None,
        )
    }

    /// A client authenticating with whichever credential
    /// [`resolve_backend_credential`](crate::security::credentials::session_support::resolve_backend_credential)
    /// chose: the TinyHumans API key or the session JWT.
    pub fn new_with_credential(
        backend_url: String,
        credential: crate::security::credentials::session_support::BackendCredential,
    ) -> Self {
        Self::new_inner(backend_url, credential, None)
    }

    pub fn new_with_budget_config(
        backend_url: String,
        auth_token: String,
        config: Arc<crate::config::Config>,
    ) -> Self {
        Self::new_inner(
            backend_url,
            crate::security::credentials::session_support::BackendCredential::Session(auth_token),
            Some(config),
        )
    }

    /// Credential-aware variant of [`Self::new_with_budget_config`].
    pub fn new_with_credential_and_budget_config(
        backend_url: String,
        credential: crate::security::credentials::session_support::BackendCredential,
        config: Arc<crate::config::Config>,
    ) -> Self {
        Self::new_inner(backend_url, credential, Some(config))
    }

    /// Whether this client authenticates with a TinyHumans API key rather
    /// than a session JWT.
    pub fn uses_api_key(&self) -> bool {
        self.credential.is_api_key()
    }

    /// The auth headers for a request built outside the backend transport
    /// (binary download, raw DELETE): `Authorization: Bearer` for a session,
    /// `x-api-key` for an API key.
    pub(crate) fn auth_headers(&self) -> anyhow::Result<reqwest::header::HeaderMap> {
        self.validate_credential_endpoint()?;
        crate::backend::transport::credential_headers(&self.credential)
            .map_err(|e| anyhow::anyhow!("invalid backend credential header: {e}"))
    }

    pub(super) fn validate_credential_endpoint(&self) -> anyhow::Result<()> {
        let endpoint = &self.backend_url;
        if self.uses_api_key()
            && !crate::inference::provider::openhuman_backend_model::is_managed_endpoint_for_api_key(
                endpoint,
            )
        {
            anyhow::bail!("TinyHumans API key requires the managed backend or a loopback endpoint");
        }
        if !crate::inference::provider::openhuman_backend_model::is_safe_endpoint_for_managed_bearer(
            endpoint,
        ) {
            anyhow::bail!("backend credential requires HTTPS or a loopback HTTP endpoint");
        }
        Ok(())
    }

    fn new_inner(
        backend_url: String,
        credential: crate::security::credentials::session_support::BackendCredential,
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

        // JSON traffic goes through the process backend transport
        // (`TransportProfile::Integrations`: platform TLS, 60 s timeout,
        // product identity — see `openhuman_tinyhumans::backend::headers`).
        // Only the binary download
        // client is built here.
        //
        // `download_client` deliberately does NOT carry the product identity.
        // Its one caller (`get_bytes`) fetches a route that can redirect to
        // presigned storage. The first client stops at the redirect; the second
        // follows it without x-api-key or any other backend header.
        //
        // These raw clients do not add product attribution to storage requests.
        let download_client = crate::util::tls::tls_client_builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15 * 60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build integration download HTTP client");
        let presigned_client = crate::util::tls::tls_client_builder()
            .http1_only()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15 * 60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build presigned download HTTP client");

        let auth_token = credential.secret().to_owned();
        Self {
            backend_url,
            auth_token,
            credential,
            budget_config,
            download_client,
            presigned_client,
            pricing: tokio::sync::OnceCell::new(),
        }
    }
}
