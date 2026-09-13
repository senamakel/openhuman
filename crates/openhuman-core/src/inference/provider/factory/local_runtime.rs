//! Local OpenAI-compatible runtimes (Ollama / LM Studio / MLX / OMLX /
//! local-openai) as crate-native `ChatModel`s.

use super::*;
use crate::inference::provider::crate_openai;
#[cfg(not(test))]
use crate::inference::provider::factory::access_gates::verify_session_active;

/// Local OpenAI-compatible runtimes (Ollama / LM Studio / MLX / OMLX /
/// local-openai) as a crate-native [`ChatModel`] (issue #4727).
///
/// Returns `None` when `role` does not resolve to a local runtime, allowing
/// [`create_chat_model_with_model_id`] to try cloud/BYOK/CLI constructors.
///
/// Endpoint/auth/`num_ctx` resolution uses the shared
/// `ollama_base_url_from_config` / `lm_studio_base_url` / profile helpers. It
/// runs the host access gates for custom/local providers —
/// [`enforce_local_only_inference`] (privacy mode) +
/// [`verify_session_active`] (session requirement) — so routing a local runtime
/// here cannot bypass either. Temperature rides the per-call `ModelRequest` on
/// the crate path (parity with the managed-backend cutover; the `@<temp>` suffix
/// still bakes a fixed override).
///
pub(super) type ResolvedChatModel = (Arc<dyn ChatModel<()>>, String);

pub(super) type OptionalChatModelResult = Option<anyhow::Result<ResolvedChatModel>>;

pub(super) fn try_create_local_runtime_chat_model(
    role: &str,
    config: &Config,
) -> OptionalChatModelResult {
    let resolved = provider_for_role(role, config);
    try_create_local_runtime_chat_model_from_string(role, &resolved, config, true)
}

pub(super) fn try_create_local_runtime_chat_model_from_string(
    role: &str,
    provider: &str,
    config: &Config,
    require_session: bool,
) -> OptionalChatModelResult {
    use crate::inference::local::profile::{LOCAL_OPENAI_PROFILE, MLX_PROFILE, OMLX_PROFILE};

    let p = provider.trim().to_string();
    let is_local = p.starts_with(OLLAMA_PROVIDER_PREFIX)
        || p.starts_with(LM_STUDIO_PROVIDER_PREFIX)
        || p.starts_with(MLX_PROVIDER_PREFIX)
        || p.starts_with(OMLX_PROVIDER_PREFIX)
        || p.starts_with(LOCAL_OPENAI_PROVIDER_PREFIX);
    if !is_local {
        return None;
    }

    // Preserve host privacy-mode refusal + the session requirement for
    // custom/local providers.
    if let Err(e) = enforce_local_only_inference(role, &p) {
        return Some(Err(e));
    }
    if require_session {
        #[cfg(not(test))]
        if let Err(e) = verify_session_active(config) {
            return Some(Err(e));
        }
    }

    // Egress spine (privacy epic S2, #4436): committed to a local runtime here
    // (past the non-local `None` return + access gates). Disclose it as
    // NON-external — local inference never leaves the device, so
    // `emit_external_transfer` records it without firing a pending event. This
    // is the single local chokepoint for every ChatModel/turn entry.
    emit_inference_egress(role, &p);

    let unsupported = config.temperature_unsupported_models.clone();
    let empty_model_err = |p: &str, form: &str| {
        anyhow::anyhow!("[chat-factory] provider string '{p}' has an empty model — use '{form}'")
    };

    // Resolve the local `api_key` + auth style shared by lmstudio/omlx/local-openai
    // (Bearer when a key is configured, else no auth — same as the host builders).
    let keyed_auth = || {
        let api_key = config.local_ai.api_key.as_deref().unwrap_or("").to_string();
        let auth = if api_key.trim().is_empty() {
            CompatAuthStyle::None
        } else {
            CompatAuthStyle::Bearer
        };
        (api_key, auth)
    };
    // First env override, else `local_ai.base_url`, else the profile default.
    let env_or_config_url = |env: &str, default: &str| {
        std::env::var("OPENHUMAN_LOCAL_INFERENCE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var(env).ok().filter(|s| !s.trim().is_empty()))
            .or_else(|| config.local_ai.base_url.clone())
            .unwrap_or_else(|| default.to_string())
    };

    if let Some(rest) = p.strip_prefix(OLLAMA_PROVIDER_PREFIX) {
        let (model, temp) = split_model_and_temperature(rest);
        if model.is_empty() {
            return Some(Err(empty_model_err(&p, "ollama:<model-id>")));
        }
        // Ollama exposes the OpenAI-compatible endpoint at `/v1`.
        let base_url = crate::inference::local::ollama_base_url_from_config(config);
        let normalized = base_url.trim_end_matches('/').trim_end_matches("/v1");
        let endpoint = format!("{normalized}/v1");
        let chat = crate_openai::make_crate_local_runtime_chat_model(
            "ollama",
            &endpoint,
            "",
            CompatAuthStyle::None,
            &model,
            &unsupported,
            temp,
            config.local_ai.num_ctx,
        );
        return Some(Ok((chat, model)));
    }
    if let Some(rest) = p.strip_prefix(LM_STUDIO_PROVIDER_PREFIX) {
        let (model, temp) = split_model_and_temperature(rest);
        if model.is_empty() {
            return Some(Err(empty_model_err(&p, "lmstudio:<model-id>")));
        }
        let endpoint = crate::inference::local::lm_studio::lm_studio_base_url(config);
        let (api_key, auth) = keyed_auth();
        let chat = crate_openai::make_crate_local_runtime_chat_model(
            "lmstudio",
            &endpoint,
            &api_key,
            auth,
            &model,
            &unsupported,
            temp,
            None,
        );
        return Some(Ok((chat, model)));
    }
    if let Some(rest) = p.strip_prefix(MLX_PROVIDER_PREFIX) {
        let (model, temp) = split_model_and_temperature(rest);
        if model.is_empty() {
            return Some(Err(empty_model_err(&p, "mlx:<model-id>")));
        }
        let endpoint = env_or_config_url("MLX_SERVER_URL", MLX_PROFILE.default_base_url);
        let chat = crate_openai::make_crate_local_runtime_chat_model(
            "mlx",
            &endpoint,
            "",
            CompatAuthStyle::None,
            &model,
            &unsupported,
            temp,
            None,
        );
        return Some(Ok((chat, model)));
    }
    if let Some(rest) = p.strip_prefix(OMLX_PROVIDER_PREFIX) {
        let (model, temp) = split_model_and_temperature(rest);
        if model.is_empty() {
            return Some(Err(empty_model_err(&p, "omlx:<model-id>")));
        }
        let endpoint = env_or_config_url("OMLX_SERVER_URL", OMLX_PROFILE.default_base_url);
        let (api_key, auth) = keyed_auth();
        let chat = crate_openai::make_crate_local_runtime_chat_model(
            "omlx",
            &endpoint,
            &api_key,
            auth,
            &model,
            &unsupported,
            temp,
            None,
        );
        return Some(Ok((chat, model)));
    }
    if let Some(rest) = p.strip_prefix(LOCAL_OPENAI_PROVIDER_PREFIX) {
        let (model, temp) = split_model_and_temperature(rest);
        if model.is_empty() {
            return Some(Err(empty_model_err(&p, "local-openai:<model-id>")));
        }
        let endpoint = env_or_config_url("LOCAL_OPENAI_URL", LOCAL_OPENAI_PROFILE.default_base_url);
        let (api_key, auth) = keyed_auth();
        let chat = crate_openai::make_crate_local_runtime_chat_model(
            "local-openai",
            &endpoint,
            &api_key,
            auth,
            &model,
            &unsupported,
            temp,
            None,
        );
        return Some(Ok((chat, model)));
    }
    None
}

/// Build a crate-native local-runtime model for setup/probe calls that run
/// before the desktop session gate is established.
pub(crate) fn create_local_chat_model_from_string(
    provider: &str,
    config: &Config,
) -> anyhow::Result<(Arc<dyn ChatModel<()>>, String)> {
    try_create_local_runtime_chat_model_from_string("chat", provider, config, false)
        .ok_or_else(|| anyhow::anyhow!("unsupported local provider string '{provider}'"))?
}
