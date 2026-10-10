use super::*;

const CLASSIFIED_NOTE: &str = "Stopping after 2 attempt(s): failure class \
    `uncertain_side_effect` still blocks operation `web_answer_tool` on \
    `https://example.com/private?q=secret`. Resolve this blocker before retrying.";

#[test]
fn a_classified_breaker_note_yields_its_class_and_operation_but_not_its_scope() {
    let stop = TurnStop::breaker(CLASSIFIED_NOTE);
    assert_eq!(stop.kind, TurnStopKind::Breaker);
    assert_eq!(stop.failure_class.as_deref(), Some("uncertain_side_effect"));
    assert_eq!(stop.operation.as_deref(), Some("web_answer_tool"));
    let message = stop.status_message();
    assert_eq!(
        message,
        "stopped: breaker uncertain_side_effect on web_answer_tool"
    );
    assert!(!message.contains("example.com"), "the scope never leaks");
}

#[test]
fn an_unclassified_breaker_note_falls_back_to_its_stop_reason_kind() {
    let stop = TurnStop::breaker(
        "The last 6 tool calls in a row failed. Last error: token sk-live-123 rejected",
    );
    assert_eq!(stop.kind, TurnStopKind::Breaker);
    assert_eq!(stop.failure_class.as_deref(), Some("varied_failures"));
    assert_eq!(stop.operation, None);
    assert_eq!(stop.status_message(), "stopped: breaker varied_failures");
}

#[test]
fn identifiers_that_are_not_plain_names_are_dropped() {
    let stop = TurnStop::breaker(
        "Stopping after 1 attempt(s): failure class `permission` still blocks operation \
         `rm -rf /home/alice` on `x`.",
    );
    assert_eq!(stop.failure_class.as_deref(), Some("permission"));
    assert_eq!(stop.operation, None, "free text must not reach a trace");
}

#[test]
fn classify_prefers_breaker_then_wind_down_then_cap() {
    assert_eq!(TurnStop::classify(None, false, false), None);
    assert_eq!(
        TurnStop::classify(None, false, true),
        Some(TurnStop::iteration_cap())
    );
    assert_eq!(
        TurnStop::classify(None, true, true),
        Some(TurnStop::wind_down()),
        "a wind-down pause can read as a cap; the wind-down is the cause"
    );
    assert_eq!(
        TurnStop::classify(Some(CLASSIFIED_NOTE), true, true).map(|s| s.kind),
        Some(TurnStopKind::Breaker)
    );
}

#[test]
fn an_incomplete_reason_maps_to_cap_or_breaker() {
    assert_eq!(
        TurnStop::from_incomplete_reason(SUBAGENT_ITERATION_CAP_REASON),
        TurnStop::iteration_cap()
    );
    assert_eq!(
        TurnStop::from_incomplete_reason(CLASSIFIED_NOTE)
            .operation
            .as_deref(),
        Some("web_answer_tool")
    );
    assert_eq!(TurnStop::wind_down().status_message(), "stopped: wind_down");
    assert_eq!(
        TurnStop::iteration_cap().status_message(),
        "stopped: iteration_cap"
    );
}
