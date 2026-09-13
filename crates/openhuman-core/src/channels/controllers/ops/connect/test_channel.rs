//! Testing a channel's credentials without persisting anything.

use serde_json::Value;

use crate::config::Config;
use crate::rpc::RpcOutcome;

use super::super::super::definitions::{find_channel_definition, ChannelAuthMode};
use super::super::types::ChannelTestResult;
use super::email::{build_email_config, verify_email_credentials};

/// Test a channel connection without persisting credentials.
pub async fn test_channel(
    _config: &Config,
    channel_id: &str,
    auth_mode: ChannelAuthMode,
    credentials_value: Value,
) -> Result<RpcOutcome<ChannelTestResult>, String> {
    let def = find_channel_definition(channel_id)
        .ok_or_else(|| format!("unknown channel: {channel_id}"))?;

    let creds_map = credentials_value
        .as_object()
        .ok_or("credentials must be a JSON object")?;

    // Validate fields first.
    def.validate_credentials(auth_mode, creds_map)?;

    // Email supports a real connection test: build the effective config and
    // attempt an IMAP login without persisting anything.
    if channel_id == "email" && auth_mode == ChannelAuthMode::ApiKey {
        let email_cfg = build_email_config(creds_map, None)?;
        verify_email_credentials(&email_cfg).await?;
        return Ok(RpcOutcome::new(
            ChannelTestResult {
                success: true,
                message: "IMAP login succeeded.".to_string(),
            },
            vec![],
        ));
    }

    // For other channels, field validation is the test. A future version can
    // instantiate the channel provider and call health_check().
    Ok(RpcOutcome::new(
        ChannelTestResult {
            success: true,
            message: format!(
                "Credentials for '{}' ({}) are structurally valid.",
                channel_id, auth_mode
            ),
        },
        vec![],
    ))
}
