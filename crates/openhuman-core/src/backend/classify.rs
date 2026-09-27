//! Backend / provider error-body classification shared across domains.
//!
//! Budget exhaustion is reported by the hosted backend and by managed inference
//! providers as a deterministic user state, not a defect; every domain that
//! sees such a body (inference HTTP errors, agent loop guards, the scheduler,
//! web chat, telemetry) classifies it through this one predicate.

/// Return whether a provider message represents deterministic exhausted-budget
/// user state rather than a product defect.
pub fn is_budget_exhausted_message(message: &str) -> bool {
    const PHRASES: &[&str] = &[
        "insufficient budget",
        "budget exceeded",
        "add credits",
        "insufficient balance",
        "no remaining credits",
        "credit balance is too low",
    ];
    let lower = message.to_ascii_lowercase();
    PHRASES.iter().any(|phrase| lower.contains(phrase))
}

#[cfg(test)]
#[path = "classify_tests.rs"]
mod tests;
