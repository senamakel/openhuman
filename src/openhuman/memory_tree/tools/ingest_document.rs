use crate::openhuman::config::rpc as config_rpc;
use crate::openhuman::memory::tree_rpc as rpc;
use crate::openhuman::memory_store::chunks::types::SourceKind;
use crate::openhuman::memory_sync::canonicalize::document::DocumentInput;
use crate::openhuman::tools::traits::{Tool, ToolResult};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;

pub struct MemoryTreeIngestDocumentTool;

#[async_trait]
impl Tool for MemoryTreeIngestDocumentTool {
    fn name(&self) -> &str {
        "memory_tree_ingest_document"
    }

    fn description(&self) -> &str {
        "Ingest a document into the memory tree for future retrieval. \
         This is the write path into the knowledge index — use it after \
         fetching web content, extracting facts, or collecting data from \
         external sources. The ingested document will be chunked, embedded, \
         and available via query_global, query_source, and search_entities."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Document title (e.g. 'ROOT v6.36.12 Release Notes')."
                },
                "body": {
                    "type": "string",
                    "description": "Document body in markdown or plain text."
                },
                "source_id": {
                    "type": "string",
                    "description": "Stable source identifier (e.g. 'root_releases', 'github_root_changelog'). Re-ingesting with same source_id replaces old chunks."
                },
                "provider": {
                    "type": "string",
                    "description": "Source provider name (e.g. 'github', 'web', 'root_docs'). Defaults to 'agent'."
                },
                "source_ref": {
                    "type": "string",
                    "description": "Optional URL or pointer back to the original source."
                },
                "owner": {
                    "type": "string",
                    "description": "Optional account/user this content belongs to. Used for owner-scoped queries and attribution. Defaults to empty (unowned/agent-global)."
                }
            },
            "required": ["title", "body", "source_id"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> anyhow::Result<ToolResult> {
        log::debug!("[tool][memory_tree] ingest_document invoked");

        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("ingest_document: missing required field `title`"))?
            .to_string();
        let body = args
            .get("body")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("ingest_document: missing required field `body`"))?
            .to_string();
        let source_id = args
            .get("source_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("ingest_document: missing required field `source_id`"))?
            .trim()
            .to_string();
        let provider = args
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("agent")
            .to_string();
        let source_ref = args
            .get("source_ref")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let owner = args
            .get("owner")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if title.trim().is_empty() || body.trim().is_empty() || source_id.is_empty() {
            return Ok(ToolResult::error(
                "ingest_document: title, body, and source_id must be non-empty".to_string(),
            ));
        }

        let cfg = config_rpc::load_config_with_timeout().await.map_err(|e| {
            log::debug!("[tool][memory_tree] ingest_document config_load_failed err={e}");
            anyhow::anyhow!("ingest_document: load config failed: {e}")
        })?;

        let doc = DocumentInput {
            provider,
            title: title.trim().to_string(),
            body: body.trim().to_string(),
            modified_at: Utc::now(),
            source_ref,
        };

        let req = rpc::IngestRequest {
            source_kind: SourceKind::Document,
            source_id: source_id.clone(),
            owner,
            tags: vec!["agent_ingested".to_string()],
            payload: serde_json::to_value(&doc).map_err(|e| {
                log::debug!("[tool][memory_tree] ingest_document payload_serialize_failed err={e}");
                anyhow::anyhow!("ingest_document: failed to serialize payload: {e}")
            })?,
        };

        let outcome = rpc::ingest_rpc(&cfg, req).await.map_err(|e| {
            log::debug!(
                "[tool][memory_tree] ingest_document rpc_failed source_id={source_id} err={e}"
            );
            anyhow::anyhow!("ingest_document: ingestion failed: {e}")
        })?;

        let n = outcome.value.chunks_written;
        log::info!(
            "[tool][memory_tree] ingest_document done source_id={} chunks={}",
            source_id,
            n
        );
        Ok(ToolResult::success(format!(
            "Ingested document \"{}\" as source_id={}. {} chunks created and indexed.",
            title, source_id, n
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::tools::traits::Tool;
    use serde_json::json;

    #[test]
    fn parameters_schema_requires_title_body_and_source_id() {
        let tool = MemoryTreeIngestDocumentTool;
        let schema = tool.parameters_schema();
        assert_eq!(schema["required"], json!(["title", "body", "source_id"]));
        assert_eq!(schema["properties"]["provider"]["type"], "string");
    }

    #[test]
    fn missing_required_fields_produce_none_via_json_accessors() {
        let value = json!({
            "title": "Doc title",
            "body": "Body"
        });
        assert_eq!(value.get("source_id").and_then(|v| v.as_str()), None);
    }

    #[test]
    fn source_kind_document_string_is_expected() {
        assert_eq!(SourceKind::Document.as_str(), "document");
    }

    #[tokio::test]
    async fn execute_rejects_missing_title_before_config_load() {
        let tool = MemoryTreeIngestDocumentTool;
        let err = tool
            .execute(json!({
                "body": "Body text",
                "source_id": "doc-1"
            }))
            .await
            .expect_err("missing title should fail");
        assert!(
            err.to_string()
                .contains("ingest_document: missing required field `title`")
        );
    }

    #[tokio::test]
    async fn execute_rejects_missing_body_before_config_load() {
        let tool = MemoryTreeIngestDocumentTool;
        let err = tool
            .execute(json!({
                "title": "Doc title",
                "source_id": "doc-1"
            }))
            .await
            .expect_err("missing body should fail");
        assert!(
            err.to_string()
                .contains("ingest_document: missing required field `body`")
        );
    }

    #[tokio::test]
    async fn execute_rejects_missing_source_id_before_config_load() {
        let tool = MemoryTreeIngestDocumentTool;
        let err = tool
            .execute(json!({
                "title": "Doc title",
                "body": "Body text"
            }))
            .await
            .expect_err("missing source_id should fail");
        assert!(
            err.to_string()
                .contains("ingest_document: missing required field `source_id`")
        );
    }

    #[tokio::test]
    async fn execute_rejects_blank_required_fields() {
        let tool = MemoryTreeIngestDocumentTool;
        let result = tool
            .execute(json!({
                "title": "   ",
                "body": "Body text",
                "source_id": "doc-1"
            }))
            .await
            .expect("blank title should return ToolResult error, not anyhow failure");
        assert!(result.is_error);
        assert_eq!(
            result.text(),
            "ingest_document: title, body, and source_id must be non-empty"
        );

        let result = tool
            .execute(json!({
                "title": "Doc title",
                "body": "   ",
                "source_id": "doc-1"
            }))
            .await
            .expect("blank body should return ToolResult error");
        assert!(result.is_error);

        let result = tool
            .execute(json!({
                "title": "Doc title",
                "body": "Body text",
                "source_id": "   "
            }))
            .await
            .expect("blank source_id should return ToolResult error");
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn execute_success_path_returns_ingest_confirmation_after_validation() {
        let tool = MemoryTreeIngestDocumentTool;
        let result = tool
            .execute(json!({
                "title": "Doc title",
                "body": "Body text",
                "source_id": "doc-1",
                "provider": "web",
                "owner": "owner-1"
            }))
            .await
            .expect("valid request should succeed in the local test environment");
        assert!(!result.is_error);
        let text = result.text();
        assert!(
            text.contains("Ingested document \"Doc title\" as source_id=doc-1."),
            "unexpected success payload: {text}"
        );
    }
}
