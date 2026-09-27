//! [`BackendClient`]: authenticated JSON calls to hosted-backend routes over
//! the [`BackendTransport`] port, and the typed errors they surface.

use anyhow::{Context, Result};
use reqwest::{Client, Method, Url};
use serde_json::Value;
use std::sync::Arc;

use crate::backend::transport::{
    resolve_backend_transport, BackendRequest, BackendTransport, BackendTransportError,
    TransportProfile,
};
use crate::security::credentials::session_support::BackendCredential;

/// Typed errors surfaced by `authed_json` for expected backend states that
/// callers should recover from in-flow rather than funnel into Sentry.
#[derive(Debug, thiserror::Error)]
pub enum BackendApiError {
    /// Edit / delete of a channel message returned 404. Happens when the
    /// user deletes the message on the provider side (Telegram, Discord,
    /// Slack, …) but our local `StreamingState` still has the id, or when
    /// the backend GC'd the relay row before we got around to editing it.
    /// Callers should clear stale state and skip the retry. Targets
    /// `OPENHUMAN-TAURI-2Y` (~454 events on `/channels/telegram/messages/<id>`).
    #[error("message not found on {provider}: {message_id}")]
    MessageNotFound {
        /// Channel provider segment (e.g. `"telegram"`, `"discord"`).
        provider: String,
        /// Provider-specific message id from the URL.
        message_id: String,
    },
    /// Backend rejected the bearer JWT with `401 Unauthorized`. This is an
    /// expected user-session state (token expired, revoked, rotated
    /// server-side) — not a code bug. Callers can route to a re-sign-in
    /// flow; the auth domain owns recovery. Targets `OPENHUMAN-TAURI-4K8`
    /// (12 events on `/openai/v1/audio/speech` mascot TTS, but the same
    /// shape fires on every authed endpoint once the session lapses).
    #[error("backend rejected session token on {method} {path}")]
    Unauthorized {
        /// HTTP method as a static string (`"GET"`, `"POST"`, …).
        method: String,
        /// Request path the 401 came back from (no query string).
        path: String,
    },
    /// Backend rejected a TinyHumans API key (`x-api-key`) with
    /// `401 Unauthorized` — a library-mode runtime's credential, not a user
    /// session. Must stay distinct from [`Self::Unauthorized`]:
    /// `flatten_authed_error` maps that variant onto the `SESSION_EXPIRED`
    /// sentinel, which `core/jsonrpc.rs` treats as "clear the app session and
    /// sign out". A rejected API key on a runtime that never had a session
    /// would otherwise trigger that same session-expiry recovery, clearing an
    /// app session that was never the problem and leaving the rejected key
    /// installed. Callers should surface this as a credential error instead.
    #[error("backend rejected api key on {method} {path}")]
    ApiKeyRejected {
        /// HTTP method as a static string (`"GET"`, `"POST"`, …).
        method: String,
        /// Request path the 401 came back from (no query string).
        path: String,
    },
    /// `PATCH /channels/<provider>/messages/<id>` returned 404 because the
    /// backend **implements no such route** — not because the message is gone.
    ///
    /// The backend's channel router serves `POST /:channel/messages` and
    /// `DELETE /:channel/messages/:messageId` only; the `PATCH` edit route was
    /// never added (the deployed OpenAPI contract has no entry for it, and it
    /// is absent from the SDK's generated public-route registry). Every edit
    /// therefore hits the unmatched-route 404, and always has (#5230).
    ///
    /// This must stay distinct from [`Self::MessageNotFound`]: that variant
    /// means "this specific message no longer exists", so its handlers
    /// correctly forget the message id. A missing *route* says nothing about
    /// the message — it is still there and we still own it, so callers must
    /// keep the id (to delete or finally edit it) and only disable the edit
    /// capability. Collapsing the two made the live "💭 Thinking:" bubble and
    /// the streaming draft leak into the chat un-updated and un-deleted.
    ///
    /// Note the backend's `DELETE` handler answers only 400/403/502 and never
    /// 404, so on the deployed contract a 404 on this path can *only* mean
    /// route absence today. `MessageNotFound` is retained for `DELETE` because
    /// the provider-side-deletion semantics are what its callers want and a
    /// future backend revision may start returning it.
    #[error(
        "channel message edit route not implemented by backend ({provider}, message {message_id})"
    )]
    ChannelEditUnsupported {
        /// Channel provider segment (e.g. `"telegram"`, `"discord"`).
        provider: String,
        /// Provider-specific message id from the URL.
        message_id: String,
    },
    /// No [`BackendTransport`] is installed in this process, so the request
    /// could not be sent at all. This is the steady state of a core built
    /// and run without `openhuman-tinyhumans` — an expected build condition,
    /// never a bug. `flatten_authed_error` maps it onto the
    /// `BACKEND_UNAVAILABLE:` sentinel that
    /// `core::observability` demotes.
    #[error("backend transport not available for {method} {path}")]
    BackendUnavailable {
        /// HTTP method as a static string (`"GET"`, `"POST"`, …).
        method: String,
        /// Request path that could not be sent (no query string).
        path: String,
    },
}

/// Flatten an `authed_json` error onto the JSON-RPC `String` channel.
///
/// `BackendApiError::Unauthorized` is an expected backend session-lapse 401
/// (token expired / revoked / rotated server-side), not a code bug — see the
/// variant docs above. Callers used to flatten it with `format!("{e:#}")` /
/// `e.to_string()`, producing `"backend rejected session token on {method}
/// {path}"`, which matches none of the JSON-RPC session-expiry classifiers
/// (`is_session_expired_error`, `is_session_expired_message`, the `before_send`
/// net), so every lapsed-session 401 leaked to Sentry — TAURI-RUST-8WY
/// (`/teams/me/usage`), TAURI-RUST-8WZ (`/payments/stripe/currentPlan`), and the
/// rest of the authed-endpoint family (#3297).
///
/// Mapping `Unauthorized` onto the existing `SESSION_EXPIRED` sentinel makes the
/// dispatcher (`core/jsonrpc.rs`) classify it as session expiry: it skips the
/// Sentry report AND publishes `DomainEvent::SessionExpired` so the auth domain
/// drives re-sign-in. This keys off the typed downcast — not the Display
/// wording — so it stays correct if the `#[error(...)]` text changes, consistent
/// with #2959's removal of brittle string-based suppression. Every other error
/// (including `MessageNotFound`) keeps its full `{e:#}` chain so genuine
/// failures still reach Sentry.
pub fn flatten_authed_error(err: anyhow::Error) -> String {
    match err.downcast_ref::<BackendApiError>() {
        Some(BackendApiError::Unauthorized { method, path }) => {
            format!("SESSION_EXPIRED: backend rejected session token on {method} {path}")
        }
        // Deliberately NOT the `SESSION_EXPIRED` sentinel: this runtime
        // authenticates with an API key, not a session, so there is no
        // session to expire and `core/jsonrpc.rs`'s `SessionExpired` publish
        // (clear the session, prompt re-sign-in) would be the wrong
        // recovery. See `BackendApiError::ApiKeyRejected`.
        Some(BackendApiError::ApiKeyRejected { method, path }) => {
            format!(
                "{}backend rejected api key on {method} {path}",
                crate::core::observability::API_KEY_REJECTED_PREFIX
            )
        }
        Some(BackendApiError::BackendUnavailable { method, path }) => {
            format!(
                "{}backend transport not available for {method} {path}",
                crate::core::observability::BACKEND_UNAVAILABLE_PREFIX
            )
        }
        _ => format!("{err:#}"),
    }
}

/// Whether a 404 body came from *no route matching* rather than from a handler
/// reporting a missing resource.
///
/// The backend registers no catch-all 404, so an unmatched route falls through
/// to Express's built-in `finalhandler`, which answers with an HTML page whose
/// body reads `Cannot PATCH /channels/…`. Every handler-level 404 answers with a
/// JSON envelope instead — `DELETE /channels/:channel/messages/:messageId`
/// already returns `{"success": false, "error": …}`, and a future `PATCH`
/// handler would mirror it.
///
/// So: parses as JSON ⇒ a handler answered ⇒ the message is missing, not the
/// route. Anything else (HTML, plain text, empty) ⇒ treat as route absence.
///
/// The asymmetry is deliberate. Misreading route absence as message absence only
/// costs one wasted edit attempt per message; misreading a message-missing 404 as
/// route absence disables progressive edits for the entire provider for the rest
/// of the process (#5230 review). Defaulting the ambiguous shapes to route
/// absence also preserves today's behaviour, where no backend implements the
/// route at all.
fn is_unmatched_route_404(body: &str) -> bool {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return true;
    }
    serde_json::from_str::<Value>(trimmed).is_err()
}

/// Extract `(provider, message_id)` from a backend channel path of the
/// shape `…/channels/<provider>/messages/<id>`. Returns `None` for paths
/// that do not contain this four-segment subsequence.
///
/// Handles both the canonical four-segment form and paths with an arbitrary
/// base-path prefix (e.g. `/api/v1/channels/telegram/messages/1103`) via a
/// sliding window so that `BACKEND_URL` variants with path prefixes do not
/// silently fall through to `report_error` (OPENHUMAN-TAURI-R7).
fn parse_message_path(path: &str) -> Option<(&str, &str)> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    // Fast path: exact four-segment canonical form /channels/<p>/messages/<id>
    if segments.len() == 4 && segments[0] == "channels" && segments[2] == "messages" {
        return Some((segments[1], segments[3]));
    }
    // Sliding window: handles base-path prefixes like /api/v1/channels/<p>/messages/<id>
    for window in segments.windows(4) {
        if window[0] == "channels" && window[2] == "messages" {
            return Some((window[1], window[3]));
        }
    }
    None
}

/// Max bytes of the `body_shape` key-name list echoed into the `authed_json`
/// report. Bounded so a body with pathologically many keys can't bloat the
/// event; truncation is UTF-8-safe.
const BACKEND_API_BODY_SHAPE_MAX_BYTES: usize = 120;

/// PII-safe classification of a non-2xx response body for telemetry.
///
/// `report_error`'s message is written to the core/Tauri daily logs BEFORE any
/// Sentry `before_send` scrubbing, and that scrubber only catches a few
/// secret-shaped patterns — so the raw body must never be echoed (a non-2xx body
/// can carry emails / profile JSON / OAuth errors / nonstandard token fields).
/// We emit only the SHAPE: for a JSON object, the count of top-level keys plus
/// the sorted subset that look like schema field names; otherwise a coarse
/// label. Even key NAMES are response-controlled (a foreign backend could return
/// `{"jo@example.com": 1}`), so only keys matching a conservative ASCII-identifier
/// shape are echoed — everything else is counted as `redacted` and never logged.
/// The surviving names are enough to identify which backend/gateway produced a
/// response — the `TAURI-RUST-8C` case (a 91-byte body matching no route this
/// backend emits), where our canonical envelope is `{success,error,errorCode}`
/// and a foreign gateway/proxy is not.
fn backend_api_body_shape(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return "empty".to_string();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => {
            let total = map.len();
            let mut safe: Vec<&str> = map
                .keys()
                .map(String::as_str)
                .filter(|k| is_schema_like_key(k))
                .collect();
            safe.sort_unstable();
            let redacted = total - safe.len();
            // `safe` keys are ASCII identifiers, so the join is ASCII and the
            // truncation can only ever land on a byte boundary — but route it
            // through the UTF-8-safe truncator regardless (defence-in-depth).
            let keys = crate::util::truncate_at_byte_boundary(
                &safe.join(","),
                BACKEND_API_BODY_SHAPE_MAX_BYTES,
            );
            format!("object(keys={total},safe=[{keys}],redacted={redacted})")
        }
        Ok(Value::Array(_)) => "array".to_string(),
        Ok(_) => "scalar".to_string(),
        Err(_) => "non_json".to_string(),
    }
}

/// A JSON key safe to echo into telemetry: a short ASCII identifier (the shape
/// of a schema field name). Anything else — non-ASCII, punctuation like `@`,
/// whitespace, or overlong — is treated as response-controlled data and excluded
/// so `body_shape` can never leak an email/UUID/free-text used as a key.
fn is_schema_like_key(key: &str) -> bool {
    const MAX_KEY_LEN: usize = 40;
    !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// Normalize the backend envelope while preserving OpenHuman's historical
/// response shape. In particular, `/auth/me` returns `{success,user}` rather
/// than `{success,data}`; SDK transport must not expose that envelope detail to
/// existing callers.
fn parse_api_response_value(value: Value) -> Result<Value> {
    let Some(object) = value.as_object() else {
        return Ok(value);
    };
    if let Some(user) = object.get("user").filter(|user| !user.is_null()) {
        return Ok(user.clone());
    }
    let Some(success) = object.get("success").and_then(Value::as_bool) else {
        return Ok(value);
    };
    if !success {
        let message = object
            .get("message")
            .or_else(|| object.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("request unsuccessful");
        anyhow::bail!("API request failed: {message}");
    }
    if let Some(data) = object.get("data").filter(|data| !data.is_null()) {
        return Ok(data.clone());
    }
    if let Some(user) = object.get("user").filter(|user| !user.is_null()) {
        return Ok(user.clone());
    }
    let mut unwrapped = object.clone();
    unwrapped.remove("success");
    Ok(Value::Object(unwrapped))
}

/// A client for authenticated hosted-backend routes.
///
/// Owns the *routes* and the error classification; the HTTP round-trip itself
/// rides the process [`BackendTransport`] (see [`crate::backend::transport`]),
/// which is what carries TLS, timeouts, attribution headers and the
/// credential header shape. A core with no transport installed answers every
/// call with [`BackendApiError::BackendUnavailable`].
#[derive(Clone)]
pub struct BackendClient {
    base: Url,
}

impl BackendClient {
    /// Creates a new `BackendClient` with the given API base URL.
    ///
    /// Any path, query, or fragment in `api_base` is stripped so that
    /// `Url::join` always resolves root-relative REST paths correctly.
    /// This guards against callers who pass a full LLM completions URL
    /// (e.g. `https://host/v1/chat/completions`) instead of just the origin:
    /// without stripping, `join("teams/me/usage")` would produce the wrong
    /// path `/v1/chat/teams/me/usage` via RFC 3986 relative resolution.
    pub fn new(api_base: &str) -> Result<Self> {
        let mut base = Url::parse(api_base.trim()).context("Invalid API base URL")?;
        anyhow::ensure!(
            matches!(base.scheme(), "http" | "https") && base.host_str().is_some(),
            "API base URL must be an absolute http(s) URL with host"
        );
        base.set_path("");
        base.set_query(None);
        base.set_fragment(None);
        Ok(Self { base })
    }

    /// A client for the backend origin the installed transport resolves from
    /// `config.api_url` (see [`crate::backend::base_url`]).
    ///
    /// Without a transport this fails with
    /// [`BackendApiError::BackendUnavailable`], which
    /// [`flatten_authed_error`] turns into the `BACKEND_UNAVAILABLE:` sentinel.
    pub fn from_config(config: &crate::config::Config) -> Result<Self> {
        match crate::backend::base_url(&config.api_url) {
            Ok(base) => Self::new(&base),
            Err(BackendTransportError::Unavailable) => {
                log::debug!("[backend-api] no backend transport installed; client unavailable");
                Err(anyhow::Error::new(BackendApiError::BackendUnavailable {
                    method: "ANY".to_string(),
                    path: String::new(),
                }))
            }
            Err(other) => Err(anyhow::Error::new(other)),
        }
    }

    /// The backend origin this client resolves routes against.
    pub fn base_url(&self) -> &str {
        self.base.as_str()
    }

    /// The process backend transport, or [`BackendApiError::BackendUnavailable`]
    /// for `method path` when none is installed.
    fn transport(&self, method: &Method, path: &str) -> Result<Arc<dyn BackendTransport>> {
        resolve_backend_transport().map_err(|error| match error {
            BackendTransportError::Unavailable => {
                let route = self.url_for(path).map(|u| u.path().to_string());
                log::debug!(
                    "[backend-api] no backend transport installed; {} {} unavailable",
                    method.as_str(),
                    route.as_deref().unwrap_or(path)
                );
                anyhow::Error::new(BackendApiError::BackendUnavailable {
                    method: method.as_str().to_string(),
                    path: route.unwrap_or_else(|_| path.to_string()),
                })
            }
            other => anyhow::Error::new(other),
        })
    }

    /// The transport's `reqwest::Client` for callers that need to drive a
    /// non-JSON request shape (e.g. `multipart/form-data` uploads for cloud
    /// STT) without re-implementing TLS/proxy plumbing. Carries the
    /// attribution headers; the caller adds the credential.
    pub fn raw_client(&self) -> Result<Client> {
        Ok(resolve_backend_transport()
            .map_err(|error| match error {
                BackendTransportError::Unavailable => {
                    anyhow::Error::new(BackendApiError::BackendUnavailable {
                        method: "RAW".to_string(),
                        path: String::new(),
                    })
                }
                other => anyhow::Error::new(other),
            })?
            .http_client(TransportProfile::Api))
    }

    /// Resolve a backend-relative path against the configured base URL.
    /// Mirrors what `authed_json` does internally so callers using
    /// `raw_client()` don't have to assemble URLs by hand.
    pub fn url_for(&self, path: &str) -> Result<Url> {
        self.base
            .join(path.trim_start_matches('/'))
            .with_context(|| format!("build URL for {path}"))
    }

    /// Generic authenticated JSON request helper for backend API routes.
    ///
    /// `credential` accepts a [`BackendCredential`] or a bare session token.
    /// The transport sends API keys as `x-api-key` and sessions as Bearer JWTs.
    pub async fn authed_json(
        &self,
        credential: impl Into<BackendCredential>,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let credential = credential.into();
        let is_api_key = credential.is_api_key();
        if is_api_key
            && !crate::inference::provider::openhuman_backend_model::is_managed_endpoint_for_api_key(
                self.base.as_str(),
            )
        {
            anyhow::bail!("TinyHumans API key requires the managed backend or a loopback endpoint");
        }
        if !crate::inference::provider::openhuman_backend_model::is_safe_endpoint_for_managed_bearer(
            self.base.as_str(),
        ) {
            anyhow::bail!("backend credential requires HTTPS or a loopback HTTP endpoint");
        }
        let transport = self.transport(&method, path)?;
        let response = transport
            .send_json(BackendRequest {
                profile: TransportProfile::Api,
                base_url: self.base.as_str(),
                method: method.clone(),
                path,
                query: &[],
                body: body.as_ref(),
                credential: Some(&credential),
                unwrap_envelope: true,
            })
            .await;
        self.finish_authed_json(method, path, response, is_api_key)
    }

    fn finish_authed_json(
        &self,
        method: Method,
        path: &str,
        response: Result<Value, BackendTransportError>,
        is_api_key: bool,
    ) -> Result<Value> {
        let url = self.url_for(path)?;
        let value = match response {
            Ok(value) => return parse_api_response_value(value),
            Err(BackendTransportError::Unavailable) => {
                return Err(anyhow::Error::new(BackendApiError::BackendUnavailable {
                    method: method.as_str().to_string(),
                    path: url.path().to_string(),
                }));
            }
            Err(BackendTransportError::Http(e)) => {
                // Walk the error source chain so transient markers hidden in nested
                // causes (reqwest -> hyper -> rustls TLS EOF, etc.) still classify
                // correctly. The top-level `e.to_string()` often only carries the
                // outermost wrapper, e.g. "error sending request for url (...)".
                let mut error_message = e.to_string();
                let mut src: Option<&(dyn std::error::Error + 'static)> =
                    std::error::Error::source(&e);
                while let Some(s) = src {
                    error_message.push_str(" → ");
                    error_message.push_str(&s.to_string());
                    src = s.source();
                }
                if crate::core::observability::contains_transient_transport_phrase(&error_message) {
                    tracing::warn!(
                        domain = "backend_api",
                        operation = "authed_json",
                        method = method.as_str(),
                        path = url.path(),
                        failure = "transport",
                        error = %error_message,
                        "[backend_api] transient transport failure on {} {}: {}",
                        method.as_str(),
                        url.path(),
                        error_message,
                    );
                } else {
                    crate::core::observability::report_error(
                        error_message.as_str(),
                        "backend_api",
                        "authed_json",
                        &[
                            ("method", method.as_str()),
                            ("path", url.path()),
                            ("failure", "transport"),
                        ],
                    );
                }
                return Err(anyhow::Error::new(e).context(format!(
                    "backend request {} {}",
                    method.as_str(),
                    url.path()
                )));
            }
            Err(BackendTransportError::Status { status, body }) => (status, body),
            Err(error) => {
                return Err(anyhow::Error::new(error).context(format!(
                    "backend request {} {}",
                    method.as_str(),
                    url.path()
                )));
            }
        };
        {
            let (status_code, response_body) = value;
            let status = reqwest::StatusCode::from_u16(status_code)
                .context("backend transport returned an invalid HTTP status")?;
            let text = match response_body {
                Value::String(text) => text,
                other => serde_json::to_string(&other).unwrap_or_default(),
            };
            let status_str = status_code.to_string();

            // 401 on any authed backend endpoint is an expected user-session
            // state (token expired / revoked / rotated server-side), not a
            // code bug — every authed endpoint will see this once the session
            // lapses. Surface a typed `BackendApiError::Unauthorized` so the
            // auth domain can drive recovery, and skip `report_error` to
            // avoid Sentry noise. Targets `OPENHUMAN-TAURI-4K8` (mascot TTS
            // surfaced it first on `/openai/v1/audio/speech`, but the same
            // shape applies to every `authed_json` path).
            if status_code == 401 {
                tracing::info!(
                    domain = "backend_api",
                    operation = "authed_json",
                    method = method.as_str(),
                    path = url.path(),
                    status = status_code,
                    failure = "non_2xx",
                    "[backend_api] 401 on {} {} — session token rejected, surfacing typed error",
                    method.as_str(),
                    url.path(),
                );
                // The credential kind decides the *recovery*, not just the
                // wording: `flatten_authed_error` maps `Unauthorized` onto
                // the `SESSION_EXPIRED` sentinel that triggers session
                // sign-out, which is the wrong recovery for a rejected API
                // key (there is no session to expire) — see
                // `BackendApiError::ApiKeyRejected`.
                return Err(anyhow::Error::new(if is_api_key {
                    BackendApiError::ApiKeyRejected {
                        method: method.as_str().to_string(),
                        path: url.path().to_string(),
                    }
                } else {
                    BackendApiError::Unauthorized {
                        method: method.as_str().to_string(),
                        path: url.path().to_string(),
                    }
                }));
            }

            // 404 on `/channels/<provider>/messages/<id>` is an expected
            // state (user deleted the message provider-side, or backend
            // GC'd the relay row) — not a code bug. Surface a typed
            // `BackendApiError::MessageNotFound` so callers (`bus.rs`
            // streaming/thinking/delete/final paths) can clear stale
            // ids and skip retry, without funneling the 404 into
            // `report_error`. Targets `OPENHUMAN-TAURI-2Y` (~454 events).
            if status_code == 404 {
                let channel_message = parse_message_path(url.path());
                // A 404 on the *edit* route is normally route absence, not
                // message absence — today the backend implements no `PATCH
                // /channels/:channel/messages/:messageId` at all (#5230). Answer
                // with a distinct typed error so `bus.rs` keeps the message id
                // (it still owns that message and must be able to delete it)
                // and only disables the edit capability. Checked before the
                // `MessageNotFound` arm below, which would otherwise swallow it.
                //
                // `is_unmatched_route_404` is what keeps this honest once the
                // route *does* exist (staging, a custom backend, or after the
                // backend PR lands): a handler-level "that message is gone" 404
                // must stay a per-message `MessageNotFound`, because
                // `ChannelEditUnsupported` makes `bus.rs` call
                // `mark_channel_edits_unsupported` and disable progressive edits
                // for the whole provider for the rest of the process.
                if method == Method::PATCH
                    && (channel_message.is_some()
                        || (url.path().contains("/channels/") && url.path().contains("/messages/")))
                    && is_unmatched_route_404(&text)
                {
                    let (provider, message_id) = channel_message
                        .map(|(provider, id)| (provider.to_string(), id.to_string()))
                        .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()));
                    tracing::warn!(
                        domain = "backend_api",
                        operation = "authed_json",
                        provider = provider,
                        message_id = message_id,
                        "[backend_api] channel-message edit 404 on {} {} — backend implements no \
                         edit route; surfacing ChannelEditUnsupported so callers degrade instead \
                         of forgetting the message id (#5230)",
                        method.as_str(),
                        url.path(),
                    );
                    return Err(anyhow::Error::new(
                        BackendApiError::ChannelEditUnsupported {
                            provider,
                            message_id,
                        },
                    ));
                }

                if let Some((provider, message_id)) = channel_message {
                    tracing::info!(
                        domain = "backend_api",
                        operation = "authed_json",
                        provider = provider,
                        message_id = message_id,
                        "[backend_api] message-not-found 404 on {} {} — surfacing typed error",
                        method.as_str(),
                        url.path(),
                    );
                    return Err(anyhow::Error::new(BackendApiError::MessageNotFound {
                        provider: provider.to_string(),
                        message_id: message_id.to_string(),
                    }));
                }
                // Defense-in-depth: DELETE 404s on any channel-message path that
                // parse_message_path could not parse (e.g. exotic URL variant with extra
                // segments). Still an expected backend state — suppress the Sentry event
                // without propagating a typed error. Targets OPENHUMAN-TAURI-R7.
                // PATCH is handled above and returns the typed
                // `ChannelEditUnsupported` for both the parsed and unparsed shapes.
                if method == Method::DELETE
                    && url.path().contains("/channels/")
                    && url.path().contains("/messages/")
                {
                    tracing::debug!(
                        domain = "backend_api",
                        operation = "authed_json",
                        "[backend_api] channel-message 404 on {} {} — path not matched by \
                         parse_message_path, suppressing Sentry (TAURI-R7 defense-in-depth)",
                        method.as_str(),
                        url.path(),
                    );
                    anyhow::bail!(
                        "channel message not found (404) on {} {}",
                        method.as_str(),
                        url.path(),
                    );
                }
            }

            // These are transient infrastructure errors (proxy/CDN/backend
            // temporarily unavailable). They are not code bugs and callers already
            // implement retry/disable logic, so skip Sentry to avoid noise.
            let is_transient_infra =
                crate::core::observability::is_transient_http_status_code(status_code);
            let is_budget_exhausted =
                status_code == 400 && crate::backend::classify::is_budget_exhausted_message(&text);
            if is_budget_exhausted {
                tracing::info!(
                    method = method.as_str(),
                    path = url.path(),
                    status = status_code,
                    failure = "non_2xx",
                    kind = "budget",
                    "[backend_api] budget-exhausted 400 on {} {} — not reporting to Sentry",
                    method.as_str(),
                    url.path(),
                );
            } else if is_transient_infra {
                tracing::warn!(
                    domain = "backend_api",
                    operation = "authed_json",
                    method = method.as_str(),
                    path = url.path(),
                    status = status_code,
                    failure = "non_2xx",
                    "[backend_api] transient {status} on {} {} — not reporting to Sentry",
                    method.as_str(),
                    url.path(),
                );
            } else {
                // Record the host and JSON key names to locate misrouted
                // backend errors without sending response values (TAURI-RUST-8C).
                let host = url.host_str().unwrap_or("");
                let body_shape = backend_api_body_shape(&text);
                crate::core::observability::report_error(
                    format!(
                        "{} {} failed ({status}); response_body_len={}; body_shape={}",
                        method.as_str(),
                        url.path(),
                        text.len(),
                        body_shape,
                    )
                    .as_str(),
                    "backend_api",
                    "authed_json",
                    &[
                        ("method", method.as_str()),
                        ("path", url.path()),
                        ("host", host),
                        ("status", status_str.as_str()),
                        ("failure", "non_2xx"),
                    ],
                );
            }
            anyhow::bail!(
                "{} {} failed ({status}): {text}",
                method.as_str(),
                url.path()
            );
        }
    }

    /// Fetches the client key share for a specific integration.
    ///
    /// This is a one-time handoff; the key is deleted from the backend's
    /// temporary storage (Redis) after retrieval.
    pub async fn fetch_client_key(&self, integration_id: &str, bearer_jwt: &str) -> Result<String> {
        let id = integration_id.trim();
        anyhow::ensure!(
            !id.is_empty() && id.len() == 24,
            "integrationId must be a 24-char hex id"
        );
        let value = self
            .authed_json(
                bearer_jwt,
                Method::POST,
                &format!("auth/integrations/{id}/client-key"),
                None,
            )
            .await
            .context("fetch client key")?;
        let client_key = value
            .get("clientKey")
            .and_then(|k| k.as_str())
            .context("missing clientKey in response")?;
        Ok(client_key.to_string())
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;

mod channel;
