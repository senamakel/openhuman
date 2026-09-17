//! HTTP error classification, Sentry-report demotion, and `api_error`.
//!
//! Sub-modules, by responsibility:
//! - [`quota_and_credits`] — budget-exhausted, insufficient-credits, monthly
//!   quota exhaustion, and per-request rate-cap classification.
//! - [`local_provider`] — local server "no model loaded" and Ollama Cloud's
//!   opaque hosted-inference 500.
//! - [`policy_rejection`] — custom-OpenAI upstream envelopes, provider
//!   access-policy denial, backend-owned error codes, moderation rejection,
//!   and provider configuration rejection.
//! - [`context_window`] — context-window overflow classification.
//! - [`auth_failure`] — backend session expiry, BYO API-key rejection, and
//!   OpenAI OAuth session expiry.
//! - [`dispatch`] — `should_report_provider_http_failure` and the `api_error`
//!   entry point that runs a failed response through every classifier above.

#[cfg(test)]
#[path = "http_error_tests.rs"]
mod tests;

mod auth_failure;
mod context_window;
mod dispatch;
mod local_provider;
mod policy_rejection;
mod quota_and_credits;

// Shared imports every classifier submodule reaches through `use super::*;`
// (mirrors the pre-split `include!`-shared scope).
use super::sanitize::sanitize_api_error;
use crate::inference::provider::openhuman_backend_model;

pub use auth_failure::{
    is_backend_auth_failure, is_byo_provider_auth_failure_http,
    is_openai_oauth_session_expired_http, is_openai_oauth_session_expired_message,
    log_byo_provider_auth_failure, log_openai_oauth_session_expired,
    publish_backend_session_expired,
};
pub use context_window::{is_context_window_exceeded_message, log_context_window_exceeded};
pub use dispatch::{api_error, should_report_provider_http_failure};
pub use local_provider::{
    is_local_provider_no_model_loaded, is_ollama_cloud_internal_500,
    is_ollama_cloud_internal_500_message, local_provider_no_model_loaded_user_message,
    log_local_provider_no_model_loaded, log_ollama_cloud_internal_500,
    ollama_cloud_internal_500_user_message,
};
pub use policy_rejection::{
    is_backend_error_code_owned, is_custom_openai_upstream_bad_request_http_400,
    is_provider_access_policy_denied_http_403, is_provider_config_rejection_http,
    is_provider_moderation_rejection_http_400, log_backend_error_code_owned,
    log_custom_openai_upstream_bad_request_http_400, log_provider_access_policy_denied_http_403,
    log_provider_config_rejection, log_provider_moderation_rejection,
};
pub use quota_and_credits::{
    body_indicates_insufficient_credits, body_indicates_quota_exhausted,
    is_budget_exhausted_http_400, is_provider_insufficient_credits_402,
    is_provider_quota_exhausted, is_provider_rate_cap_exceeded_message,
    log_budget_exhausted_http_400, log_provider_insufficient_credits_402,
    log_provider_quota_exhausted,
};
