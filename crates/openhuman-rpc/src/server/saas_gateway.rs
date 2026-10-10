//! The SaaS gateway layer: which context each request runs under.
//!
//! Replaces the single-context layer when the process runs in SaaS mode. For
//! every request it:
//!
//! 1. answers `404` for routes a SaaS core never serves — the OpenAI-compatible
//!    `/v1`, the `/events/*` debug streams, the WebSockets, `/dev/connect` and
//!    the MCP OAuth callback — and for the `/events` chat stream outside a
//!    user's scope;
//! 2. with no `X-OpenHuman-User`, runs it on the operator plane (the bearer
//!    check downstream still applies);
//! 3. with one, checks the service bearer **first** — so an unauthenticated
//!    caller learns nothing about which users exist and cannot open profiles —
//!    then the signature, then runs the request under that user's profile.
//!
//! The decision itself lives in `crate::core_host::profiles::gateway`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core_host::core::runtime::CoreContext;
use crate::core_host::profiles::gateway::{
    resolve_scope, GatewayRefusal, GatewayScope, USER_HEADER, USER_SIG_HEADER,
};
use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Route prefixes a SaaS core never serves.
pub(crate) const CLOSED_IN_SAAS: &[&str] = &[
    "/v1",
    "/events/",
    "/ws/",
    "/socket.io",
    "/dev/connect",
    "/oauth/",
];

/// A prefix ending in `/` closes only what lies beneath it; any other prefix
/// closes the path itself and everything beneath it.
pub(crate) fn is_closed_in_saas(path: &str) -> bool {
    CLOSED_IN_SAAS.iter().any(|prefix| {
        if prefix.ends_with('/') {
            path.starts_with(prefix)
        } else {
            path == *prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    })
}

fn refuse(status: u16, message: &str) -> GatewayRefusal {
    GatewayRefusal {
        status,
        message: message.to_string(),
    }
}

fn refusal_response(refusal: GatewayRefusal) -> Response {
    let status = StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::FORBIDDEN);
    (
        status,
        axum::Json(serde_json::json!({ "error": refusal.message })),
    )
        .into_response()
}

fn header_str<'a>(req: &'a Request, name: &str) -> Option<&'a str> {
    req.headers().get(name).and_then(|v| v.to_str().ok())
}

fn bearer(req: &Request) -> Option<&str> {
    header_str(req, header::AUTHORIZATION.as_str())?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

/// The SaaS request layer; `operator` is the runtime's own context.
pub(crate) async fn saas_gateway(operator: Arc<CoreContext>, req: Request, next: Next) -> Response {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let secret = crate::core_host::core::auth::get_rpc_token();
    match decide(&req, secret, now, resolve_scope) {
        Err(refusal) => refusal_response(refusal),
        Ok(GatewayScope::User(profile)) => {
            let ctx = Arc::clone(profile.context());
            // Holding the state for the request keeps the profile from being
            // evicted under it.
            let response = CoreContext::scope(ctx, next.run(req)).await;
            drop(profile);
            response
        }
        Ok(GatewayScope::Operator) => CoreContext::scope(operator, next.run(req)).await,
    }
}

/// Which scope a request runs under, or the response that refuses it.
///
/// `secret` is the service bearer (`None` before the core has one), `now` is
/// unix seconds and `resolve` maps the checked headers to a scope
/// ([`resolve_scope`] in production). Taking them as arguments keeps the
/// decision free of process-wide state.
// The refusal is the axum `Response` the gateway sends as-is, built once per
// refused request; boxing it would only move the allocation (same as
// `http_host::auth`).
#[allow(clippy::result_large_err)]
pub(crate) fn decide(
    req: &Request,
    secret: Option<&str>,
    now: u64,
    resolve: impl FnOnce(Option<&str>, Option<&str>, &str, u64) -> Result<GatewayScope, GatewayRefusal>,
) -> Result<GatewayScope, GatewayRefusal> {
    let path = req.uri().path();
    if is_closed_in_saas(path) {
        log::debug!("[rpc:saas] {path} is not served in SaaS mode");
        return Err(refuse(404, "not found"));
    }

    // Absent means the operator plane. Present but unreadable, or present
    // twice, is refused: falling back to the operator would skip the user
    // signature check.
    let mut user_headers = req.headers().get_all(USER_HEADER).iter();
    let Some(first) = user_headers.next() else {
        // The chat event stream is a user's; the operator has none.
        if path == "/events" {
            return Err(refuse(404, "not found"));
        }
        return Ok(GatewayScope::Operator);
    };
    if user_headers.next().is_some() {
        log::debug!("[rpc:saas] refusing a request with more than one {USER_HEADER}");
        return Err(refuse(400, "more than one user header"));
    }
    let Ok(user) = first.to_str() else {
        log::debug!("[rpc:saas] refusing an unreadable {USER_HEADER}");
        return Err(refuse(400, "unreadable user header"));
    };

    let Some(secret) = secret else {
        return Err(refuse(503, "the core is not ready"));
    };
    if !bearer(req)
        .is_some_and(|supplied| crate::core_host::core::auth::bearer_matches(supplied, secret))
    {
        return Err(refuse(401, "unauthorized"));
    }
    // Checked after the bearer, so an unauthenticated caller learns nothing.
    // A second signature must not be ignored in favour of the first.
    let mut sig_headers = req.headers().get_all(USER_SIG_HEADER).iter();
    let signature = sig_headers.next();
    if sig_headers.next().is_some() {
        log::debug!("[rpc:saas] refusing a request with more than one {USER_SIG_HEADER}");
        return Err(refuse(400, "more than one user signature header"));
    }
    let signature = signature.and_then(|value| value.to_str().ok());
    resolve(Some(user), signature, secret, now)
}

#[cfg(test)]
#[path = "saas_gateway_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "saas_gateway_proptest_tests.rs"]
mod proptest_tests;
