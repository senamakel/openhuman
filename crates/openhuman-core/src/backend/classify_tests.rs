use super::*;

#[test]
fn detects_known_budget_exhaustion_phrases_case_insensitively() {
    for message in [
        "INSUFFICIENT BUDGET",
        "budget EXCEEDED — ADD credits",
        "Insufficient BALANCE",
        "You have no remaining credits to use the LLM apis.",
        "Your CREDIT BALANCE IS TOO LOW to access the Anthropic API",
    ] {
        assert!(is_budget_exhausted_message(message), "{message:?}");
    }
}

#[test]
fn ignores_non_budget_messages() {
    for message in [
        "Bad request: missing field",
        "You have 100 remaining credits this month",
        "Your credit balance is $50.00",
        "",
    ] {
        assert!(!is_budget_exhausted_message(message), "{message:?}");
    }
}
