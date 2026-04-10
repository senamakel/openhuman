//! Minimal LLM client for the discovery agent.
//!
//! The discovery agent only needs a single capability: "send this chat
//! history + tool set to the backend's OpenAI-compatible endpoint and
//! tell me whether the assistant wants to call a tool or stop". We do
//! NOT want to pull in the full `providers/` stack because:
//!
//! 1. It's designed for interactive multi-turn sessions with streaming
//!    and per-provider quirks — overkill for a one-shot background job.
//! 2. Wrapping it behind a trait here lets tests stub responses
//!    deterministically without running a real model.
//!
//! Shape is OpenAI-style: `messages`, `tools`, `tool_choice`, and a
//! response with either text content or a `tool_calls` array.

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

/// Role of a single chat message in the LLM request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmRole {
    System,
    User,
    Assistant,
    Tool,
}

/// One message in the discovery agent's chat history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    /// Plain text content. For `Tool` messages this is the tool output.
    /// For `Assistant` messages that invoke tools, this is usually empty
    /// and the calls live in the sibling [`LlmResponse::tool_calls`].
    pub content: String,
    /// When `role == Tool`, the id this message is responding to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// When `role == Tool`, the tool name (helpful for models that
    /// don't echo it back).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl LlmMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::System,
            content: content.into(),
            tool_call_id: None,
            name: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::User,
            content: content.into(),
            tool_call_id: None,
            name: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: LlmRole::Assistant,
            content: content.into(),
            tool_call_id: None,
            name: None,
        }
    }
    pub fn tool(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: LlmRole::Tool,
            content: content.into(),
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
        }
    }
}

/// One tool invocation parsed from an assistant response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolCall {
    /// Opaque id the runner should echo back on the matching `Tool`
    /// reply message.
    pub id: String,
    /// Tool name the assistant wants to invoke (e.g. `"owner_write"`).
    pub name: String,
    /// Arguments as parsed JSON.
    pub arguments: Value,
}

/// The assistant's reply for a single round.
#[derive(Debug, Clone, Default)]
pub struct LlmResponse {
    /// Plain text content (may be empty when the assistant is purely
    /// calling tools).
    pub content: String,
    /// Any tool calls the assistant wants the runner to dispatch.
    pub tool_calls: Vec<LlmToolCall>,
    /// OpenAI-style finish reason, e.g. `"tool_calls"` or `"stop"`.
    pub finish_reason: String,
}

/// Errors surfaced from LLM calls.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("llm request failed: {0}")]
    Request(String),
    #[error("llm response parse failed: {0}")]
    Parse(String),
    #[error("llm returned non-success status {status}: {body}")]
    NonSuccess { status: u16, body: String },
}

/// Trait wrapping a single-round chat completion call. Object-safe so
/// tests can substitute a stub.
#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Send one round of the chat history plus the available tool
    /// schemas and return the assistant's reply.
    ///
    /// `tools` is a JSON array in OpenAI tool-schema format. An empty
    /// array disables tool calling.
    async fn chat(
        &self,
        messages: &[LlmMessage],
        tools: &[Value],
        model: &str,
    ) -> Result<LlmResponse, LlmError>;
}

/// Direct-reqwest implementation that hits
/// `{api_url}/openai/v1/chat/completions` on the OpenHuman backend with
/// the user's app-session JWT.
pub struct BackendLlmClient {
    http: reqwest::Client,
    endpoint: String,
    jwt: String,
}

impl BackendLlmClient {
    pub fn new(api_url: &str, jwt: impl Into<String>) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| LlmError::Request(format!("build reqwest client: {e}")))?;

        let base = api_url.trim_end_matches('/');
        Ok(Self {
            http,
            endpoint: format!("{base}/openai/v1/chat/completions"),
            jwt: jwt.into(),
        })
    }

    /// Convenience constructor pulling backend URL + JWT from config.
    pub fn from_config(config: &crate::openhuman::config::Config) -> Result<Self, LlmError> {
        let api_url = crate::api::config::effective_api_url(&config.api_url);
        let jwt = crate::api::jwt::get_session_token(config)
            .map_err(|e| LlmError::Request(format!("get_session_token: {e}")))?
            .ok_or_else(|| LlmError::Request("no app-session token available".into()))?;
        Self::new(&api_url, jwt)
    }
}

#[async_trait]
impl LlmClient for BackendLlmClient {
    async fn chat(
        &self,
        messages: &[LlmMessage],
        tools: &[Value],
        model: &str,
    ) -> Result<LlmResponse, LlmError> {
        let mut body = json!({
            "model": model,
            "messages": messages,
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.to_vec());
            body["tool_choice"] = Value::String("auto".into());
        }

        log::debug!(
            "[discovery:llm] POST {} model={} messages={} tools={}",
            self.endpoint,
            model,
            messages.len(),
            tools.len()
        );

        let resp = self
            .http
            .post(&self.endpoint)
            .header(AUTHORIZATION, format!("Bearer {}", self.jwt))
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Request(e.to_string()))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(LlmError::NonSuccess {
                status: status.as_u16(),
                body: text,
            });
        }

        parse_openai_chat_response(&text)
    }
}

/// Parse an OpenAI-compatible `/chat/completions` response into the
/// compact [`LlmResponse`] shape. Public so tests can reuse it.
pub fn parse_openai_chat_response(raw: &str) -> Result<LlmResponse, LlmError> {
    let v: Value = serde_json::from_str(raw).map_err(|e| LlmError::Parse(e.to_string()))?;
    let choice = v
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or_else(|| LlmError::Parse("no choices in response".into()))?;

    let finish_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .unwrap_or("stop")
        .to_string();

    let message = choice
        .get("message")
        .ok_or_else(|| LlmError::Parse("choice missing message".into()))?;

    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut tool_calls = Vec::new();
    if let Some(arr) = message.get("tool_calls").and_then(Value::as_array) {
        for call in arr {
            let id = call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let func = call
                .get("function")
                .ok_or_else(|| LlmError::Parse("tool_call missing function".into()))?;
            let name = func
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| LlmError::Parse("tool_call function missing name".into()))?
                .to_string();
            let args_raw = func
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            // Surface malformed tool-call arguments so the runner can treat
            // the round as a failed tool dispatch instead of silently dropping
            // the LLM's intent on the floor.
            let arguments: Value = serde_json::from_str(args_raw).map_err(|e| {
                LlmError::Parse(format!(
                    "tool_call '{name}' has malformed arguments JSON: {e}"
                ))
            })?;
            tool_calls.push(LlmToolCall {
                id,
                name,
                arguments,
            });
        }
    }

    Ok(LlmResponse {
        content,
        tool_calls,
        finish_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_text_response() {
        let raw = r#"{
            "choices": [{
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": "hello"}
            }]
        }"#;
        let parsed = parse_openai_chat_response(raw).unwrap();
        assert_eq!(parsed.content, "hello");
        assert_eq!(parsed.finish_reason, "stop");
        assert!(parsed.tool_calls.is_empty());
    }

    #[test]
    fn parses_tool_calls() {
        let raw = r#"{
            "choices": [{
                "finish_reason": "tool_calls",
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "owner_write",
                            "arguments": "{\"facts\":[{\"type\":\"identity\",\"key\":\"full_name\",\"value\":\"Ada\"}]}"
                        }
                    }]
                }
            }]
        }"#;
        let parsed = parse_openai_chat_response(raw).unwrap();
        assert_eq!(parsed.finish_reason, "tool_calls");
        assert_eq!(parsed.tool_calls.len(), 1);
        assert_eq!(parsed.tool_calls[0].name, "owner_write");
        assert_eq!(parsed.tool_calls[0].id, "call_1");
        assert!(parsed.tool_calls[0].arguments.is_object());
    }

    #[test]
    fn llm_message_tool_reply_includes_id_and_name() {
        let msg = LlmMessage::tool("call_1", "owner_write", "ok");
        assert_eq!(msg.role, LlmRole::Tool);
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(msg.name.as_deref(), Some("owner_write"));
    }
}
