use super::*;

#[tokio::test]
async fn per_turn_tool_limit_reaches_the_execution_policy() {
    crate::agent::stop_hooks::with_tool_call_limit(Some(2), async {
        let policy = run_policy_for(10, false);
        assert_eq!(policy.limits.max_tool_calls, 2);
        assert_eq!(policy.limits.max_model_calls, 10);
    })
    .await;
    assert_eq!(run_policy_for(10, false).limits.max_tool_calls, 80);
}

/// #6953: the model reads how long each tool call took, so it can budget the
/// rest of the turn against it.
#[test]
fn tool_results_carry_their_duration() {
    assert!(run_policy_for(10, false).tool_result_durations);
}

#[test]
fn stream_timeouts_are_set_explicitly_with_first_event_off() {
    let mut limits = RunPolicy::default().limits;
    apply_stream_limits(&mut limits, None, None, None);
    assert_eq!(limits.stream_idle_timeout_ms, Some(120_000));
    assert_eq!(limits.stream_first_event_timeout_ms, None);
    assert_eq!(limits.max_consecutive_stream_idle_timeouts, Some(5));
}

#[test]
fn stream_timeout_env_overrides_parse() {
    assert_eq!(parse_stream_idle_timeout_ms(Some("30")), Some(30_000));
    assert_eq!(parse_stream_idle_timeout_ms(Some("0")), None);
    assert_eq!(parse_stream_idle_timeout_ms(Some("junk")), Some(120_000));
    assert_eq!(
        parse_stream_first_event_timeout_ms(Some("45")),
        Some(45_000)
    );
    assert_eq!(parse_stream_first_event_timeout_ms(Some("0")), None);
    assert_eq!(parse_stream_first_event_timeout_ms(None), None);
    assert_eq!(parse_max_consecutive_stream_idle_timeouts(Some("0")), None);
    assert_eq!(
        parse_max_consecutive_stream_idle_timeouts(Some("2")),
        Some(2)
    );
}

#[test]
fn local_providers_get_longer_ceilings_than_hosted_ones() {
    let hosted = run_policy_for_provider(10, false, false);
    let local = run_policy_for_provider(10, false, true);
    // Env overrides win over both defaults, so compare only without one.
    if std::env::var_os("OPENHUMAN_MODEL_CALL_TIMEOUT_SECS").is_none()
        && std::env::var_os("OPENHUMAN_AGENT_TURN_TIMEOUT_SECS").is_none()
    {
        assert_eq!(hosted.limits.max_model_call_ms, Some(900_000));
        assert_eq!(local.limits.max_model_call_ms, Some(3_600_000));
        assert_eq!(hosted.limits.max_wall_clock_ms, Some(3_600_000));
        assert_eq!(local.limits.max_wall_clock_ms, Some(14_400_000));
    }
    assert!(local.limits.max_model_call_ms < local.limits.max_wall_clock_ms);
    // Stream-silence hang detection is identical for both.
    assert_eq!(
        hosted.limits.stream_idle_timeout_ms,
        local.limits.stream_idle_timeout_ms
    );
}

#[test]
fn local_web_backstop_sits_above_the_local_turn_ceiling() {
    let turn_secs = agent_turn_wall_clock_ms_for(true).map(|ms| ms / 1_000);
    let backstop = local_web_turn_backstop_secs();
    assert_eq!(
        backstop,
        turn_secs.map(|s| s + LOCAL_WEB_TURN_BACKSTOP_GRACE_SECS)
    );
}

#[test]
fn hosted_openai_is_not_treated_as_self_hosted() {
    // The bare `openai` name maps onto the generic local OpenAI kind in
    // `tinyinference_local`; the endpoint must decide, not the name.
    assert!(!provider_is_self_hosted(
        "openai",
        Some("https://api.openai.com/v1")
    ));
    assert!(!provider_is_self_hosted("openai", None));
    assert!(!provider_is_self_hosted(
        "openai:gpt-4o",
        Some("https://api.openai.com/v1")
    ));
    assert!(!provider_is_self_hosted("openrouter:x", None));
    assert!(!provider_is_self_hosted("cloud", None));
    assert!(!provider_is_self_hosted(
        "local-openai:m",
        Some("https://gpu.example.com/v1")
    ));
}

#[test]
fn local_runtimes_and_private_endpoints_are_self_hosted() {
    assert!(provider_is_self_hosted("ollama:llama3", None));
    assert!(provider_is_self_hosted("lmstudio:m", None));
    assert!(provider_is_self_hosted("mlx:m", None));
    assert!(provider_is_self_hosted(
        "llamacpp:m",
        Some("http://127.0.0.1:8080/v1")
    ));
    assert!(provider_is_self_hosted(
        "vllm:m",
        Some("http://192.168.1.20:8000/v1")
    ));
    assert!(provider_is_self_hosted(
        "openai",
        Some("http://localhost:8080/v1")
    ));
}

#[test]
fn hosted_openai_keeps_the_hosted_ceilings_in_the_policy() {
    if std::env::var_os("OPENHUMAN_MODEL_CALL_TIMEOUT_SECS").is_some()
        || std::env::var_os("OPENHUMAN_AGENT_TURN_TIMEOUT_SECS").is_some()
    {
        return;
    }
    let local = provider_is_self_hosted("openai", Some("https://api.openai.com/v1"));
    let policy = run_policy_for_provider(10, false, local);
    assert_eq!(policy.limits.max_model_call_ms, Some(900_000));
    assert_eq!(policy.limits.max_wall_clock_ms, Some(3_600_000));
}
