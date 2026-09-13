//! `<slug>:<model>` BYOK cloud providers: shared slug resolution (model fallback,
//! abstract-tier remapping, credentials, codex routing) and the crate-native builders.

use super::*;
use crate::inference::provider::crate_anthropic;
use crate::inference::provider::crate_openai;
#[cfg(not(test))]
use crate::inference::provider::factory::access_gates::verify_backend_session_active;
#[cfg(not(test))]
use crate::inference::provider::factory::access_gates::verify_session_active;
use crate::inference::provider::fallback_diagnostics;

/// Look up a `cloud_providers` entry by slug and build the provider.
/// The shared resolution for a `<slug>:<model>` cloud provider — the cloud
/// `cloud_providers` entry, the effective model id (with `default_model`
/// fallback + abstract-tier remapping), the resolved API key, and the OpenAI
/// codex-oauth routing shared by every cloud `ChatModel` constructor.
pub(super) struct CloudSlugResolution<'a> {
    entry: &'a crate::config::schema::cloud_providers::CloudProviderCreds,
    effective_model: String,
    key: String,
    codex: crate::inference::provider::openai_codex::OpenAiCodexRouting,
}

pub(super) fn resolve_cloud_slug<'a>(
    role: &str,
    slug: &str,
    model: &str,
    config: &'a Config,
) -> anyhow::Result<CloudSlugResolution<'a>> {
    let entry = config.cloud_providers.iter().find(|e| e.slug == slug);

    let entry = entry.ok_or_else(|| {
        let known: Vec<&str> = config
            .cloud_providers
            .iter()
            .map(|e| e.slug.as_str())
            .collect();
        anyhow::anyhow!(
            "[chat-factory] no cloud provider configured for slug '{}' (role '{}') — \
             add an entry with that slug to cloud_providers in config.toml. \
             Configured slugs: [{}]",
            slug,
            role,
            known.join(", ")
        )
    })?;

    // Resolve effective model: use provided model if non-empty, else fall back
    // to the entry's legacy default_model (if any), else empty → error.
    let mut effective_model = if model.trim().is_empty() {
        entry.default_model.clone().unwrap_or_default()
    } else {
        model.to_string()
    };

    // Guard: if effective_model is still empty after fallback, bail with an
    // actionable error. Sending an empty model string to providers like
    // nvidia-nim causes a 400 "model field is required" — a confusing error
    // that obscures the real cause (missing model in the provider string or
    // unset default_model on the config entry).
    // See https://github.com/tinyhumansai/openhuman/issues/2784.
    //
    // OpenhumanJwt entries are exempt: they always delegate to
    // make_openhuman_backend which derives the model from config.default_model,
    // ignoring whatever effective_model we computed here.
    if entry.auth_style != AuthStyle::OpenhumanJwt && effective_model.trim().is_empty() {
        log::warn!(
            "[nvidia-nim][chat-factory] role={} slug={} resolved to empty model — \
             provider string must include a model id (e.g. '{}:<model-id>') or \
             set default_model on the cloud_providers entry",
            role,
            slug,
            slug,
        );
        anyhow::bail!(
            "[chat-factory] no model configured: role '{}' resolved to an empty model id for slug '{}'. \
             Include a model in the provider string (e.g. '{slug}:<model-id>') or \
             set default_model on the cloud_providers entry for slug '{slug}'.",
            role,
            slug,
        );
    }

    if entry.auth_style != AuthStyle::OpenhumanJwt && is_abstract_tier_model(&effective_model) {
        if let Some(default_model) = entry
            .default_model
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty() && !is_abstract_tier_model(m))
        {
            log::info!(
                "[providers][chat-factory] role={} slug={} remapping abstract model {} -> {}",
                role,
                slug,
                effective_model,
                default_model
            );
            effective_model = default_model.to_string();
        } else {
            anyhow::bail!(
                "[chat-factory] model '{}' is an abstract tier for role '{}', \
                 but cloud provider slug '{}' has no concrete default_model configured. \
                 Set cloud_providers[].default_model to a provider-native model id (e.g. deepseek-v4-pro).",
                effective_model,
                role,
                slug
            );
        }
    }

    log::info!(
        "[providers][chat-factory] role={} slug={} model={} endpoint_host={}",
        role,
        slug,
        effective_model,
        redact_endpoint(&entry.endpoint)
    );

    // #5146 §2.1: a raw "failed to read API key for slug 'anthropic'" is
    // baffling when the user never configured Anthropic — they set a local
    // Ollama model and this is a background role that fell back to the cloud.
    // Attach the role, and the local chat model that caused the fallback, so
    // the message explains itself and names a concrete remedy.
    // Only an *implicit* fallback is explained as one. A role with its own
    // explicit cloud route can fail key lookup here too, and telling that user
    // their local chat model caused it would be a lie.
    let implicit_fallback = role_uses_implicit_cloud_fallback(role, config);
    let local_chat = if implicit_fallback {
        config
            .chat_provider
            .as_deref()
            .filter(|chat| crate::inference::local::profile::is_local_provider_string(chat))
    } else {
        None
    };
    let missing_credentials = || {
        // Safe fields only: role, slug, and the routing shape. Never the
        // underlying error (it can echo a key) and never the key itself.
        log::warn!(
            "[providers][chat-factory] credential lookup failed role={} slug={} auth_style={} implicit_cloud_fallback={}",
            role,
            slug,
            entry.auth_style.as_str(),
            implicit_fallback
        );
        fallback_diagnostics::missing_provider_credentials_message(role, slug, local_chat)
    };

    let key = lookup_key_for_slug(slug, config)
        .map_err(|e| anyhow::anyhow!("{} (underlying error: {e})", missing_credentials()))?;

    // A readable auth profile with no key for this slug returns `Ok("")`, which
    // would otherwise build a client with an empty bearer and surface as a raw
    // 401 from the provider several layers later — exactly the baffling error
    // this diagnostic exists to replace.
    //
    // Scoped to the *implicit fallback* path deliberately. That is the case the
    // diagnostic is for: a local-chat user whose background role landed on a
    // BYOK slug they never configured. An explicitly routed provider keeps its
    // existing behaviour and is allowed to build without a stored key — callers
    // construct such models to probe or describe a provider before a key is
    // saved, and failing that at construction time would be a behaviour change
    // well beyond this diagnostic.
    //
    // Styles that carry no stored key (`OpenhumanJwt` injects a session JWT
    // downstream, `None` sends no auth header at all) are legitimately blank and
    // never trip this.
    if implicit_fallback
        && key.trim().is_empty()
        && matches!(entry.auth_style, AuthStyle::Bearer | AuthStyle::Anthropic)
    {
        anyhow::bail!("{}", missing_credentials());
    }
    let bearer_is_oauth = slug == "openai" && openai_bearer_is_oauth(config);
    let codex = resolve_openai_codex_routing(config, slug, &entry.endpoint, &key, bearer_is_oauth)
        .map_err(anyhow::Error::msg)?;

    Ok(CloudSlugResolution {
        entry,
        effective_model,
        key,
        codex,
    })
}

/// A `<slug>:<model>` BYOK cloud provider as a crate-native [`ChatModel`] — the
/// Native model for every configured cloud auth style, including the managed
/// `OpenhumanJwt` entry (issue #4727 Phase 3).
///
/// Returns `None` unless the role resolves to a **configured** cloud slug. When
/// it does:
/// - `Anthropic` / `None` / plain `Bearer` → crate `OpenAiModel` Chat Completions;
/// - `Bearer` with OpenAI **Codex OAuth** → crate `OpenAiModel` on the Responses
///   API (`with_responses_api_primary`), with the codex account/originator
///   headers, user-agent, `client_version` query param, and `max_output_tokens`
///   omitted (the crate `/v1/responses` support, tinyagents#51);
/// - `OpenhumanJwt` → the crate-native managed backend model.
///
/// The legacy host's rare chat-completions-404 → `/v1/responses` **fallback** for
/// non-codex slugs is not replicated (the crate has responses-*primary*, not
/// fallback); chat completions is the primary path those slugs use in practice.
///
/// The resolution is shared via [`resolve_cloud_slug`], so slugs resolve
/// identically to the legacy path; only the wire client differs. The **same**
/// access gate the `Provider` path applies (`enforce_local_only_inference` +
/// `verify_session_active`) runs before building. Temperature rides the per-call
/// `ModelRequest` (managed/local parity; the `@<temp>` suffix still bakes a fixed
/// override).
pub(super) fn try_create_cloud_slug_chat_model(
    role: &str,
    config: &Config,
) -> OptionalChatModelResult {
    try_create_cloud_slug_chat_model_with_native_tools(role, config, true)
}

pub(super) fn try_create_cloud_slug_chat_model_with_native_tools(
    role: &str,
    config: &Config,
    native_tool_calling: bool,
) -> OptionalChatModelResult {
    // Resolve the role's provider string, expanding the empty / "cloud" sentinel
    // to the primary cloud target.
    let mut resolved = provider_for_role(role, config);
    if resolved.trim().is_empty() || resolved.trim() == "cloud" {
        resolved = resolve_primary_cloud_provider_string(config);
    }
    try_create_cloud_slug_chat_model_from_string_with_native_tools(
        role,
        &resolved,
        config,
        native_tool_calling,
    )
}

pub(super) fn try_create_cloud_slug_chat_model_from_string(
    role: &str,
    provider: &str,
    config: &Config,
) -> OptionalChatModelResult {
    try_create_cloud_slug_chat_model_from_string_with_native_tools(role, provider, config, true)
}

pub(super) fn try_create_cloud_slug_chat_model_from_string_with_native_tools(
    role: &str,
    provider: &str,
    config: &Config,
    native_tool_calling: bool,
) -> OptionalChatModelResult {
    let p = provider.trim().to_string();

    // Only the "<slug>:<model>[@temp]" cloud form routes here. The managed
    // backend, BYOK-incomplete sentinel, and bespoke subprocess providers
    // (claude-code / claude_agent_sdk) are handled on the `Provider` path.
    if p == PROVIDER_OPENHUMAN
        || p == BYOK_INCOMPLETE_SENTINEL
        || p == CLAUDE_AGENT_SDK_PROVIDER
        || p.starts_with(CLAUDE_AGENT_SDK_PREFIX)
        || p.starts_with(crate::inference::provider::claude_code::PROVIDER_PREFIX)
    {
        return None;
    }
    let colon = p.find(':')?;
    let slug = p[..colon].trim().to_string();
    if slug.is_empty() {
        return None;
    }
    let (raw_model, temperature_override) = split_model_and_temperature(&p[colon + 1..]);
    // Not a configured cloud slug → let the `Provider` path surface the precise
    // "no cloud provider configured" error rather than silently no-op'ing.
    if !config.cloud_providers.iter().any(|e| e.slug == slug) {
        return None;
    }

    // Preserve the `Provider` path's gate for custom/cloud providers.
    if let Err(e) = enforce_local_only_inference(role, &p) {
        return Some(Err(e));
    }
    #[cfg(not(test))]
    if let Err(e) = verify_session_active(config) {
        return Some(Err(e));
    }

    let CloudSlugResolution {
        entry,
        effective_model,
        key,
        codex,
    } = match resolve_cloud_slug(role, &slug, &raw_model, config) {
        Ok(r) => r,
        Err(e) => return Some(Err(e)),
    };

    // Every configured cloud slug builds a crate-native model. OpenhumanJwt
    // delegates to the managed backend model; Codex OAuth routes to the
    // Responses API with its headers / UA / query; every other
    // Bearer/Anthropic/None slug uses Chat Completions (its primary path — the
    // legacy host's rare 404 → `/v1/responses` fallback for non-codex slugs is
    // not replicated).
    let mut endpoint = entry.endpoint.clone();
    let mut extra_headers: Vec<(String, String)> = Vec::new();
    let mut extra_query_params: Vec<(String, String)> = Vec::new();
    let mut user_agent: Option<String> = None;
    let mut responses_api_primary = false;
    let mut responses_omit_max_output_tokens = false;

    let auth = match entry.auth_style {
        AuthStyle::Anthropic => {
            // Anthropic's OpenAI-compatibility endpoint does not support prompt
            // caching (and reports `prompt_tokens_details` as always empty), so
            // a BYOK Claude key served through Chat Completions re-billed the
            // whole prefix on every call. Native tool calling is the normal
            // case and routes to the crate's Messages API adapter, which
            // places `cache_control` breakpoints. Text mode (prompt-guided
            // tools) is only implemented on the Chat Completions adapter, so
            // that rare case keeps the compat path.
            if native_tool_calling && crate_anthropic::endpoint_is_anthropic_messages(&endpoint) {
                crate::security::egress::emit_external_transfer(
                    crate::security::egress::EgressDescriptor::inference(
                        &slug,
                        &effective_model,
                        true,
                    ),
                );
                let chat = crate_anthropic::build_crate_anthropic_model(
                    crate_anthropic::CrateAnthropicConfig {
                        endpoint: endpoint.as_str(),
                        api_key: key.as_str(),
                        model: effective_model.as_str(),
                        temperature_override,
                        temperature_unsupported_models: config
                            .temperature_unsupported_models
                            .as_slice(),
                    },
                );
                return Some(Ok((chat, effective_model)));
            }
            log::debug!(
                "[providers][chat-factory] slug={slug} auth_style=anthropic native_tools=false → OpenAI-compatible text-mode path (no prompt caching)"
            );
            CompatAuthStyle::Anthropic
        }
        AuthStyle::None => CompatAuthStyle::None,
        AuthStyle::OpenhumanJwt => {
            #[cfg(not(test))]
            if let Err(error) = verify_backend_session_active(config) {
                return Some(Err(error));
            }
            let model_override =
                (!effective_model.trim().is_empty()).then_some(effective_model.as_str());
            let (backend, pinned_model) =
                match resolve_managed_backend_with_model_override(role, config, model_override) {
                    Ok(result) => result,
                    Err(error) => return Some(Err(error)),
                };
            return Some(Ok((
                Arc::new(backend.with_native_tool_calling(native_tool_calling)),
                pinned_model,
            )));
        }
        AuthStyle::Bearer => {
            // The codex routing may re-target the endpoint (OAuth backend).
            endpoint = codex.endpoint.clone();
            if let Some(account_id) = codex.account_id.as_deref() {
                extra_headers.push((
                    OPENAI_CODEX_ACCOUNT_HEADER.to_string(),
                    account_id.to_string(),
                ));
            }
            if codex.using_oauth {
                // Codex OAuth → Responses API primary + the codex request shape.
                extra_headers.push((
                    OPENAI_CODEX_ORIGINATOR_HEADER.to_string(),
                    OPENAI_CODEX_ORIGINATOR.to_string(),
                ));
                user_agent = Some(openai_codex_user_agent());
                extra_query_params
                    .push(("client_version".to_string(), openai_codex_client_version()));
                responses_api_primary = true;
                responses_omit_max_output_tokens = true;
            }
            CompatAuthStyle::Bearer
        }
    };

    // Egress spine (privacy epic S2, #4436): committed to a BYOK cloud slug here
    // — past the managed/bespoke returns and the access
    // gates, so this constructs. Disclose as external. Single cloud chokepoint
    // for every cloud ChatModel/turn entry.
    crate::security::egress::emit_external_transfer(
        crate::security::egress::EgressDescriptor::inference(&slug, &effective_model, true),
    );

    let unsupported = config.temperature_unsupported_models.clone();
    let chat = crate_openai::build_crate_openai_model(crate_openai::CrateOpenAiConfig {
        provider_name: slug.as_str(),
        endpoint: endpoint.as_str(),
        api_key: key.as_str(),
        auth_style: auth,
        model: effective_model.as_str(),
        temperature_unsupported_models: unsupported.as_slice(),
        temperature_override,
        // Cloud OpenAI-compatible providers accept a `system` role — no merge
        // (parity with the crate-native OpenAI model defaults).
        merge_system_into_user: false,
        extra_headers: extra_headers.as_slice(),
        native_tool_calling: Some(native_tool_calling),
        vision: None,
        default_provider_options: None,
        responses_api_primary,
        responses_omit_max_output_tokens,
        extra_query_params: extra_query_params.as_slice(),
        user_agent: user_agent.as_deref(),
        // OpenRouter forwards explicit `cache_control` markers to Anthropic
        // and Gemini, which cache nothing through a Chat Completions relay
        // without them; hosted OpenAI rejects unknown part fields, so the
        // flag is keyed on the relay, not on by default.
        explicit_cache_control: crate_openai::endpoint_is_openrouter(&endpoint),
    });
    Some(Ok((chat, effective_model)))
}
