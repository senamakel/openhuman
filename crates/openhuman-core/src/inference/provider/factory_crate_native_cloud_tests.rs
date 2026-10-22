#[test]
fn create_chat_model_routes_plain_bearer_cloud_slug_to_crate_native() {
    let _guard = crate::inference::inference_test_guard();
    // DeepSeek is a built-in chat-completions-only Bearer provider: no
    // `/v1/responses` fallback and no codex-oauth, so it is wire-equivalent and
    // flips crate-native.
    let mut config = Config::default();
    config.cloud_providers.push(deepseek_entry("p_ds"));
    config.chat_provider = Some("deepseek:deepseek-reasoner".to_string());
    let (model, model_id) = create_chat_model_with_model_id_in(
        &crate::core::runtime::DispatchCtx::ambient(),
        "chat",
        &config,
        0.7,
    )
    .expect("bearer cloud create_chat_model must build");
    assert_eq!(model_id, "deepseek-reasoner");
    let profile = model
        .profile()
        .expect("crate-native cloud model exposes a profile");
    assert_eq!(profile.provider.as_deref(), Some("deepseek"));
    // A generic cloud model keeps native tool calling + vision on (unlike the
    // local runtimes), so this is the crate `OpenAiModel` default profile.
    assert!(profile.tool_calling);
}

#[test]
fn turn_model_route_metadata_uses_post_remap_cloud_model() {
    let _guard = crate::inference::inference_test_guard();
    let mut config = Config::default();
    config.cloud_providers.push(deepseek_entry("p_ds"));
    config.chat_provider = Some("deepseek:hint:chat".to_string());

    let (_model, provider, resolved_model) = create_turn_chat_model_with_native_tools_and_route(
        &crate::core::runtime::DispatchCtx::ambient(),
        "chat",
        &config,
        "hint:chat",
        0.7,
        true,
        &EgressLedger::none(),
    )
    .expect("abstract BYOK tier must build");

    assert_eq!(provider, "deepseek");
    assert_eq!(resolved_model, "deepseek-chat");
}

#[test]
fn explicit_cloud_provider_string_routes_to_crate_native_model() {
    let _guard = crate::inference::inference_test_guard();
    let mut config = Config::default();
    config.cloud_providers.push(deepseek_entry("p_ds"));
    let (model, model_id) = create_chat_model_from_string_with_model_id(
        "chat",
        "deepseek:deepseek-reasoner",
        &config,
        0.7,
    )
    .expect("explicit cloud model must build");
    assert_eq!(model_id, "deepseek-reasoner");
    assert_eq!(
        model
            .profile()
            .and_then(|profile| profile.provider.as_deref()),
        Some("deepseek")
    );
}

#[test]
fn create_chat_model_routes_anthropic_auth_cloud_slug_to_crate_native() {
    let _guard = crate::inference::inference_test_guard();
    // Anthropic-auth cloud slugs are always wire-equivalent (their endpoints have
    // no `/v1/responses`, so the host's dormant fallback is behavior-neutral).
    let mut config = Config::default();
    config
        .cloud_providers
        .push(anthropic_entry("p_anth", "anthropic"));
    config.chat_provider = Some("anthropic:claude-sonnet-4-6".to_string());
    let (model, model_id) = create_chat_model_with_model_id_in(
        &crate::core::runtime::DispatchCtx::ambient(),
        "chat",
        &config,
        0.7,
    )
    .expect("anthropic cloud create_chat_model must build");
    assert_eq!(model_id, "claude-sonnet-4-6");
    assert_eq!(
        model.profile().and_then(|p| p.provider.as_deref()),
        Some("anthropic")
    );
    // The native Messages API adapter, not the OpenAI-compat flavour: its
    // cache identity is `anthropic:<base>:<model>`, while the compat client
    // would report the provider slug with an OpenAI-shaped identity. Only the
    // native adapter places `cache_control` breakpoints — Anthropic's compat
    // endpoint documents prompt caching as unsupported.
    let identity = model.cache_identity().expect("anthropic identifies itself");
    assert!(
        identity.starts_with("anthropic:https://api.anthropic.com/v1:claude-sonnet-4-6"),
        "expected the native Messages adapter, got identity {identity}"
    );
    assert!(model
        .profile()
        .is_some_and(|p| p.tool_calling && p.streaming));
}

/// Text mode (prompt-guided tools) is only implemented on the Chat Completions
/// adapter, so an Anthropic slug asked for `native_tools = false` keeps the
/// compat client rather than a native adapter that would ignore the request.
#[test]
fn anthropic_auth_cloud_slug_in_text_mode_keeps_the_compat_client() {
    let _guard = crate::inference::inference_test_guard();
    let mut config = Config::default();
    config
        .cloud_providers
        .push(anthropic_entry("p_anth", "anthropic"));
    let (model, model_id) = try_create_cloud_slug_chat_model_from_string_with_native_tools(
        "chat",
        "anthropic:claude-sonnet-4-6",
        &config,
        false,
        &EgressLedger::none(),
    )
    .expect("anthropic slug resolves")
    .expect("build");
    assert_eq!(model_id, "claude-sonnet-4-6");
    let identity = model.cache_identity().unwrap_or_default();
    assert!(
        !identity.starts_with("anthropic:https://"),
        "text mode must not route to the native adapter, got {identity}"
    );
    assert!(model.profile().is_some_and(|p| !p.tool_calling));
}
