//! Schemas and handlers for coding-session discovery and ingestion.

use serde_json::{Map, Value};

use crate::core::all::ControllerFuture;
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::memory::sources::rpc;

use super::{parse_value, to_json, NAMESPACE};

pub(super) fn schema(function: &str) -> Option<ControllerSchema> {
    Some(match function {
        "coding_session_status" => ControllerSchema {
            namespace: NAMESPACE,
            function: "coding_session_status",
            description: "Discover local Codex and Claude Code session histories and report the human-authored evidence available for memory ingestion.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "sources",
                ty: TypeSchema::Array(Box::new(TypeSchema::Ref("CodingSessionSourceStatus"))),
                comment: "Discovery and evidence counts for each supported coding-agent session source.",
                required: true,
            }],
        },
        "ingest_coding_sessions" => ControllerSchema {
            namespace: NAMESPACE,
            function: "ingest_coding_sessions",
            description: "Distill human-authored turns from local Codex and Claude Code sessions into the TinyCortex persona memory layer.",
            inputs: vec![
                FieldSchema {
                    name: "backfill",
                    ty: TypeSchema::Bool,
                    comment: "When true, reprocess all discovered sessions; otherwise ingest only changed sessions.",
                    required: false,
                },
                FieldSchema {
                    name: "max_sessions",
                    ty: TypeSchema::U64,
                    comment: "Maximum session digests for this run (clamped to 1,000).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema { name: "mode", ty: TypeSchema::String, comment: "Executed run mode.", required: true },
                FieldSchema { name: "files_seen", ty: TypeSchema::U64, comment: "Discovered coding-session files.", required: true },
                FieldSchema { name: "sessions_processed", ty: TypeSchema::U64, comment: "Coding sessions distilled successfully.", required: true },
                FieldSchema { name: "sessions_skipped", ty: TypeSchema::U64, comment: "Unchanged sessions skipped during an incremental run.", required: true },
                FieldSchema { name: "sessions_failed", ty: TypeSchema::U64, comment: "Sessions retained for retry after provider failure.", required: true },
                FieldSchema { name: "evidence_units", ty: TypeSchema::U64, comment: "Human-authored evidence units extracted.", required: true },
                FieldSchema { name: "observations", ty: TypeSchema::U64, comment: "Persona observations distilled.", required: true },
                FieldSchema { name: "budget_hit", ty: TypeSchema::Bool, comment: "Whether the run stopped at its session/call budget.", required: true },
                FieldSchema { name: "pack_path", ty: TypeSchema::Option(Box::new(TypeSchema::String)), comment: "Compiled persona pack path when written.", required: false },
            ],
        },
        _ => return None,
    })
}

pub(super) fn handle_coding_session_status(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move { to_json(rpc::coding_session_status_rpc().await?) })
}

pub(super) fn handle_ingest_coding_sessions(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        // `rpc::CodingSessionIngestRequest`, not the engine path: this adapter
        // names its own domain's request type, the way every sibling here does.
        // See the re-export's docs in `rpc/coding_sessions.rs` for why the type
        // is still the engine's underneath (#5560).
        let req = parse_value::<rpc::CodingSessionIngestRequest>(Value::Object(params))?;
        to_json(rpc::ingest_coding_sessions_rpc(req).await?)
    })
}
