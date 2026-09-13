//! Toolkit-allowlist, connection, and direct-mode-API-key handlers:
//! `list_toolkits`, `list_capabilities`, `list_agent_ready_toolkits`,
//! `list_connections`, `authorize`, `delete_connection`, `get_mode`,
//! `set_api_key`, `clear_api_key`.

use serde_json::{Map, Value};

use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;
use crate::integrations::composio::ops;

use super::util::{read_optional, read_required_non_empty, to_json};

pub(super) fn handle_list_toolkits(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(ops::composio_list_toolkits(&config).await?)
    })
}

pub(super) fn handle_list_capabilities(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(ops::composio_list_capabilities(&config).await?)
    })
}

pub(super) fn handle_list_agent_ready_toolkits(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async { to_json(ops::composio_list_agent_ready_toolkits().await?) })
}

pub(super) fn handle_list_connections(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async {
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(ops::composio_list_connections(&config).await?)
    })
}

pub(super) fn handle_authorize(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let toolkit = read_required_non_empty(&params, "toolkit")?;
        let extra_params = params.get("extra_params").cloned();
        to_json(ops::composio_authorize(&config, &toolkit, extra_params).await?)
    })
}

pub(super) fn handle_delete_connection(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let connection_id = read_required_non_empty(&params, "connection_id")?;
        let clear_memory = read_optional::<bool>(&params, "clear_memory")?.unwrap_or(false);
        to_json(ops::composio_delete_connection(&config, &connection_id, clear_memory).await?)
    })
}

pub(super) fn handle_get_mode(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        tracing::debug!("[composio-direct] rpc get_mode entry");
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(ops::composio_get_mode(&config).await?)
    })
}

pub(super) fn handle_set_api_key(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        tracing::debug!("[composio-direct] rpc set_api_key entry");
        let config = config_rpc::load_config_with_timeout().await?;
        let api_key = read_required_non_empty(&params, "api_key")?;
        let activate_direct = read_optional::<bool>(&params, "activate_direct")?.unwrap_or(false);
        to_json(ops::composio_set_api_key(&config, &api_key, activate_direct).await?)
    })
}

pub(super) fn handle_clear_api_key(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        tracing::debug!("[composio-direct] rpc clear_api_key entry");
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(ops::composio_clear_api_key(&config).await?)
    })
}
