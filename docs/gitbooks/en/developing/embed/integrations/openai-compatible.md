---
description: "Route inference to an explicit endpoint and bearer without changing shared host configuration."
icon: code
---

# OpenAI-compatible providers and BYOK

Construct a route with `Provider::openai_compatible(base_url, api_key).model(model_id)`, then set it on the runtime or an individual agent. Pass the API root, such as a provider's `/v1` URL; the adapter appends `/chat/completions`. Supply an explicit model id so the route can register that model for its workload roles. A `Route` can also carry the endpoint's provider options and a reasoning effort, which then hold for every call on it.

The runtime and agent routes in this example answer independently. Assertions inspect the actual request bodies, including model, temperature, and token limits.

<!-- BEGIN EMBED: crates/openhuman-embed/examples/defaults_overrides.rs#defaults_overrides -->

```rust
    let inherited = runtime.agent(AgentSpec::new("inherited"))?;
    let override_provider = support::provider("override reply").await;
    let overridden = runtime.agent(
        AgentSpec::new("overridden")
            .provider(support::route(&override_provider, "override-model"))
            .access(openhuman_embed::Access::readonly())
            .model_defaults(openhuman_embed::ModelDefaults {
                temperature: Some(0.8),
                max_tokens: Some(128),
                ..Default::default()
            }),
    )?;
    let inherited_reply = inherited.run("Hello default").await?;
    let overridden_reply = overridden.run("Hello override").await?;
    if support::offline() {
        assert_eq!(inherited_reply.reply, "hello from the stub");
        let requests = support::chat_requests(&provider).await;
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body)?;
        assert_eq!(body["model"], "fixture");
        assert_eq!(body["temperature"], 0.4);
        assert_eq!(body["max_tokens"], 256);
    }
    assert_eq!(overridden_reply.reply, "override reply");
    let requests = support::chat_requests(&override_provider).await;
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body)?;
    assert_eq!(body["model"], "override-model");
    assert_eq!(body["temperature"], 0.8);
    assert_eq!(body["max_tokens"], 128);
    assert_eq!(runtime.defaults().model.temperature, Some(0.4));
    println!("default and override routes answered independently");
```

<!-- END EMBED -->

The [complete defaults_overrides example](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-embed/examples/defaults_overrides.rs) includes the loopback endpoint setup. Run `cargo run -p openhuman-embed --example defaults_overrides` for its offline assertions.

Examples using `support::example_provider` switch to live inference only with `OPENHUMAN_EXAMPLE_LIVE=1` and all three of `OPENHUMAN_EXAMPLE_BASE_URL`, `OPENHUMAN_EXAMPLE_API_KEY`, and `OPENHUMAN_EXAMPLE_MODEL`. These settings are example controls, not replacements for your application's credential configuration. Keep provider keys in your host's secret configuration.

## Provider options and reasoning effort

`Route::provider_options` and `Route::reasoning_effort` set request preferences for the endpoint itself, not for one turn. For example, `Route::openai_compatible("https://openrouter.ai/api/v1", key).provider_options(json!({"provider": {"order": ["first-host", "second-host"], "allow_fallbacks": true}})).reasoning_effort(ReasoningEffort::Low)` asks OpenRouter for those hosts in that order and low reasoning on every call. Pass that route to `Provider::routed(route).model(model_id)`. `ReasoningEffort` is re-exported from `openhuman_embed`.

- **What is sent.** The options are flattened into every Chat Completions body on the route. This covers the agent's turns, a per-turn `Turn::route`, scheduled agent runs, the payload summarizer and sub-agents, and `Completer` calls on the route. They must form a JSON object. Keys owned by the normalized request (`model`, `messages`, `tools`, `temperature`, `max_tokens`, `stream` and the like) are never sent: an agent turn drops them and a completion refuses them.
- **OpenRouter's default.** On an `openrouter.ai` endpoint the registry otherwise sends `{"provider":{"sort":"price"}}`. A `provider` object in the route's options replaces that default.
- **What a call can still override.** A turn's `Turn::provider_options`, or a `CompletionRequest::provider_options`, replaces the route's options per top-level key: a request `provider` replaces the route's whole `provider` object, and keys the request does not name keep the route's value.
- **Reasoning.** The effort becomes the route model's `runtime.reasoning_effort_by_model` entry. A thread's own reasoning choice still outranks it, it outranks `runtime.reasoning_effort`, and sub-agents keep their provider default. The wire carries `reasoning_effort`. When a turn caps its output on OpenRouter, it carries `reasoning.max_tokens` (a share of the cap) instead. A `Completer` request that sets no reasoning of its own gets the route's effort.

For a non-HTTP implementation, [local and custom models](local-models.md) covers `Provider::custom`, native fallback, and role pins. [Managed inference](tinyhumans-managed.md) uses a different backend transport boundary.
