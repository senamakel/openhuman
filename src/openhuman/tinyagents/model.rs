//! `tinyagents` [`ChatModel`] adapter over an openhuman [`Provider`] (issue #4249).
//!
//! Wraps `Arc<dyn Provider>` so the `tinyagents` agent-loop can drive a real
//! openhuman inference backend. On each model call the harness hands us a
//! provider-neutral [`ModelRequest`] (rich messages + advertised tool schemas);
//! we translate it into an openhuman [`ChatRequest`], call `provider.chat`, and
//! translate the [`ChatResponse`] back into a harness [`ModelResponse`] —
//! carrying through text, native tool calls, and token usage.

use std::sync::Arc;

use async_trait::async_trait;
use tinyagents::harness::message::{AssistantMessage, ContentBlock};
use tinyagents::harness::model::{ChatModel, ModelRequest, ModelResponse};
use tinyagents::harness::tool::ToolCall as TaToolCall;
use tinyagents::harness::usage::Usage;

use crate::openhuman::inference::provider::{ChatMessage, ChatRequest, Provider};
use crate::openhuman::tools::ToolSpec;

/// A harness chat model backed by an openhuman [`Provider`].
///
/// The application `State` is `()` — openhuman tools and providers carry no
/// harness-visible shared state — so this adapter implements
/// `ChatModel<()>`.
pub struct ProviderModel {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    max_tokens: Option<u32>,
}

impl ProviderModel {
    /// Build a model adapter for `provider`, pinned to `model`/`temperature`.
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>, temperature: f64) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
            max_tokens: None,
        }
    }

    /// Cap the output tokens requested from the provider for every call.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

#[async_trait]
impl ChatModel<()> for ProviderModel {
    async fn invoke(
        &self,
        _state: &(),
        request: ModelRequest,
    ) -> tinyagents::Result<ModelResponse> {
        // Translate the harness transcript + tool schemas into openhuman terms.
        let messages: Vec<ChatMessage> = request
            .messages
            .iter()
            .map(super::convert::message_to_chat_message)
            .collect();
        let specs: Vec<ToolSpec> = request
            .tools
            .iter()
            .map(|s| ToolSpec {
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.parameters.clone(),
            })
            .collect();

        let chat_request = ChatRequest {
            messages: &messages,
            tools: if specs.is_empty() { None } else { Some(&specs) },
            stream: None,
            max_tokens: self.max_tokens,
        };

        tracing::debug!(
            model = %self.model,
            messages = messages.len(),
            tools = specs.len(),
            "[tinyagents] provider.chat via harness model adapter"
        );

        let response = self
            .provider
            .chat(chat_request, &self.model, self.temperature)
            .await
            .map_err(|e| {
                tinyagents::TinyAgentsError::Model(format!("openhuman provider chat failed: {e}"))
            })?;

        // Build the assistant message: visible text + any native tool calls.
        let mut content = Vec::new();
        if let Some(text) = response.text.as_ref().filter(|t| !t.is_empty()) {
            content.push(ContentBlock::Text(text.clone()));
        }
        let tool_calls: Vec<TaToolCall> = response
            .tool_calls
            .iter()
            .map(|tc| TaToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null),
            })
            .collect();

        let usage = response
            .usage
            .as_ref()
            .map(|u| Usage::new(u.input_tokens, u.output_tokens));

        let finish_reason = if tool_calls.is_empty() {
            "stop"
        } else {
            "tool_calls"
        };

        let message = AssistantMessage {
            id: None,
            content,
            tool_calls,
            usage,
        };
        Ok(ModelResponse {
            message,
            usage,
            finish_reason: Some(finish_reason.to_string()),
            raw: None,
            resolved_model: None,
        })
    }
}
