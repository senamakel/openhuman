//! Read-only channel catalog lookups.

use crate::rpc::RpcOutcome;

use super::super::super::definitions::{
    all_channel_definitions, find_channel_definition, ChannelDefinition,
};

/// List all available channel definitions.
pub async fn list_channels() -> Result<RpcOutcome<Vec<ChannelDefinition>>, String> {
    Ok(RpcOutcome::new(all_channel_definitions(), vec![]))
}

/// Describe a single channel by id.
pub async fn describe_channel(channel_id: &str) -> Result<RpcOutcome<ChannelDefinition>, String> {
    let def = find_channel_definition(channel_id)
        .ok_or_else(|| format!("unknown channel: {channel_id}"))?;
    Ok(RpcOutcome::new(def, vec![]))
}
