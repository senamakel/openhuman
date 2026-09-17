//! User-facing copy and detection for OpenHuman's own budget/quota limits —
//! inference credits and the SecurityPolicy per-hour action-budget cap.

use once_cell::sync::Lazy;
use regex::Regex;

static BUDGET_ERROR_NORMALIZE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[-_\s]+").expect("budget normalize regex"));
static BUDGET_ERROR_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"budget.*exceed").expect("budget exceeded regex"),
        Regex::new(r"top up").expect("top up regex"),
        Regex::new(r"add.*credits").expect("add credits regex"),
        Regex::new(r"out of credits").expect("out of credits regex"),
        Regex::new(r"no remaining credits").expect("no remaining credits regex"),
    ]
});

pub(crate) fn is_inference_budget_exceeded_error(message: &str) -> bool {
    let normalized = BUDGET_ERROR_NORMALIZE_RE
        .replace_all(&message.trim().to_ascii_lowercase(), " ")
        .into_owned();
    if BUDGET_ERROR_PATTERNS
        .iter()
        .any(|pattern| pattern.is_match(&normalized))
    {
        return true;
    }
    // Align with the canonical OpenHuman-backend budget detector
    // (`billing_error::is_budget_exhausted_message`) so the managed
    // no-credits response — a 400 carrying "Insufficient budget" /
    // "Insufficient balance" — surfaces the actionable budget message
    // below instead of the generic "Something went wrong" apology
    // (issue #3088). Without this, an Ollama user with zero credits and
    // routing still on Managed sees an opaque "provider error" and has no
    // way to self-diagnose that they must top up or switch routing.
    crate::inference::provider::is_budget_exhausted_message(message)
}

pub(crate) fn inference_budget_exceeded_user_message() -> &'static str {
    // Keep the literal "top up" / "credits" tokens (asserted by
    // `budget_exceeded_copy_mentions_top_up`) and add the self-diagnosis
    // path for issue #3088: a user who enabled a local model but left
    // routing on Managed needs to know they can switch to their own model
    // rather than being stuck. We guide, never auto-switch — the user's
    // routing choice in Settings is respected.
    "You're out of credits, so I can't run the managed (cloud) model right now. \
     You can top up your credits or pick a plan to continue — or, if you've enabled a \
     local model like Ollama, switch routing to \"Use Your Own Models\" in Connections → API keys → LLM."
}

pub(crate) fn generic_inference_error_user_message() -> &'static str {
    "Something went wrong. Please try again.\nThis error has been reported. You can also report it on Discord.\n<openhuman-link path=\"community/discord-report\">Report on Discord</openhuman-link>"
}

/// Detect the SecurityPolicy global hourly action-budget signal
/// emitted by the built-in tools (`web_fetch`, `curl`, `http_request`,
/// `composio`, etc.) — see `crates/openhuman-core/src/security/
/// policy.rs::SecurityPolicy::is_rate_limited`.
///
/// We match the canonical English strings those tools emit. This is
/// load-bearing for issue #2364: before this check ran, any string
/// containing "rate limit" was misclassified as a provider 429 and
/// the user saw the generic "You're being rate-limited" copy, which
/// hides that the cap is OpenHuman's own per-hour safety budget,
/// not the upstream LLM provider.
pub(crate) fn is_action_budget_exhausted(err_lower: &str) -> bool {
    err_lower.contains("rate limit exceeded: action budget exhausted")
        || err_lower.contains("rate limit exceeded: too many actions in the last hour")
        || err_lower.contains("action blocked: rate limit exceeded")
}
