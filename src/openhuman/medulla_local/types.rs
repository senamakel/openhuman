//! Domain types for the `medulla_local` flow: the `hello` handshake, the
//! `instruct`/`status` request results, and the `inference` port-callback
//! request/response shapes (§3, §4, §5.1 of the serve protocol spec).
//!
//! These mirror the medulla-v1 types named in the spec exactly enough to
//! decode/encode the wire; fields the draft does not consume are kept
//! `#[serde(default)]` / optional so a serve version that adds fields never
//! breaks the host.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Reserved error codes (§8). A port callback the host cannot answer returns
/// one of these in a `ret` error envelope.
pub mod error_codes {
    pub const BAD_REQUEST: &str = "bad_request";
    pub const NOT_READY: &str = "not_ready";
    pub const UNKNOWN_OP: &str = "unknown_op";
    pub const UNKNOWN_PORT: &str = "unknown_port";
    pub const UNSUPPORTED_METHOD: &str = "unsupported_method";
    pub const PORT_UNAVAILABLE: &str = "port_unavailable";
    pub const TIMEOUT: &str = "timeout";
    pub const RETRYABLE: &str = "retryable";
    pub const INTERNAL: &str = "internal";
}

/// A host tool spec advertised in the `hello` request (`ToolSpec`, §3). The
/// serve side binds these into its module registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// The `hello` request params the host sends after receiving `ready` (§3).
/// `ports` is the set of port callbacks the host will answer; the active set
/// is the intersection with serve's advertised `capabilities`.
#[derive(Debug, Clone, Serialize)]
pub struct HelloParams {
    pub protocol: u32,
    pub host: String,
    pub ports: Vec<String>,
    pub tools: Vec<ToolSpec>,
}

/// The `hello` response (§3): the negotiated active port set.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HelloResult {
    #[serde(default)]
    pub protocol: u32,
    #[serde(default, rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub ports: Vec<String>,
}

/// Synchronous receipt for an `instruct` (§4.1). The cycle itself runs async
/// and is observed via `event` frames.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct InstructReceipt {
    #[serde(rename = "instructionId")]
    pub instruction_id: String,
    #[serde(rename = "cycleId")]
    pub cycle_id: String,
}

/// Token accounting shared by `status` and `inference` (§4.4, §5.1).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct Usage {
    #[serde(default, rename = "inputTokens")]
    pub input_tokens: u64,
    #[serde(default, rename = "outputTokens")]
    pub output_tokens: u64,
}

/// Snapshot of `HarnessStatus` (§4.4). Unmodelled sub-shapes ride as raw
/// `Value` so a richer serve status never fails to decode.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct HarnessStatus {
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub queued: u64,
    #[serde(default, rename = "activeInstructionId")]
    pub active_instruction_id: Option<String>,
    #[serde(default, rename = "activeCycleId")]
    pub active_cycle_id: Option<String>,
    #[serde(default)]
    pub tasks: Vec<Value>,
    #[serde(default, rename = "runningDelegations")]
    pub running_delegations: u64,
    #[serde(default)]
    pub usage: Value,
    #[serde(default)]
    pub escalations: Vec<Value>,
}

/// A single chat message crossing the `inference` port (§5.1).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WireChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
}

/// The `inference.invoke` port-callback params (§5.1). `tier` is one of
/// `orchestrator` / `reasoning` / `compress`; the host maps it onto its
/// per-role model routing.
#[derive(Debug, Clone, Deserialize)]
pub struct InferenceCall {
    #[serde(default)]
    pub tier: String,
    #[serde(default)]
    pub op: String,
    #[serde(default, rename = "cycleId")]
    pub cycle_id: Option<String>,
    #[serde(default)]
    pub messages: Vec<WireChatMessage>,
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default)]
    pub meta: Value,
}

/// A tool call the model requested, returned inside [`InferenceResult`] (§5.1).
#[derive(Debug, Clone, Default, Serialize)]
pub struct WireToolCall {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// The `InferenceResult` a host returns for an `inference.invoke` (§5.1).
#[derive(Debug, Clone, Default, Serialize)]
pub struct InferenceResult {
    pub content: String,
    #[serde(rename = "reasoningContent")]
    pub reasoning_content: Option<String>,
    pub model: String,
    #[serde(rename = "toolCalls")]
    pub tool_calls: Vec<WireToolCall>,
    pub usage: Usage,
}

/// Status of the supervised serve child, surfaced over the `medulla_local`
/// RPC namespace.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MedullaLocalStatus {
    /// Whether the `medulla-local` engine is selected/available.
    pub enabled: bool,
    /// Whether a serve child is currently connected.
    pub running: bool,
    /// serve version string from the `ready` banner, if connected.
    pub serve_version: Option<String>,
    /// Session id negotiated in the handshake, if connected.
    pub session_id: Option<String>,
    /// Active port set negotiated in `hello`.
    pub ports: Vec<String>,
    /// Last error, if the supervisor is in a failed/unavailable state.
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn instruct_receipt_round_trips_camel_case() {
        let receipt: InstructReceipt = serde_json::from_str(
            r#"{"instructionId":"inst-agent-0","cycleId":"cyc:agent:agent:0"}"#,
        )
        .unwrap();
        assert_eq!(receipt.instruction_id, "inst-agent-0");
        assert_eq!(receipt.cycle_id, "cyc:agent:agent:0");
    }

    #[test]
    fn inference_call_decodes_tier_and_messages() {
        let call: InferenceCall = serde_json::from_value(json!({
            "tier": "orchestrator",
            "op": "orchestrate",
            "cycleId": "cyc:1",
            "messages": [{"role": "user", "content": "hi"}],
            "meta": {"priority": "root"}
        }))
        .unwrap();
        assert_eq!(call.tier, "orchestrator");
        assert_eq!(call.messages.len(), 1);
        assert_eq!(call.messages[0].role, "user");
    }

    #[test]
    fn inference_result_serializes_camel_case() {
        let result = InferenceResult {
            content: "done".into(),
            reasoning_content: None,
            model: "orchestrator-v1".into(),
            tool_calls: vec![],
            usage: Usage {
                input_tokens: 9,
                output_tokens: 3,
            },
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(value["content"], "done");
        assert_eq!(value["reasoningContent"], Value::Null);
        assert_eq!(value["usage"]["inputTokens"], 9);
        assert_eq!(value["toolCalls"], json!([]));
    }

    #[test]
    fn harness_status_tolerates_sparse_payload() {
        let status: HarnessStatus =
            serde_json::from_str(r#"{"state":"running","queued":1}"#).unwrap();
        assert_eq!(status.state, "running");
        assert_eq!(status.queued, 1);
        assert!(status.tasks.is_empty());
    }
}
