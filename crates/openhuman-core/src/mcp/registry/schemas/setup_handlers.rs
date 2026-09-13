//! `mcp_setup` handler implementations — deserialise params and delegate
//! to `setup_ops.rs`.

use crate::mcp::registry::schemas::params::read_optional_string;
use crate::mcp::registry::schemas::params::read_optional_u32;
use serde_json::{Map, Value};

use crate::config::rpc as config_rpc;
use crate::core::all::ControllerFuture;

use super::params::{read_required, to_json};

pub(super) fn handle_setup_search(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let query = read_optional_string(&params, "query")?;
        let page = read_optional_u32(&params, "page")?;
        let page_size = read_optional_u32(&params, "page_size")?;
        to_json(
            crate::mcp::registry::setup_ops::mcp_setup_search(&config, query, page, page_size)
                .await?,
        )
    })
}

pub(super) fn handle_setup_get(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let qualified_name = read_required::<String>(&params, "qualified_name")?;
        to_json(crate::mcp::registry::setup_ops::mcp_setup_get(&config, qualified_name).await?)
    })
}

pub(super) fn handle_setup_request_secret(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let key_name = read_required::<String>(&params, "key_name")?;
        let prompt = read_required::<String>(&params, "prompt")?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::mcp::registry::setup_ops::mcp_setup_request_secret(&config, key_name, prompt)
                .await?,
        )
    })
}

pub(super) fn handle_setup_submit_secret(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let ref_id = read_required::<String>(&params, "ref_id")?;
        let value = read_required::<String>(&params, "value")?;
        let config = config_rpc::load_config_with_timeout().await?;
        to_json(
            crate::mcp::registry::setup_ops::mcp_setup_submit_secret(&config, ref_id, value)
                .await?,
        )
    })
}

pub(super) fn handle_setup_test_connection(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let qualified_name = read_required::<String>(&params, "qualified_name")?;
        let env_refs =
            read_required::<std::collections::HashMap<String, String>>(&params, "env_refs")?;
        to_json(
            crate::mcp::registry::setup_ops::mcp_setup_test_connection(
                &config,
                qualified_name,
                env_refs,
            )
            .await?,
        )
    })
}

pub(super) fn handle_setup_install_and_connect(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let config = config_rpc::load_config_with_timeout().await?;
        let qualified_name = read_required::<String>(&params, "qualified_name")?;
        let env_refs =
            read_required::<std::collections::HashMap<String, String>>(&params, "env_refs")?;
        to_json(
            crate::mcp::registry::setup_ops::mcp_setup_install_and_connect(
                &config,
                qualified_name,
                env_refs,
            )
            .await?,
        )
    })
}
