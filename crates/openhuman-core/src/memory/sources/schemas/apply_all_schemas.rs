//! Schema and handler for the "apply all in" sweep controller.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::memory::sources::rpc;

use super::{to_json, NAMESPACE};

pub(super) fn schema(function: &str) -> Option<ControllerSchema> {
    Some(match function {
        "apply_all_in" => ControllerSchema {
            namespace: NAMESPACE,
            function: "apply_all_in",
            description: "Enable ALL memory sources, clear all per-source caps, \
                          and trigger a background sync for every source. \
                          Returns immediately with the updated source list and \
                          the count of sync tasks queued.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "sources",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Ref("MemorySourceEntry"))),
                    comment: "All memory sources after the all-in transformation.",
                    required: true,
                },
                FieldSchema {
                    name: "sync_triggered",
                    ty: TypeSchema::U64,
                    comment: "Number of sync tasks spawned.",
                    required: true,
                },
                FieldSchema {
                    name: "sync_failed",
                    ty: TypeSchema::U64,
                    comment: "Number of enabled sources whose sync trigger failed (openhuman#5820).",
                    required: true,
                },
                FieldSchema {
                    name: "sync_errors",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "One `<source_id>: <error>` line per failed trigger; absent when none failed.",
                    required: false,
                },
            ],
        },
        _ => return None,
    })
}

pub(super) fn handle_apply_all_in(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::apply_all_in_rpc().await?) })
}
