use super::*;

#[test]
fn transient_with_attempts_left_retries() {
    assert_eq!(
        classify(1, MAX_DOWNLOAD_ATTEMPTS, true),
        RetryDecision::Retry
    );
    assert_eq!(
        classify(2, MAX_DOWNLOAD_ATTEMPTS, true),
        RetryDecision::Retry
    );
}

#[test]
fn transient_on_last_attempt_gives_up() {
    assert_eq!(
        classify(MAX_DOWNLOAD_ATTEMPTS, MAX_DOWNLOAD_ATTEMPTS, true),
        RetryDecision::GiveUp
    );
}

#[test]
fn transient_past_budget_gives_up() {
    assert_eq!(
        classify(MAX_DOWNLOAD_ATTEMPTS + 1, MAX_DOWNLOAD_ATTEMPTS, true),
        RetryDecision::GiveUp
    );
}

#[test]
fn non_transient_never_retries() {
    // Fatal on the very first attempt, regardless of remaining budget.
    assert_eq!(
        classify(1, MAX_DOWNLOAD_ATTEMPTS, false),
        RetryDecision::GiveUp
    );
    assert_eq!(
        classify(2, MAX_DOWNLOAD_ATTEMPTS, false),
        RetryDecision::GiveUp
    );
}

#[test]
fn single_attempt_budget_never_retries() {
    // max == 1 means no retries even for a transient error.
    assert_eq!(classify(1, 1, true), RetryDecision::GiveUp);
}

#[test]
fn backoff_is_monotonic_and_bounded() {
    let b1 = backoff_for(1);
    let b2 = backoff_for(2);
    assert!(b2 > b1, "backoff must grow with attempt");
    assert_eq!(b1, Duration::from_secs(2));
    // Worst-case wait across the full budget stays small (2s + 4s = 6s).
    let total: Duration = (1..MAX_DOWNLOAD_ATTEMPTS).map(backoff_for).sum();
    assert!(
        total <= Duration::from_secs(10),
        "total backoff stays bounded"
    );
}
