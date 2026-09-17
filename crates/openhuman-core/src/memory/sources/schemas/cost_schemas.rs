//! Schemas and handlers for the sync audit log, per-source cost estimate, and
//! the monthly cost summary.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::memory::sources::rpc;

use super::{parse_value, to_json, NAMESPACE};

pub(super) fn schema(function: &str) -> Option<ControllerSchema> {
    Some(match function {
        "sync_audit_log" => ControllerSchema {
            namespace: NAMESPACE,
            function: "sync_audit_log",
            description:
                "Sync audit history — timestamp, tokens consumed, cost, duration for each sync run.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "entries",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("SyncAuditEntry"))),
                comment: "Audit entries, most recent first.",
                required: true,
            }],
        },
        "estimate_sync_cost" => ControllerSchema {
            namespace: NAMESPACE,
            function: "estimate_sync_cost",
            description:
                "Estimate the cost of syncing a source before starting. Returns item count, \
                 estimated tokens, and estimated cost in USD.",
            inputs: vec![FieldSchema {
                name: "source_id",
                ty: TypeSchema::String,
                comment: "Source id to estimate.",
                required: true,
            }],
            outputs: vec![
                FieldSchema {
                    name: "source_id",
                    ty: TypeSchema::String,
                    comment: "Echo of source id.",
                    required: true,
                },
                FieldSchema {
                    name: "item_count",
                    ty: TypeSchema::U64,
                    comment: "Number of items to sync.",
                    required: true,
                },
                FieldSchema {
                    name: "estimated_tokens",
                    ty: TypeSchema::U64,
                    comment: "Estimated input tokens.",
                    required: true,
                },
                FieldSchema {
                    name: "estimated_cost_usd",
                    ty: TypeSchema::F64,
                    comment: "Estimated cost in USD.",
                    required: true,
                },
                FieldSchema {
                    name: "budget_max_cost_usd",
                    ty: TypeSchema::Option(Box::new(TypeSchema::F64)),
                    comment: "Configured cost cap if set.",
                    required: false,
                },
                FieldSchema {
                    name: "budget_max_tokens",
                    ty: TypeSchema::Option(Box::new(TypeSchema::U64)),
                    comment: "Configured token cap if set.",
                    required: false,
                },
            ],
        },
        "monthly_cost_summary" => ControllerSchema {
            namespace: NAMESPACE,
            function: "monthly_cost_summary",
            description: "Aggregate sync costs for the current calendar month.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "month",
                    ty: TypeSchema::String,
                    comment: "YYYY-MM.",
                    required: true,
                },
                FieldSchema {
                    name: "total_cost_usd",
                    ty: TypeSchema::F64,
                    comment: "Total spend in USD.",
                    required: true,
                },
                FieldSchema {
                    name: "total_syncs",
                    ty: TypeSchema::U64,
                    comment: "Number of sync runs.",
                    required: true,
                },
                FieldSchema {
                    name: "total_items",
                    ty: TypeSchema::U64,
                    comment: "Total items fetched.",
                    required: true,
                },
                FieldSchema {
                    name: "total_input_tokens",
                    ty: TypeSchema::U64,
                    comment: "Total input tokens.",
                    required: true,
                },
                FieldSchema {
                    name: "total_output_tokens",
                    ty: TypeSchema::U64,
                    comment: "Total output tokens.",
                    required: true,
                },
            ],
        },
        _ => return None,
    })
}

pub(super) fn handle_sync_audit_log(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::sync_audit_log_rpc().await?) })
}

pub(super) fn handle_estimate_sync_cost(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let req = parse_value::<rpc::EstimateSyncCostRequest>(Value::Object(params))?;
        to_json(rpc::estimate_sync_cost_rpc(req).await?)
    })
}

pub(super) fn handle_monthly_cost_summary(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::monthly_cost_summary_rpc().await?) })
}
