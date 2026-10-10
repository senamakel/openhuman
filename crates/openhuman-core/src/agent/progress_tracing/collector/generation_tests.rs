use super::*;

#[test]
fn model_label_skips_an_empty_provider() {
    assert_eq!(model_label("managed", "chat-v1"), "managed.chat-v1");
    assert_eq!(
        model_label("", "openrouter/deepseek/deepseek-v4-flash"),
        "openrouter/deepseek/deepseek-v4-flash"
    );
    assert_eq!(model_label("  ", "m"), "m");
}

#[test]
fn resolve_model_falls_back_for_an_empty_name() {
    assert_eq!(resolve_model("gpt-4o", Some("other")), "gpt-4o");
    assert_eq!(resolve_model("", Some("deepseek")), "deepseek");
    assert_eq!(resolve_model(" ", None), "unknown");
}

#[test]
fn prompt_inclusive_usage_is_kept() {
    // deepseek/OpenRouter route: input already counts the cache read.
    let usage = normalize_usage(66_732, 484, 64_768, 0);
    assert!(!usage.widened);
    assert_eq!(usage.input_total, 66_732);
    assert_eq!(usage.input_uncached, 1_964);
    assert_eq!(usage.total, 67_216);
}

#[test]
fn uncached_only_input_is_widened() {
    // Claude Code route: Anthropic's raw `input_tokens` is the uncached
    // remainder, so the reported total used to be smaller than the cache read.
    let usage = normalize_usage(12, 300, 40_000, 2_000);
    assert!(usage.widened);
    assert_eq!(usage.input_total, 42_012);
    assert_eq!(usage.input_uncached, 12);
    assert_eq!(usage.cache_read, 40_000);
    assert_eq!(usage.cache_creation, 2_000);
    assert_eq!(usage.total, 42_312);
    assert_eq!(
        usage.total,
        usage.input_uncached + usage.cache_read + usage.cache_creation + usage.output
    );
}

#[test]
fn unknown_cost_is_absent_and_reported_zero_is_preserved() {
    assert_eq!(effective_cost(None), (None, CostSource::Unpriced));
    assert_eq!(effective_cost(Some(0.5)), (Some(0.5), CostSource::Priced));
    assert_eq!(effective_cost(Some(0.0)), (Some(0.0), CostSource::Priced));
}

#[test]
fn single_delta_at_completion_is_unstreamed() {
    assert!(is_unstreamed(4_990, 1_000, 5_000));
    assert!(!is_unstreamed(1_300, 1_000, 5_000));
    // A fast call is not second-guessed.
    assert!(!is_unstreamed(1_050, 1_000, 1_060));
}
