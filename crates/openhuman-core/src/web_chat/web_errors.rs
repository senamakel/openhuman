//! Turning a flattened inference/agent-loop error string into a stable,
//! user-facing chat error: budget/quota copy, timeout markers, provider
//! detail extraction, retry-after parsing, and the classification ladder
//! that ties them together.

mod backend_error_code;
mod budget;
mod classify;
mod provider_detail;
mod response_predicates;
mod retry;
mod timeout;

pub(crate) use budget::{
    generic_inference_error_user_message, inference_budget_exceeded_user_message,
    is_action_budget_exhausted, is_inference_budget_exceeded_error,
};
pub(crate) use classify::{classify_inference_error, ClassifiedError};
pub(crate) use provider_detail::{
    extract_provider_error_detail, extract_provider_name, is_fallback_chain_exhausted,
    with_provider_detail,
};
pub(crate) use response_predicates::is_empty_provider_response_text;
pub(crate) use retry::{
    is_non_retryable_rate_limit_text, parse_retry_after_secs_from_str, retry_after_hint,
};
pub(crate) use timeout::{
    is_outer_backstop_timeout, is_turn_timeout_error, turn_timeout_error_message,
};
