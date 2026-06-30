//! `tinyagents` [`ChatModel`] adapter over an openhuman [`Provider`] (issue #4249).
//!
//! Wraps `Arc<dyn Provider>` so the `tinyagents` agent-loop can drive a real
//! openhuman inference backend. On each model call the harness hands us a
//! provider-neutral [`ModelRequest`] (rich messages + advertised tool schemas);
//! we translate it into an openhuman [`ChatRequest`], call `provider.chat`, and
//! translate the [`ChatResponse`] back into a harness [`ModelResponse`] —
//! carrying through text, native tool calls, and token usage.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use tinyagents::harness::message::{AssistantMessage, ContentBlock, MessageDelta};
use tinyagents::harness::model::{
    ChatModel, ModelRequest, ModelResponse, ModelStream, ModelStreamItem,
};
use tinyagents::harness::tool::ToolCall as TaToolCall;
use tinyagents::harness::usage::Usage;
use tokio::sync::mpsc::{Sender, UnboundedSender};

use super::observability::{IterationCursor, SubagentScope};
use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::inference::provider::{
    ChatMessage, ChatRequest, ChatResponse, Provider, ProviderDelta,
};
use crate::openhuman::tools::ToolSpec;

/// Out-of-band forwarder for model reasoning / thinking deltas.
///
/// tinyagents 0.2.0's streaming `MessageDelta` carries only visible text +
/// tool-call fragments — there is no reasoning channel on the harness stream —
/// so the [`OpenhumanEventBridge`](super::OpenhumanEventBridge) cannot mirror
/// thinking output. The model adapter is the only seam that sees the provider's
/// [`ProviderDelta::ThinkingDelta`], so it forwards reasoning straight onto the
/// progress sink here, sharing the bridge's [`IterationCursor`] so each delta is
/// attributed to the model call it belongs to. Parent runs emit
/// [`AgentProgress::ThinkingDelta`]; child runs emit the `Subagent` counterpart.
#[derive(Clone)]
pub struct ThinkingForwarder {
    sink: Sender<AgentProgress>,
    scope: Option<SubagentScope>,
    cursor: IterationCursor,
}

impl ThinkingForwarder {
    pub fn new(
        sink: Sender<AgentProgress>,
        scope: Option<SubagentScope>,
        cursor: IterationCursor,
    ) -> Self {
        Self {
            sink,
            scope,
            cursor,
        }
    }

    /// Best-effort, non-blocking emit of one reasoning chunk (drops on a full
    /// channel, matching the streaming text path).
    fn emit(&self, delta: String) {
        if delta.is_empty() {
            return;
        }
        let iteration = self.cursor.load(Ordering::SeqCst);
        let progress = match &self.scope {
            None => AgentProgress::ThinkingDelta { delta, iteration },
            Some(s) => AgentProgress::SubagentThinkingDelta {
                agent_id: s.agent_id.clone(),
                task_id: s.task_id.clone(),
                delta,
                iteration,
            },
        };
        let _ = self.sink.try_send(progress);
    }
}

/// Translate a harness [`ModelRequest`] into openhuman's message list + tool
/// specs (shared by the buffered and streaming paths).
fn build_chat_inputs(request: &ModelRequest) -> (Vec<ChatMessage>, Vec<ToolSpec>) {
    let messages = request
        .messages
        .iter()
        .map(super::convert::message_to_chat_message)
        .collect();
    let specs = request
        .tools
        .iter()
        .map(|s| ToolSpec {
            name: s.name.clone(),
            description: s.description.clone(),
            parameters: s.parameters.clone(),
        })
        .collect();
    (messages, specs)
}

/// Translate an openhuman [`ChatResponse`] into a harness [`ModelResponse`]
/// (visible text + native tool calls + token usage).
fn response_to_model_response(response: &ChatResponse) -> ModelResponse {
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
    ModelResponse {
        message: AssistantMessage {
            id: None,
            content,
            tool_calls,
            usage,
        },
        usage,
        finish_reason: Some(finish_reason.to_string()),
        raw: None,
        resolved_model: None,
    }
}

/// Forward one openhuman [`ProviderDelta`]. Visible text becomes a harness
/// [`MessageDelta`] (so the bridge mirrors it as a text delta); reasoning rides
/// the out-of-band [`ThinkingForwarder`] (the harness stream has no reasoning
/// channel); tool-call fragments are not streamed (the final `Completed`
/// response carries the native tool calls verbatim).
fn forward_delta(
    tx: &UnboundedSender<ModelStreamItem>,
    thinking: Option<&ThinkingForwarder>,
    delta: ProviderDelta,
) {
    match delta {
        ProviderDelta::TextDelta { delta } => {
            if !delta.is_empty() {
                let _ = tx.send(ModelStreamItem::MessageDelta(MessageDelta {
                    text: delta,
                    tool_call: None,
                }));
            }
        }
        ProviderDelta::ThinkingDelta { delta } => {
            if let Some(forwarder) = thinking {
                forwarder.emit(delta);
            }
        }
        _ => {}
    }
}

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
    /// When set, the adapter forwards provider reasoning deltas onto the
    /// progress sink (the harness stream has no reasoning channel).
    thinking: Option<ThinkingForwarder>,
}

impl ProviderModel {
    /// Build a model adapter for `provider`, pinned to `model`/`temperature`.
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>, temperature: f64) -> Self {
        Self {
            provider,
            model: model.into(),
            temperature,
            max_tokens: None,
            thinking: None,
        }
    }

    /// Cap the output tokens requested from the provider for every call.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Forward provider reasoning / thinking deltas onto a progress sink via
    /// `forwarder` (parent or sub-agent scoped). See [`ThinkingForwarder`].
    pub fn with_thinking(mut self, forwarder: ThinkingForwarder) -> Self {
        self.thinking = Some(forwarder);
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
        let (messages, specs) = build_chat_inputs(&request);
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
        // Non-streaming path: surface any reasoning the provider returned as a
        // single post-hoc thinking delta (it had no per-token channel to ride).
        if let Some(forwarder) = &self.thinking {
            if let Some(reasoning) = response
                .reasoning_content
                .as_ref()
                .filter(|r| !r.is_empty())
            {
                forwarder.emit(reasoning.clone());
            }
        }
        Ok(response_to_model_response(&response))
    }

    /// Stream the model response, forwarding openhuman's `ProviderDelta` events
    /// as harness [`ModelStreamItem`]s so the agent loop emits live `ModelDelta`
    /// events (which the [`OpenhumanEventBridge`](super::OpenhumanEventBridge)
    /// mirrors onto `AgentProgress` text deltas).
    ///
    /// A streaming-capable provider forwards incremental text to the
    /// per-call delta channel; a non-streaming provider simply returns the
    /// aggregated response, which still arrives as the terminal `Completed`
    /// item. Native tool calls always ride on `Completed`.
    async fn stream(&self, _state: &(), request: ModelRequest) -> tinyagents::Result<ModelStream> {
        let (messages, specs) = build_chat_inputs(&request);
        let provider = self.provider.clone();
        let model = self.model.clone();
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let thinking = self.thinking.clone();

        let (item_tx, item_rx) = tokio::sync::mpsc::unbounded_channel::<ModelStreamItem>();

        // Producer: run the provider call while forwarding its incremental
        // deltas, then emit the terminal item. Everything captured is owned, so
        // the task is `'static`.
        tokio::spawn(async move {
            let _ = item_tx.send(ModelStreamItem::Started);
            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<ProviderDelta>(64);
            let chat_fut = async {
                let req = ChatRequest {
                    messages: &messages,
                    tools: if specs.is_empty() { None } else { Some(&specs) },
                    stream: Some(&delta_tx),
                    max_tokens,
                };
                provider.chat(req, &model, temperature).await
            };
            tokio::pin!(chat_fut);

            let mut streamed_thinking = false;
            let response = loop {
                tokio::select! {
                    maybe = delta_rx.recv() => {
                        if let Some(delta) = maybe {
                            streamed_thinking |= matches!(delta, ProviderDelta::ThinkingDelta { .. });
                            forward_delta(&item_tx, thinking.as_ref(), delta);
                        }
                    }
                    res = &mut chat_fut => break res,
                }
            };
            // Drain any deltas that landed before the call returned.
            while let Ok(delta) = delta_rx.try_recv() {
                streamed_thinking |= matches!(delta, ProviderDelta::ThinkingDelta { .. });
                forward_delta(&item_tx, thinking.as_ref(), delta);
            }

            let terminal = match response {
                Ok(resp) => {
                    // Fallback for providers that return reasoning only on the
                    // aggregated response (no incremental thinking deltas): emit
                    // it once so child/parent thinking output isn't lost.
                    if !streamed_thinking {
                        if let Some(forwarder) = &thinking {
                            if let Some(reasoning) =
                                resp.reasoning_content.as_ref().filter(|r| !r.is_empty())
                            {
                                forwarder.emit(reasoning.clone());
                            }
                        }
                    }
                    ModelStreamItem::Completed(response_to_model_response(&resp))
                }
                Err(e) => ModelStreamItem::Failed(format!("openhuman provider chat failed: {e}")),
            };
            let _ = item_tx.send(terminal);
        });

        let stream = futures_util::stream::unfold(item_rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        });
        Ok(Box::pin(stream))
    }
}
