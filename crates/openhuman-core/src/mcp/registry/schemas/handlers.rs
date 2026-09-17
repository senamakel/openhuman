//! `mcp_clients` handler implementations — deserialise params and delegate
//! to `ops.rs`.

use serde_json::{Map, Value};

use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;

use super::params::{
    read_optional, read_optional_json, read_optional_string, read_optional_u32, read_required,
    to_json,
};

// ── Handler implementations ──────────────────────────────────────────────────

pub(super) fn handle_registry_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let query = read_optional_string(&params, "query")?;
        let transport = read_optional_string(&params, "transport")?;
        let page = read_optional_u32(&params, "page")?;
        let page_size = read_optional_u32(&params, "page_size")?;
        to_json(
            crate::mcp::registry::ops::mcp_clients_registry_search(
                &config, query, transport, page, page_size,
            )
            .await?,
        )
    })
}

pub(super) fn handle_registry_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let qualified_name = read_required::<String>(&params, "qualified_name")?;
        to_json(crate::mcp::registry::ops::mcp_clients_registry_get(&config, qualified_name).await?)
    })
}

pub(super) fn handle_installed_list(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let _ = params;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::mcp::registry::ops::mcp_clients_installed_list(&config).await?)
    })
}

pub(super) fn handle_install(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let qualified_name = read_required::<String>(&params, "qualified_name")?;
        let env = read_required::<std::collections::HashMap<String, String>>(&params, "env")?;
        let config_value = read_optional_json(&params, "config")?;
        to_json(
            crate::mcp::registry::ops::mcp_clients_install(
                &config,
                qualified_name,
                env,
                config_value,
            )
            .await?,
        )
    })
}

pub(super) fn handle_update_env(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        let env = read_required::<std::collections::HashMap<String, String>>(&params, "env")?;
        to_json(crate::mcp::registry::ops::mcp_clients_update_env(&config, server_id, env).await?)
    })
}

pub(super) fn handle_uninstall(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        to_json(crate::mcp::registry::ops::mcp_clients_uninstall(&config, server_id).await?)
    })
}

pub(super) fn handle_detect_auth(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        to_json(crate::mcp::registry::ops::mcp_clients_detect_auth(&config, server_id).await?)
    })
}

pub(super) fn handle_oauth_begin(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        to_json(crate::mcp::registry::ops::mcp_clients_oauth_begin(&config, server_id).await?)
    })
}

pub(super) fn handle_connect(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        to_json(crate::mcp::registry::ops::mcp_clients_connect(&config, server_id).await?)
    })
}

pub(super) fn handle_disconnect(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        to_json(crate::mcp::registry::ops::mcp_clients_disconnect(&config, server_id).await?)
    })
}

pub(super) fn handle_set_enabled(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let server_id = read_required::<String>(&params, "server_id")?;
        let enabled = read_required::<bool>(&params, "enabled")?;
        to_json(
            crate::mcp::registry::ops::mcp_clients_set_enabled(&config, server_id, enabled).await?,
        )
    })
}

pub(super) fn handle_status(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let _ = params;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::mcp::registry::ops::mcp_clients_status(&config).await?)
    })
}

pub(super) fn handle_tool_call(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let server_id = read_required::<String>(&params, "server_id")?;
        let tool_name = read_required::<String>(&params, "tool_name")?;
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or(Value::Object(Map::new()));
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::mcp::registry::ops::mcp_clients_tool_call(
                &config, server_id, tool_name, arguments,
            )
            .await?,
        )
    })
}

pub(super) fn handle_config_assist(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let qualified_name = read_required::<String>(&params, "qualified_name")?;
        let user_message = read_required::<String>(&params, "user_message")?;
        let history =
            read_optional::<Vec<crate::mcp::registry::types::ChatTurn>>(&params, "history")?;
        to_json(
            crate::mcp::registry::ops::mcp_clients_config_assist(
                &config,
                qualified_name,
                user_message,
                history,
            )
            .await?,
        )
    })
}

pub(super) fn handle_registry_settings_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let _ = params;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(crate::mcp::registry::ops::mcp_clients_registry_settings_get(&config).await?)
    })
}

pub(super) fn handle_registry_settings_set(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let smithery_api_key = read_optional::<String>(&params, "smithery_api_key")?;
        let mcp_official_base = read_optional::<String>(&params, "mcp_official_base")?;
        let mcp_official_token = read_optional::<String>(&params, "mcp_official_token")?;
        let mut config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::mcp::registry::ops::mcp_clients_registry_settings_set(
                &mut config,
                smithery_api_key,
                mcp_official_base,
                mcp_official_token,
            )
            .await?,
        )
    })
}
