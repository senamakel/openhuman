use super::*;
use crate::agent::harness::definition::{
    AgentDefinition, DefinitionSource, ModelSpec, PromptSource, SandboxMode, ToolScope,
};

fn dummy_definition() -> AgentDefinition {
    AgentDefinition {
        id: "summarizer".into(),
        when_to_use: "test".into(),
        display_name: Some("Summarizer".into()),
        system_prompt: PromptSource::Inline("test prompt".into()),
        omit_identity: true,
        omit_memory_context: true,
        omit_safety_preamble: true,
        omit_profile: true,
        omit_memory_md: true,
        model: ModelSpec::Hint("summarization".into()),
        temperature: 0.2,
        tools: ToolScope::Named(vec![]),
        disallowed_tools: vec![],
        skill_filter: None,
        extra_tools: vec![],
        max_iterations: 1,
        iteration_policy: Default::default(),
        max_result_chars: None,
        max_turn_output_tokens: None,
        timeout_secs: None,
        sandbox_mode: SandboxMode::None,
        background: false,
        trigger_memory_agent: Default::default(),
        tokenjuice_compression: crate::inference::tokenjuice::AgentTokenjuiceCompression::Auto,
        subagents: vec![],
        delegate_name: None,
        agent_tier: crate::agent::harness::definition::AgentTier::Worker,
        source: DefinitionSource::Builtin,
        graph: Default::default(),
    }
}

// Tests use the production-default thresholds expressed as tokens:
// 500 000 tokens lower bound, 2 000 000 tokens upper bound.
// Since estimate_tokens = chars / 4, 1 char ≈ 0.25 tokens.
const TEST_THRESHOLD_TOKENS: usize = 500_000;
const TEST_MAX_TOKENS: usize = 2_000_000;

fn dummy_parent_ctx() -> RunContext<()> {
    RunContext::new(tinyagents_harness::context::RunConfig::new("test"), ())
}

#[tokio::test]
async fn below_threshold_is_not_needed_not_unavailable() {
    let summarizer =
        SubagentPayloadSummarizer::new(dummy_definition(), TEST_THRESHOLD_TOKENS, TEST_MAX_TOKENS);
    // 1 KB of 'x' → ~256 tokens, well below the 500 000 threshold.
    let raw = "x".repeat(1_024);
    let outcome = summarizer
        .maybe_summarize_in_parent(&dummy_parent_ctx(), "test_tool", None, &raw)
        .await
        .expect("below-threshold check should not error");
    assert!(
        matches!(outcome, SummarizeOutcome::NotNeeded),
        "a payload below the threshold needed nothing, so the model must be \
         told nothing; got {outcome:?}"
    );
}

#[tokio::test]
async fn above_max_cap_is_disclosed_as_unavailable() {
    let summarizer =
        SubagentPayloadSummarizer::new(dummy_definition(), TEST_THRESHOLD_TOKENS, TEST_MAX_TOKENS);
    // 9 MB of 'x' → ~2 359 296 tokens, above the 2 000 000 cap.
    let raw = "x".repeat(9 * 1024 * 1024);
    let outcome = summarizer
        .maybe_summarize_in_parent(&dummy_parent_ctx(), "test_tool", None, &raw)
        .await
        .expect("above-cap check should not error");
    // The regression guard. This used to be `outcome.is_none()`, the same
    // value the below-threshold case returns — which is exactly how a
    // payload that will be truncated downstream reached the model looking
    // like ordinary output.
    assert!(
        matches!(
            outcome,
            SummarizeOutcome::Unavailable(UnavailableReason::PayloadTooLarge)
        ),
        "a payload over the cap is not summarized and will be truncated, so \
         it must be disclosed, not passed through silently; got {outcome:?}"
    );
}

#[tokio::test]
async fn tripped_breaker_is_disclosed_as_unavailable() {
    let summarizer =
        SubagentPayloadSummarizer::new(dummy_definition(), TEST_THRESHOLD_TOKENS, TEST_MAX_TOKENS);
    // Manually trip the breaker by recording 3 failures.
    summarizer.record_failure();
    summarizer.record_failure();
    summarizer.record_failure();
    assert!(summarizer.breaker_tripped(), "breaker should be tripped");

    // 3 MB of 'x' → ~786 432 tokens: inside the [500k, 2M] summarize
    // window, so would normally dispatch — but breaker is tripped.
    let raw = "x".repeat(3 * 1024 * 1024);
    let outcome = summarizer
        .maybe_summarize_in_parent(&dummy_parent_ctx(), "test_tool", None, &raw)
        .await
        .expect("breaker check should not error");
    assert!(
        matches!(
            outcome,
            SummarizeOutcome::Unavailable(UnavailableReason::Disabled)
        ),
        "a tripped breaker must short-circuit before any dispatch AND be \
         disclosed — it is the case that repeats most often, because the \
         breaker is rebuilt per session build; got {outcome:?}"
    );
}

#[test]
fn every_unavailable_notice_says_not_to_re_run_without_predicting_the_future() {
    // The whole point of the notice. A model handed a truncated dump with
    // no explanation does the reasonable thing and calls the same tool
    // again, which is the re-dispatch loop a user perceives as a hang.
    //
    // The second half of the name is the part that took two rounds to get
    // right. The instruction has to hold for every variant *without*
    // asserting what a re-run would do, because `Failed` is recorded before
    // the breaker opens and a later attempt can genuinely succeed — so a
    // notice claiming otherwise contradicts the breaker. Both previously
    // shipped phrasings are asserted absent below so neither comes back as
    // a tightening.
    for reason in [
        UnavailableReason::PayloadTooLarge,
        UnavailableReason::Disabled,
        UnavailableReason::Failed,
    ] {
        let notice = reason.notice();
        assert!(
            notice.contains("Do not re-run the tool for a summary"),
            "{reason:?} must tell the model not to retry: {notice}"
        );
        for prediction in ["will return the same result", "will not produce a summary"] {
            assert!(
                !notice.contains(prediction),
                "{reason:?} must not predict what a re-run would do ({prediction:?}): \
                 {notice}"
            );
        }
        assert!(
            notice.starts_with("[openhuman: summarization unavailable"),
            "{reason:?} must be greppable and self-identifying: {notice}"
        );
    }
}

#[test]
fn the_three_notices_are_distinct() {
    // Three different situations; a reader (human or model) should be able
    // to tell which one happened.
    let all = [
        UnavailableReason::PayloadTooLarge.notice(),
        UnavailableReason::Disabled.notice(),
        UnavailableReason::Failed.notice(),
    ];
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            assert_ne!(a, b, "each reason needs its own notice");
        }
    }
}

#[test]
fn build_summarizer_prompt_includes_tool_name_and_hint() {
    let prompt = build_summarizer_prompt(
        "GITHUB_LIST_REPOSITORY_ISSUES",
        Some("find the most urgent open issues"),
        "{\"issues\": [{\"id\": 1}]}",
    );
    assert!(prompt.contains("GITHUB_LIST_REPOSITORY_ISSUES"));
    assert!(prompt.contains("find the most urgent open issues"));
    assert!(prompt.contains("Parent task hint:"));
    assert!(prompt.contains("--- BEGIN ---"));
    assert!(prompt.contains("--- END ---"));
    assert!(prompt.contains("{\"issues\": [{\"id\": 1}]}"));
}

#[test]
fn build_summarizer_prompt_omits_hint_when_none() {
    let prompt = build_summarizer_prompt("file_read", None, "log line 1\nlog line 2");
    assert!(prompt.contains("file_read"));
    assert!(prompt.contains("--- BEGIN ---"));
    assert!(prompt.contains("--- END ---"));
    assert!(prompt.contains("log line 1"));
    assert!(
        !prompt.contains("Parent task hint:"),
        "no hint line should be present when hint is None"
    );
}

#[test]
fn record_success_resets_breaker() {
    let summarizer =
        SubagentPayloadSummarizer::new(dummy_definition(), TEST_THRESHOLD_TOKENS, TEST_MAX_TOKENS);
    summarizer.record_failure();
    summarizer.record_failure();
    assert!(!summarizer.breaker_tripped());
    summarizer.record_success();
    // Even one more failure now should not trip — counter was reset.
    summarizer.record_failure();
    assert!(!summarizer.breaker_tripped());
}

// ── summary reuse and the real payload size (#6283) ─────────────────────

fn low_threshold_summarizer() -> SubagentPayloadSummarizer {
    SubagentPayloadSummarizer::new(dummy_definition(), 1, TEST_MAX_TOKENS)
}

#[tokio::test]
async fn an_identical_payload_reuses_the_earlier_summary_instead_of_dispatching() {
    let raw = "identical payload for the reuse test ".repeat(64);
    let hint = Some("reuse-test goal");
    remember_summary(
        summary_cache_key("reuse_tool", hint, &raw),
        "CACHED SUMMARY".to_string(),
    );

    // No parent execution context is installed, so a real dispatch fails and
    // reports `Unavailable`; only reuse can produce a summary here.
    let outcome = low_threshold_summarizer()
        .maybe_summarize_in_parent(&dummy_parent_ctx(), "reuse_tool", hint, &raw)
        .await
        .expect("summarization never errors here");

    match outcome {
        SummarizeOutcome::Summarized(payload) => {
            assert_eq!(payload.summary, "CACHED SUMMARY");
            assert_eq!(payload.original_bytes, raw.len());
        }
        other => panic!(
            "an identical payload must reuse the stored summary instead of dispatching \
             the summarizer again; got {other:?}"
        ),
    }
}

#[tokio::test]
async fn a_summary_written_for_another_goal_is_not_reused() {
    let raw = "payload shared across two goals ".repeat(64);
    remember_summary(
        summary_cache_key("goal_tool", Some("goal A"), &raw),
        "SUMMARY FOR GOAL A".to_string(),
    );

    let outcome = low_threshold_summarizer()
        .maybe_summarize_in_parent(&dummy_parent_ctx(), "goal_tool", Some("goal B"), &raw)
        .await
        .expect("summarization never errors here");

    assert!(
        !matches!(outcome, SummarizeOutcome::Summarized(_)),
        "a summary shaped for one goal must not be served for another; got {outcome:?}"
    );
}

#[test]
fn a_successful_summary_is_remembered_for_reuse() {
    let summarizer = low_threshold_summarizer();
    let raw = "x".repeat(50_000);
    let key = summary_cache_key("size_tool", Some("size goal"), &raw);

    let outcome = summarizer
        .handle_summarizer_result(
            "size_tool",
            &raw,
            std::time::Instant::now(),
            Ok("model note about the payload".to_string()),
            key,
        )
        .expect("a usable summary is not an error");

    let SummarizeOutcome::Summarized(payload) = outcome else {
        panic!("a non-empty, smaller summary must be accepted");
    };
    assert_eq!(
        payload.original_bytes,
        raw.len(),
        "the summarized size handed to the middleware must be the real byte count"
    );
    assert_eq!(
        cached_summary(&key).as_deref(),
        Some(payload.summary.as_str()),
        "a successful summary must be stored for reuse"
    );
}

#[test]
fn build_summarizer_prompt_states_the_real_byte_count() {
    let raw = "abc".repeat(1_000);
    let prompt = build_summarizer_prompt("size_tool", None, &raw);
    assert!(
        prompt.contains("Raw tool output: 3000 bytes, complete"),
        "the summarizer must be told the payload's exact size, got: {prompt}"
    );
}

#[test]
fn a_long_task_hint_keeps_its_trailing_request() {
    let hint = format!(
        "{}\nfind the authentication failure",
        "log line ".repeat(1_000)
    );
    let prompt = build_summarizer_prompt("hint_tool", Some(&hint), "payload");
    assert!(
        prompt.contains("find the authentication failure"),
        "the request at the end of a long message must survive clipping"
    );
    assert!(
        prompt.contains("log line"),
        "the start of the message is kept too"
    );
    assert!(
        prompt.len() < hint.len(),
        "the hint must still be bounded, got a {}-byte prompt",
        prompt.len()
    );
}
