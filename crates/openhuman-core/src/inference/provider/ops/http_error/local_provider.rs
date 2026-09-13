//! Local inference server (LM Studio / Ollama) user-state classification:
//! no model loaded, and Ollama Cloud's opaque hosted-inference 500.

/// Whether a provider non-2xx response is a local inference server that is
/// running but has **no model loaded** (e.g. LM Studio idle): a 400 carrying
/// `No models loaded. Please load a model …`.
///
/// This is pure local user-state — nothing OpenHuman sent is malformed, there
/// is no product bug and no local lever beyond the user loading a model — so it
/// should be demoted from Sentry to an info log rather than paging on every
/// retry (TAURI-RUST-DMQ: 5,469 events from a single idle LM Studio server).
/// The embeddings path already special-cases this exact string
/// (`embeddings/rpc.rs`, PR #3688 / TAURI-RUST-4P4); this is the chat sibling.
pub fn is_local_provider_no_model_loaded(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST
        && body.to_ascii_lowercase().contains("no models loaded")
}

/// Actionable user-facing guidance for a local inference server with no model
/// loaded, mirroring the embeddings verification message
/// (`embeddings/rpc.rs`). Returned in place of the raw provider body so the
/// surfaced error tells the user how to fix it.
pub fn local_provider_no_model_loaded_user_message() -> String {
    "Your local inference server (e.g. LM Studio) is running but has no model loaded. \
     Load a model — in LM Studio use the developer page or the `lms load` command — \
     then try again."
        .to_string()
}

pub fn log_local_provider_no_model_loaded(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "provider_user_state",
        reason = "local_provider_no_model_loaded",
        "[llm_provider] {operation} local inference server has no model loaded — \
         user must load a model, not reporting to Sentry"
    );
}

/// Stable anchor phrase for the actionable Ollama-Cloud-500 user message, shared
/// by [`ollama_cloud_internal_500_user_message`] (which builds it) and
/// [`is_ollama_cloud_internal_500_message`] (which matches the re-raised string
/// at the RPC/agent boundary), so the two cannot drift.
const OLLAMA_CLOUD_INTERNAL_500_USER_PREFIX: &str = "Ollama cloud is temporarily unavailable";

/// Whether a provider non-2xx response is an Ollama **Cloud** hosted-inference
/// internal error: `500` + a `{"error":"Internal Server Error (ref: <uuid>)"}`
/// body.
///
/// ollama.com's hosted `*:cloud` models (minimax-m3 / qwen3.5 / gpt-oss …)
/// intermittently `500` with this opaque, server-generated envelope. The `ref:`
/// is a fresh UUID per event, the failure is non-deterministic, and the request
/// that 500s is byte-identical to the one that succeeds when the cloud backend
/// is healthy — so there is **no client lever** (nothing to validate,
/// reshape, or reconfigure). The reliable-provider layer already retries and
/// falls back across providers/models, so each per-attempt 500 is pure noise:
/// TAURI-RUST-5MV, 3,062 events from 5 users in a single window. Demote from
/// Sentry to an info log while the error still propagates so retry/fallback runs
/// unchanged.
///
/// Anchored on the `internal server error (ref:` body shape, which is specific
/// to ollama.com's hosted envelope — a **local** Ollama daemon 500 (a genuine
/// model crash / OOM worth paging) does not carry a `ref:` UUID, so it still
/// reaches Sentry. The phrase is covered by a verbatim-body test so a provider
/// wording drift fails CI instead of silently leaking events.
pub fn is_ollama_cloud_internal_500(
    provider: &str,
    status: reqwest::StatusCode,
    body: &str,
) -> bool {
    provider == "ollama"
        && status == reqwest::StatusCode::INTERNAL_SERVER_ERROR
        && body
            .to_ascii_lowercase()
            .contains("internal server error (ref:")
}

/// Message-level half of [`is_ollama_cloud_internal_500`]: matches the actionable
/// user message re-raised at the RPC/agent boundary
/// (`core::observability::expected_error_kind`), so the higher-layer re-report is
/// demoted too instead of leaking the event the emit-site already suppressed (the
/// `domain=agent` half of TAURI-RUST-5MV). Mirrors the
/// `is_provider_insufficient_credits_402` / `body_indicates_insufficient_credits`
/// split. Keyed on the [`OLLAMA_CLOUD_INTERNAL_500_USER_PREFIX`] anchor, which we
/// own, so it cannot collide with an unrelated provider body.
pub fn is_ollama_cloud_internal_500_message(message: &str) -> bool {
    let needle = OLLAMA_CLOUD_INTERNAL_500_USER_PREFIX.to_ascii_lowercase();
    message.to_ascii_lowercase().contains(needle.as_str())
}

/// Build the actionable user-facing message for an Ollama-Cloud hosted-inference
/// 500, replacing the opaque `Internal Server Error (ref: <uuid>)` body (which
/// carries no signal the user can act on) with retry/switch guidance. The model
/// is included when known (native/streaming chat); the `api_error` path has no
/// model in scope and omits it.
pub fn ollama_cloud_internal_500_user_message(
    model: Option<&str>,
    status: reqwest::StatusCode,
) -> String {
    let code = status.as_u16();
    match model {
        Some(model) => format!(
            "{OLLAMA_CLOUD_INTERNAL_500_USER_PREFIX} for model `{model}` (Ollama returned HTTP \
             {code}); the hosted model failed on Ollama's side — retry shortly or switch models."
        ),
        None => format!(
            "{OLLAMA_CLOUD_INTERNAL_500_USER_PREFIX} (Ollama returned HTTP {code}); the hosted \
             model failed on Ollama's side — retry shortly or switch models."
        ),
    }
}

pub fn log_ollama_cloud_internal_500(
    operation: &str,
    provider: &str,
    model: Option<&str>,
    status: reqwest::StatusCode,
) {
    tracing::info!(
        domain = "llm_provider",
        operation = operation,
        provider = provider,
        model = model.unwrap_or(""),
        status = status.as_u16(),
        failure = "non_2xx",
        kind = "ollama_cloud_internal_500",
        "[llm_provider] {operation} Ollama Cloud hosted-inference 500 — provider-internal \
         (no client lever), not reporting to Sentry"
    );
}
