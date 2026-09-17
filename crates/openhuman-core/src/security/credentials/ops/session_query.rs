//! Read-only session state lookups.

use serde_json::json;

use crate::api::jwt::get_session_token;
use crate::config::Config;
use crate::rpc::RpcOutcome;
use crate::security::credentials::session_support::build_session_state;

pub async fn auth_get_state(
    config: &Config,
) -> Result<RpcOutcome<super::super::responses::AuthStateResponse>, String> {
    let state = build_session_state(config)?;
    Ok(RpcOutcome::single_log(state, "session state fetched"))
}

pub async fn auth_get_session_token_json(
    config: &Config,
) -> Result<RpcOutcome<serde_json::Value>, String> {
    let token = get_session_token(config)?;
    Ok(RpcOutcome::single_log(
        json!({ "token": token }),
        "session token fetched",
    ))
}
