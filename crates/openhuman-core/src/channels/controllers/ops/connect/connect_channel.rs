//! Initiating a channel connection (credential validation, persistence, and
//! any live pre-verification).

use serde_json::Value;

use crate::channels::email_channel::EmailConfig;
use crate::channels::providers::yuanbao::YuanbaoConfig;
use crate::config::{Config, DiscordConfig, IMessageConfig, TelegramConfig};
use crate::rpc::RpcOutcome;
use crate::security::credentials;

use super::super::super::definitions::{find_channel_definition, ChannelAuthMode};
use super::super::types::ChannelConnectionResult;
use super::super::yuanbao::{
    build_effective_yuanbao_config, require_yuanbao_field, verify_yuanbao_credentials,
};
use super::email::{build_email_config, persist_email_config, verify_email_credentials};
use super::shared::{credential_provider, parse_allowed_users, parse_optional_bool};

/// Initiate a channel connection.
///
/// For `BotToken`/`ApiKey` modes: validates fields and stores credentials.
/// For `OAuth`/`ManagedDm` modes: returns the auth action the frontend should handle.
pub async fn connect_channel(
    config: &Config,
    channel_id: &str,
    auth_mode: ChannelAuthMode,
    credentials_value: Value,
) -> Result<RpcOutcome<ChannelConnectionResult>, String> {
    let def = find_channel_definition(channel_id)
        .ok_or_else(|| format!("unknown channel: {channel_id}"))?;

    let spec = def.auth_mode_spec(auth_mode).ok_or_else(|| {
        format!(
            "channel '{}' does not support auth mode '{}'",
            channel_id, auth_mode
        )
    })?;

    // For OAuth/managed modes, return the auth action without storing credentials.
    if let Some(action) = spec.auth_action {
        return Ok(RpcOutcome::new(
            ChannelConnectionResult {
                status: "pending_auth".to_string(),
                restart_required: false,
                auth_action: Some(action.to_string()),
                message: Some(format!("Initiate '{}' auth flow on the frontend. Ignore if you are already in the auth flow.", action)),
            },
            vec![],
        ));
    }

    // Credential-based modes: validate required fields.
    let creds_map = credentials_value
        .as_object()
        .ok_or("credentials must be a JSON object")?;

    def.validate_credentials(auth_mode, creds_map)?;

    // Yuanbao: build the effective config (with any client-supplied
    // endpoint overrides applied) once, verify against THAT cluster, and
    // reuse the same config for persistence below. This prevents the
    // verifier from validating against prod while the runtime then
    // reconnects to a pre-release cluster after restart.
    let mut prebuilt_yuanbao_config: Option<YuanbaoConfig> = None;
    if channel_id == "yuanbao" && auth_mode == ChannelAuthMode::ApiKey {
        let app_key = require_yuanbao_field(creds_map, "app_key")?;
        let app_secret = require_yuanbao_field(creds_map, "app_secret")?;
        let base = config.channels_config.yuanbao.clone().unwrap_or_default();
        let effective = build_effective_yuanbao_config(base, creds_map, app_key);
        verify_yuanbao_credentials(&effective, &app_secret).await?;
        prebuilt_yuanbao_config = Some(effective);
    }

    // Email (IMAP/SMTP): build the effective config and live-verify the IMAP
    // login BEFORE storing anything, so bad server settings surface in the UI
    // rather than persisting and wedging the listener on the next restart.
    // Reused below for persistence so verify and runtime can never diverge.
    let mut prebuilt_email_config: Option<EmailConfig> = None;
    if channel_id == "email" && auth_mode == ChannelAuthMode::ApiKey {
        let email_cfg = build_email_config(creds_map, config.channels_config.email.as_ref())?;
        verify_email_credentials(&email_cfg).await?;
        prebuilt_email_config = Some(email_cfg);
    }

    // iMessage is local-only (no credentials): persist channels_config + return connected.
    if channel_id == "imessage" && auth_mode == ChannelAuthMode::ManagedDm {
        let allowed_contacts = parse_allowed_users(creds_map.get("allowed_contacts"));
        let allowed_contacts_count = allowed_contacts.len();

        let mut persisted = config.clone();
        persisted.channels_config.imessage = Some(IMessageConfig { allowed_contacts });

        persisted
            .save()
            .await
            .map_err(|e| format!("failed to persist imessage config.toml: {e}"))?;

        tracing::info!(
            target: "openhuman::channels",
            allowed_contacts_count,
            "[imessage] connect_channel: wrote channels_config.imessage; restart core for AppleScript bridge to load"
        );

        return Ok(RpcOutcome::single_log(
            ChannelConnectionResult {
                status: "connected".to_string(),
                restart_required: true,
                auth_action: None,
                message: Some(
                    "iMessage channel configured. Grant Full Disk Access and restart the service to activate.".to_string(),
                ),
            },
            "stored imessage channel config (local-only)".to_string(),
        ));
    }

    // Store credentials via the credentials domain.
    let provider_key = credential_provider(channel_id, auth_mode);

    // Extract the primary token field (bot_token or api_key) if present.
    let token = creds_map
        .get("bot_token")
        .or_else(|| creds_map.get("api_key"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    // Store remaining fields as metadata.
    let fields = if creds_map.len() > 1 || (creds_map.len() == 1 && token.is_none()) {
        Some(Value::Object(creds_map.clone()))
    } else {
        None
    };

    credentials::ops::store_provider_credentials(
        config,
        &provider_key,
        None, // default profile
        token,
        fields,
        Some(true),
    )
    .await
    .map_err(|e| format!("failed to store credentials: {e}"))?;

    // Keep runtime channel config in sync so listeners can actually start
    // with the credentials just connected from the UI.
    if channel_id == "telegram" && auth_mode == ChannelAuthMode::BotToken {
        let bot_token = creds_map
            .get("bot_token")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "missing required bot_token".to_string())?
            .to_string();
        let allowed_users = parse_allowed_users(creds_map.get("allowed_users"));
        let allowed_users_count = allowed_users.len();
        // Default chat for recipient-less proactive sends (mirrors Discord's
        // `channel_id`). Read fresh from the form each connect: present ⇒ use it
        // (empty ⇒ cleared); absent ⇒ unset.
        let chat_id = creds_map
            .get("chat_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let has_chat_id = chat_id.is_some();

        let mut persisted = config.clone();
        let (stream_mode, draft_update_interval_ms, silent_streaming, mention_only) =
            if let Some(existing) = persisted.channels_config.telegram.as_ref() {
                (
                    existing.stream_mode,
                    existing.draft_update_interval_ms,
                    existing.silent_streaming,
                    existing.mention_only,
                )
            } else {
                (crate::config::StreamMode::default(), 1000, true, false)
            };

        persisted.channels_config.telegram = Some(TelegramConfig {
            bot_token,
            chat_id,
            allowed_users,
            stream_mode,
            draft_update_interval_ms,
            silent_streaming,
            mention_only,
        });

        persisted
            .save()
            .await
            .map_err(|e| format!("failed to persist telegram config.toml: {e}"))?;

        tracing::info!(
            target: "openhuman::channels",
            allowed_users_count,
            has_chat_id,
            mention_only,
            "[telegram] connect_channel: wrote channels_config.telegram; restart core for listener to load token"
        );
    } else if channel_id == "discord" && auth_mode == ChannelAuthMode::BotToken {
        let bot_token = creds_map
            .get("bot_token")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "missing required bot_token".to_string())?
            .to_string();

        let guild_id = creds_map
            .get("guild_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let discord_channel_id = creds_map
            .get("channel_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        let mut persisted = config.clone();
        let existing = persisted.channels_config.discord.as_ref();
        // Distinguish an *explicitly cleared* allowlist from an *omitted* one.
        // The field is advertised as "blank = everyone" (definitions.rs) and the
        // provider treats an empty list as allow-all, but the old logic reused
        // the saved list whenever the parsed value was empty — so a user who
        // cleared the allowlist on reconnect stayed restricted to the previous
        // users (#3794 review — Codex P2). The key is present in `creds_map`
        // (even as an empty string) only when the FE sends it; a cleared field
        // now submits an explicit empty value. So: present ⇒ honor literally
        // (empty ⇒ allow-all); absent ⇒ reuse the saved list (reconnect
        // convenience for callers that don't resend the field at all).
        let allowed_users = match creds_map.get("allowed_users") {
            Some(raw) => parse_allowed_users(Some(raw)),
            None => existing
                .map(|cfg| cfg.allowed_users.clone())
                .unwrap_or_default(),
        };
        let allowed_users_count = allowed_users.len();
        let listen_to_bots = parse_optional_bool(creds_map.get("listen_to_bots"))
            .unwrap_or_else(|| existing.map(|cfg| cfg.listen_to_bots).unwrap_or(false));
        let mention_only = parse_optional_bool(creds_map.get("mention_only"))
            .unwrap_or_else(|| existing.map(|cfg| cfg.mention_only).unwrap_or(false));

        persisted.channels_config.discord = Some(DiscordConfig {
            bot_token,
            guild_id: guild_id.clone(),
            channel_id: discord_channel_id.clone(),
            allowed_users,
            listen_to_bots,
            mention_only,
        });

        persisted
            .save()
            .await
            .map_err(|e| format!("failed to persist discord config.toml: {e}"))?;

        tracing::info!(
            target: "openhuman::channels",
            has_guild_id = guild_id.is_some(),
            has_channel_id = discord_channel_id.is_some(),
            allowed_users_count,
            listen_to_bots,
            mention_only,
            "[discord] connect_channel: wrote channels_config.discord; restart core for listener to load token"
        );
    } else if channel_id == "yuanbao" && auth_mode == ChannelAuthMode::ApiKey {
        // Reuse the effective config built above (with `env` / `api_domain`
        // / `ws_domain` / `route_env` overrides already applied and
        // `app_secret` already cleared) so persistence and verification
        // can never diverge.
        let yb_config = prebuilt_yuanbao_config.take().ok_or_else(|| {
            "internal error: yuanbao config not built before persistence".to_string()
        })?;

        let mut persisted = config.clone();
        persisted.channels_config.yuanbao = Some(yb_config);

        persisted
            .save()
            .await
            .map_err(|e| format!("failed to persist yuanbao config.toml: {e}"))?;

        tracing::info!(
            target: "openhuman::channels",
            "[yuanbao] connect_channel: wrote channels_config.yuanbao (secret stored in credentials); restart core for WS listener"
        );
    } else if channel_id == "email" && auth_mode == ChannelAuthMode::ApiKey {
        // Reuse the config already built + IMAP-verified above so persistence
        // and verification can never diverge.
        let email_cfg = prebuilt_email_config.take().ok_or_else(|| {
            "internal error: email config not built before persistence".to_string()
        })?;
        persist_email_config(config, email_cfg).await?;
    }

    Ok(RpcOutcome::single_log(
        ChannelConnectionResult {
            status: "connected".to_string(),
            restart_required: true,
            auth_action: None,
            message: Some(format!(
                "Channel '{}' credentials stored. Restart the service to activate.",
                channel_id
            )),
        },
        format!("stored credentials for {}", provider_key),
    ))
}
