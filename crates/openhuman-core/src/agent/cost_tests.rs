use super::*;

fn usage(input: u64, output: u64, cached: u64, charged: f64) -> BilledUsage {
    BilledUsage::from_counts(input, output)
        .with_cached_input_tokens(cached)
        .with_charged_usd(charged)
}

const FLASH: &str = crate::config::MODEL_MANAGED_DEFAULT;

#[test]
fn lookup_pricing_prices_the_managed_default() {
    let p = lookup_pricing(FLASH);
    assert_eq!(p.model, FLASH);
    assert_eq!(p.input_per_mtok_usd, 0.0886);
    assert_eq!(p.output_per_mtok_usd, 0.1772);
}

#[test]
fn lookup_pricing_maps_role_aliases_and_retired_tiers_onto_the_default() {
    // Old cost records and `hint:*` routes ran on the managed default; they
    // must price from its row, not via the $3/$15 fallback, which would inflate
    // cost and could trip budget gates.
    for model in [
        "hint:chat",
        "hint:burst",
        "burst-v1",
        "vision-v1",
        "reasoning-v1",
    ] {
        let p = lookup_pricing(model);
        assert_eq!(p.model, FLASH, "{model} must price as the managed default");
    }
}

#[test]
fn lookup_pricing_falls_back_for_unknown_model() {
    let p = lookup_pricing("totally-unknown-model");
    assert_eq!(p.model, "<fallback>");
}

#[test]
fn lookup_pricing_handles_concrete_vendor_names() {
    // A catalog id prices from the vendor catalog; a dotted, non-catalog
    // vendor name no longer guesses a tier and takes the conservative fallback.
    assert_eq!(lookup_pricing("claude-opus-4.7").model, "<fallback>");
    assert_eq!(
        lookup_pricing("claude-sonnet-4-6").output_per_mtok_usd,
        15.0
    );
}

#[test]
fn estimate_call_cost_subtracts_cached_input() {
    // 1M standard input + 1M cached input + 1M output on the managed default.
    let u = usage(2_000_000, 1_000_000, 1_000_000, 0.0);
    let est = estimate_call_cost_usd(FLASH, &u);
    // 1M*0.0886 + 1M*0.0886 + 1M*0.1772 = 0.3544
    assert!((est - 0.3544).abs() < 1e-6, "got {est}");
}

#[test]
fn call_cost_prefers_charged_when_present() {
    let u = usage(100_000, 200_000, 0, 0.42);
    assert_eq!(call_cost_usd(FLASH, &u), 0.42);
}

#[test]
fn call_cost_falls_back_to_estimate_when_charged_zero() {
    let u = usage(1_000_000, 0, 0, 0.0);
    // 1M input * 0.0886 = 0.0886
    assert!((call_cost_usd(FLASH, &u) - 0.0886).abs() < 1e-6);
}

#[test]
fn turn_cost_accumulates_charged_and_estimated_separately() {
    let mut tc = TurnCost::new();
    tc.add_call(FLASH, &usage(0, 0, 0, 0.10));
    tc.add_call(FLASH, &usage(1_000_000, 0, 0, 0.0)); // est: 0.0886
    assert_eq!(tc.call_count, 2);
    assert!((tc.charged_usd - 0.10).abs() < 1e-6);
    assert!((tc.estimated_usd - 0.0886).abs() < 1e-6);
    assert!((tc.total_usd() - 0.1886).abs() < 1e-6);
}

#[test]
fn turn_cost_aggregates_token_counts() {
    let mut tc = TurnCost::new();
    tc.add_call(FLASH, &usage(100, 50, 20, 0.0));
    tc.add_call(FLASH, &usage(200, 75, 0, 0.0));
    assert_eq!(tc.input_tokens, 300);
    assert_eq!(tc.output_tokens, 125);
    assert_eq!(tc.cached_input_tokens, 20);
}

#[test]
fn free_models_are_priced_at_zero() {
    // OpenRouter `:free` variants are not billed; the catalog would otherwise
    // price `deepseek/...:free` like its paid sibling, or the fallback at $3/$15.
    for model in [
        "deepseek/deepseek-chat-v3.1:free",
        "openrouter/meta-llama/llama-3.3-70b-instruct:free",
        "Totally-Unknown-Model:FREE",
    ] {
        assert!(is_free_model(model), "{model}");
        let usage = usage(1_000_000, 1_000_000, 0, 0.0);
        assert_eq!(estimate_call_cost_usd(model, &usage), 0.0, "{model}");
        assert_eq!(estimate_known_call_cost_usd(model, &usage), Some(0.0));
        assert_eq!(call_cost_usd(model, &usage), 0.0);
    }
    assert!(!is_free_model("deepseek/deepseek-chat"));
}

#[test]
fn unknown_model_has_no_known_price() {
    assert!(lookup_known_pricing("totally-unknown-model").is_none());
    assert_eq!(
        estimate_known_call_cost_usd("totally-unknown-model", &usage(1_000, 100, 0, 0.0)),
        None,
        "reporting must not fabricate a cost for an unpriced model"
    );
    // Budget enforcement still sees the conservative fallback.
    assert!(estimate_call_cost_usd("totally-unknown-model", &usage(1_000, 100, 0, 0.0)) > 0.0);
}

#[test]
fn known_model_estimate_matches_budget_estimate() {
    let usage = usage(10_000, 2_000, 4_000, 0.0);
    assert_eq!(
        estimate_known_call_cost_usd(FLASH, &usage),
        Some(estimate_call_cost_usd(FLASH, &usage))
    );
}
