//! Schemas and handlers for the per-source sync and reconcile controllers.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::memory::sources::rpc;

use super::{parse_value, to_json, NAMESPACE};

pub(super) fn schema(function: &str) -> Option<ControllerSchema> {
    Some(match function {
        "sync" => ControllerSchema {
            namespace: NAMESPACE,
            function: "sync",
            description: "Trigger a sync for a memory source. Returns immediately; \
                          progress is published as MemorySyncStageChanged events.",
            inputs: vec![FieldSchema {
                name: "source_id",
                ty: TypeSchema::String,
                comment: "Source id to sync.",
                required: true,
            }],
            outputs: vec![
                FieldSchema {
                    name: "requested",
                    ty: TypeSchema::Bool,
                    comment: "True when the sync was queued.",
                    required: true,
                },
                FieldSchema {
                    name: "source_id",
                    ty: TypeSchema::String,
                    comment: "Echo of the requested source id.",
                    required: true,
                },
            ],
        },
        "reconcile" => ControllerSchema {
            namespace: NAMESPACE,
            function: "reconcile",
            description: "Report raw-archive vs memory-tree coverage per source scope; \
                          with execute=true, start a background incremental reconcile \
                          (summarise + ingest) for every scope with pending files. The \
                          same reconcile also runs automatically after each sync.",
            inputs: vec![
                FieldSchema {
                    name: "source_id",
                    ty: TypeSchema::String,
                    comment: "Restrict to one source id; omit for all enabled sources.",
                    required: false,
                },
                FieldSchema {
                    name: "execute",
                    ty: TypeSchema::Bool,
                    comment: "Start background reconcile for scopes with pending files \
                              (default false = report only).",
                    required: false,
                },
            ],
            outputs: vec![FieldSchema {
                name: "scopes",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("ReconcileScopeReport"))),
                comment: "Per-scope coverage: total raw files, covered, pending, started.",
                required: true,
            }],
        },
        _ => return None,
    })
}

pub(super) fn handle_sync(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::SyncRequest>(Value::Object(params))?;
        to_json(rpc::sync_rpc(req).await?)
    })
}

pub(super) fn handle_reconcile(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::ReconcileRequest>(Value::Object(params))?;
        to_json(rpc::reconcile_rpc(req).await?)
    })
}
