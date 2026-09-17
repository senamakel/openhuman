//! Schemas and handlers for per-source status and the supported-toolkit
//! catalog.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::memory::sources::rpc;

use super::{to_json, NAMESPACE};

pub(super) fn schema(function: &str) -> Option<ControllerSchema> {
    Some(match function {
        "status_list" => ControllerSchema {
            namespace: NAMESPACE,
            function: "status_list",
            description: "Per-source sync status — chunks ingested, freshness label, \
                          last-chunk timestamp.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "statuses",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("SourceStatus"))),
                comment: "One row per configured memory source.",
                required: true,
            }],
        },
        "supported_toolkits" => ControllerSchema {
            namespace: NAMESPACE,
            function: "supported_toolkits",
            description: "Toolkit slugs that ship a native memory-sync provider. \
                          The Add Source picker disables connections outside this set.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "toolkits",
                ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                comment: "Sorted, de-duplicated supported toolkit slugs.",
                required: true,
            }],
        },
        _ => return None,
    })
}

pub(super) fn handle_status_list(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::status_list_rpc().await?) })
}

pub(super) fn handle_supported_toolkits(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::supported_toolkits_rpc().await?) })
}
