//! Subprocess-backed providers: the Claude Agent SDK and the Claude Code CLI.

use super::*;
#[cfg(not(test))]
use crate::inference::provider::factory::access_gates::verify_session_active;

/// Build the Claude Agent SDK subprocess directly as a crate model. This is a
/// prompt-guided model: TinyAgents owns its text-tool protocol, while the
/// provider owns only subprocess transport and NDJSON decoding.
pub(super) fn try_create_claude_agent_sdk_chat_model(
    role: &str,
    config: &Config,
) -> OptionalChatModelResult {
    let resolved = provider_for_role(role, config);
    try_create_claude_agent_sdk_chat_model_from_string(role, &resolved, config)
}

pub(super) fn try_create_claude_agent_sdk_chat_model_from_string(
    role: &str,
    provider: &str,
    config: &Config,
) -> OptionalChatModelResult {
    let model = match prepare_claude_agent_sdk_chat_model(role, provider, config)? {
        Ok(model) => model,
        Err(error) => return Some(Err(error)),
    };
    emit_inference_egress(role, &format!("{CLAUDE_AGENT_SDK_PREFIX}{model}"));
    let chat: Arc<dyn ChatModel<()>> = Arc::new(ClaudeAgentSdkProvider::for_model(
        config.claude_agent_sdk.clone(),
        model.clone(),
    ));
    Some(Ok((chat, model)))
}

pub(super) fn prepare_claude_agent_sdk_chat_model(
    role: &str,
    provider: &str,
    config: &Config,
) -> Option<anyhow::Result<String>> {
    let model = claude_agent_sdk_model_from_string(provider, config)?;
    if let Err(error) = enforce_local_only_inference(role, provider) {
        return Some(Err(error));
    }
    #[cfg(not(test))]
    if let Err(error) = verify_session_active(config) {
        return Some(Err(error));
    }
    Some(Ok(model))
}

pub(super) fn claude_agent_sdk_model_from_string(
    provider: &str,
    config: &Config,
) -> Option<String> {
    let provider = provider.trim();
    let model = if provider == CLAUDE_AGENT_SDK_PROVIDER {
        config.claude_agent_sdk.default_model.clone()
    } else if let Some(model) = provider.strip_prefix(CLAUDE_AGENT_SDK_PREFIX) {
        model.trim().to_string()
    } else {
        return None;
    };
    Some(model)
}

pub(super) fn try_create_claude_code_chat_model(
    role: &str,
    config: &Config,
    model_override: Option<&str>,
) -> OptionalChatModelResult {
    let resolved = provider_for_role(role, config);
    try_create_claude_code_chat_model_from_string(role, &resolved, config, model_override)
}

pub(super) fn try_create_claude_code_chat_model_from_string(
    role: &str,
    provider: &str,
    config: &Config,
    model_override: Option<&str>,
) -> OptionalChatModelResult {
    let provider = provider.trim();
    let model_with_temp =
        provider.strip_prefix(crate::inference::provider::claude_code::PROVIDER_PREFIX)?;
    let (configured_model, temperature_override) = split_model_and_temperature(model_with_temp);
    if temperature_override.is_some() {
        log::warn!(
            "[providers][chat-factory] claude-code provider: per-model temperature override \
             is accepted but not wired through to the CLI — the @<temp> suffix is ignored"
        );
    }
    if configured_model.is_empty() {
        return Some(Err(anyhow::anyhow!(
            "[chat-factory] provider string '{}' for role '{}' has an empty model — \
             use 'claude-code:<model-id>'",
            provider,
            role
        )));
    }
    if let Err(error) = enforce_local_only_inference(role, provider) {
        return Some(Err(error));
    }
    #[cfg(not(test))]
    if let Err(error) = verify_session_active(config) {
        return Some(Err(error));
    }
    let workspace = crate::inference::provider::claude_code::workspace_dir_from_config(config);
    let effective_model = model_override.unwrap_or(&configured_model).to_string();
    emit_inference_egress(
        role,
        &format!(
            "{}{effective_model}",
            crate::inference::provider::claude_code::PROVIDER_PREFIX
        ),
    );
    let chat = match crate::inference::provider::claude_code::ClaudeCodeProvider::from_env(
        effective_model,
        workspace,
        config.action_dir.clone(),
    ) {
        Ok(model) => Arc::new(model) as Arc<dyn ChatModel<()>>,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok((chat, configured_model)))
}
