//! Controller-registry schemas for `openhuman.memory_sources_*`.
//!
//! Split by responsibility: [`registry_schemas`] (list/get/add/update/remove/
//! list_items/read_item), [`sync_schemas`] (sync/reconcile),
//! [`status_schemas`] (status_list/supported_toolkits), [`cost_schemas`]
//! (sync_audit_log/estimate_sync_cost/monthly_cost_summary),
//! [`apply_all_schemas`] (apply_all_in), and [`coding_session_schemas`]
//! (coding_session_status/ingest_coding_sessions). This file keeps the
//! aggregate `schemas()` dispatcher, `all_controller_schemas`,
//! `all_registered_controllers`, and the small `parse_value`/`to_json`
//! helpers every handler shares.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::core::all::RegisteredController;
use crate::core::ControllerSchema;
use crate::rpc::RpcOutcome;

mod apply_all_schemas;
mod coding_session_schemas;
mod cost_schemas;
mod registry_schemas;
mod status_schemas;
mod sync_schemas;

#[cfg(test)]
#[path = "schemas_tests.rs"]
mod tests;

const NAMESPACE: &str = "memory_sources";

pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        schemas("list"),
        schemas("get"),
        schemas("add"),
        schemas("update"),
        schemas("remove"),
        schemas("list_items"),
        schemas("read_item"),
        schemas("sync"),
        schemas("reconcile"),
        schemas("status_list"),
        schemas("supported_toolkits"),
        schemas("sync_audit_log"),
        schemas("estimate_sync_cost"),
        schemas("monthly_cost_summary"),
        schemas("apply_all_in"),
        schemas("coding_session_status"),
        schemas("ingest_coding_sessions"),
    ]
}

pub fn all_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: schemas("list"),
            handler: registry_schemas::handle_list,
        },
        RegisteredController {
            schema: schemas("get"),
            handler: registry_schemas::handle_get,
        },
        RegisteredController {
            schema: schemas("add"),
            handler: registry_schemas::handle_add,
        },
        RegisteredController {
            schema: schemas("update"),
            handler: registry_schemas::handle_update,
        },
        RegisteredController {
            schema: schemas("remove"),
            handler: registry_schemas::handle_remove,
        },
        RegisteredController {
            schema: schemas("list_items"),
            handler: registry_schemas::handle_list_items,
        },
        RegisteredController {
            schema: schemas("read_item"),
            handler: registry_schemas::handle_read_item,
        },
        RegisteredController {
            schema: schemas("sync"),
            handler: sync_schemas::handle_sync,
        },
        RegisteredController {
            schema: schemas("reconcile"),
            handler: sync_schemas::handle_reconcile,
        },
        RegisteredController {
            schema: schemas("status_list"),
            handler: status_schemas::handle_status_list,
        },
        RegisteredController {
            schema: schemas("supported_toolkits"),
            handler: status_schemas::handle_supported_toolkits,
        },
        RegisteredController {
            schema: schemas("sync_audit_log"),
            handler: cost_schemas::handle_sync_audit_log,
        },
        RegisteredController {
            schema: schemas("estimate_sync_cost"),
            handler: cost_schemas::handle_estimate_sync_cost,
        },
        RegisteredController {
            schema: schemas("monthly_cost_summary"),
            handler: cost_schemas::handle_monthly_cost_summary,
        },
        RegisteredController {
            schema: schemas("apply_all_in"),
            handler: apply_all_schemas::handle_apply_all_in,
        },
        RegisteredController {
            schema: schemas("coding_session_status"),
            handler: coding_session_schemas::handle_coding_session_status,
        },
        RegisteredController {
            schema: schemas("ingest_coding_sessions"),
            handler: coding_session_schemas::handle_ingest_coding_sessions,
        },
    ]
}

pub fn schemas(function: &str) -> ControllerSchema {
    registry_schemas::schema(function)
        .or_else(|| sync_schemas::schema(function))
        .or_else(|| status_schemas::schema(function))
        .or_else(|| cost_schemas::schema(function))
        .or_else(|| apply_all_schemas::schema(function))
        .or_else(|| coding_session_schemas::schema(function))
        .unwrap_or_else(|| panic!("unknown memory_sources schema function: {function}"))
}

fn parse_value<T: DeserializeOwned>(v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("invalid params: {e}"))
}

fn to_json<T: serde::Serialize>(outcome: RpcOutcome<T>) -> Result<Value, String> {
    outcome.into_cli_compatible_json()
}
