//! `#[derive(Deserialize)]` request-parameter shapes for the handlers in
//! [`super::handlers_triggers`] and [`super::handlers_connections`] that
//! take more than one or two scalar fields.

use serde_json::Value;

#[derive(Debug, serde::Deserialize)]
pub(super) struct TriggerHistoryParams {
    pub(super) limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ListGithubReposParams {
    pub(super) connection_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct CreateTriggerParams {
    pub(super) slug: String,
    pub(super) connection_id: Option<String>,
    pub(super) trigger_config: Option<Value>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ListAvailableTriggersParams {
    pub(super) toolkit: String,
    pub(super) connection_id: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct ListTriggersParams {
    pub(super) toolkit: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(super) struct EnableTriggerParams {
    pub(super) connection_id: String,
    pub(super) slug: String,
    pub(super) trigger_config: Option<Value>,
}
