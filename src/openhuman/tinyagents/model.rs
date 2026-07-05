//! `tinyagents` [`ChatModel`] adapter over an openhuman [`Provider`] (issue #4249).
//!
//! Wraps `Arc<dyn Provider>` so the `tinyagents` agent-loop can drive a real
//! openhuman inference backend. On each model call the harness hands us a
//! provider-neutral [`ModelRequest`] (rich messages + advertised tool schemas);
//! we translate it into an openhuman [`ChatRequest`], call `provider.chat`, and
//! translate the [`ChatResponse`] back into a harness [`ModelResponse`] —
//! carrying through text, native tool calls, and token usage.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tinyagents::harness::message::{AssistantMessage, ContentBlock, MessageDelta};
use tinyagents::harness::model::{
    ChatModel, Modalities, ModelProfile, ModelRequest, ModelResponse, ModelStream, ModelStreamItem,
};
use tinyagents::harness::tool::{ToolCall as TaToolCall, ToolDelta};
use tinyagents::harness::usage::Usage;
use tokio::sync::mpsc::{Sender, UnboundedSender};

use super::abort_guard::AbortOnDrop;
use super::observability::{IterationCursor, ProviderUsageCarry, SubagentScope, ToolNameMap};
use crate::openhuman::agent::progress::AgentProgress;
use crate::openhuman::inference::provider::thread_context::{current_thread_id, with_thread_id};
use crate::openhuman::inference::provider::{
    current_route_slot, with_route_slot, ChatMessage, ChatRequest, ChatResponse, Provider,
    ProviderDelta,
};
use crate::openhuman::tools::ToolSpec;

/// Out-of-band forwarder for progress events that do not yet round-trip through
/// tinyagents with OpenHuman parity: non-streaming post-hoc reasoning and the
/// tool-call **start** marker (tool name).
///
/// Streaming reasoning now rides tinyagents' native `MessageDelta.reasoning`
/// channel, and the incremental tool-call **argument** fragments ride the native
/// `MessageDelta.tool_call` channel (crate `ToolDelta`); both are projected by
/// [`OpenhumanEventBridge`](super::OpenhumanEventBridge). What remains here is
/// the split the crate can't express: the crate `ToolDelta` has only
/// `call_id`/`content` (no `tool_name`), so the tool-call **start** event — the
/// empty-delta `ToolCallArgsDelta` that carries the tool name and opens the UI
/// timeline row — is still emitted straight onto the progress sink, and the
/// learned `call_id → tool_name` map is *shared* with the bridge (via
/// [`ToolNameMap`]) so it can label the argument fragments it now projects off
/// the crate stream. This forwarder also still emits non-streaming post-hoc
/// reasoning (see [`ProviderModel::invoke`]). It shares the bridge's
/// [`IterationCursor`] so each event is attributed to the right model call.
/// Parent runs emit the top-level variants; child runs emit the `Subagent`
/// counterpart for thinking. Tool-arg/start events have no child variant, so
/// they ride the top-level event.
#[derive(Clone)]
pub(super) struct ThinkingForwarder {
    sink: Sender<AgentProgress>,
    scope: Option<SubagentScope>,
    cursor: IterationCursor,
    /// call_id → tool_name, learned from `ToolCallStart`. Shared with the
    /// [`OpenhumanEventBridge`](super::OpenhumanEventBridge) so the streamed
    /// argument fragments (which ride the crate `ToolDelta`, sans name) can be
    /// labelled with the tool the UI shows.
    tool_names: ToolNameMap,
}

impl ThinkingForwarder {
    pub(super) fn new(
        sink: Sender<AgentProgress>,
        scope: Option<SubagentScope>,
        cursor: IterationCursor,
        tool_names: ToolNameMap,
    ) -> Self {
        Self {
            sink,
            scope,
            cursor,
            tool_names,
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

    /// Record the tool name a streaming tool call starts with (into the map
    /// shared with the bridge, so it can label the argument fragments it
    /// projects off the crate stream), and emit the start marker — an
    /// empty-delta `ToolCallArgsDelta` — so consumers see the call begin before
    /// its arguments arrive (matching the legacy `ProviderDelta::ToolCallStart`
    /// mapping). The crate `ToolDelta` has no `tool_name` field, so this half of
    /// the tool-arg contract can't ride the crate stream and stays here.
    fn note_tool_call(&self, call_id: String, tool_name: String) {
        self.tool_names
            .lock()
            .unwrap()
            .insert(call_id.clone(), tool_name.clone());
        tracing::trace!(
            call_id = call_id.as_str(),
            tool_name = tool_name.as_str(),
            child = self.scope.is_some(),
            "[stream] tool-call start marker (name recorded for crate-stream arg fragments)"
        );
        let _ = self.sink.try_send(AgentProgress::ToolCallArgsDelta {
            call_id,
            tool_name,
            delta: String::new(),
            iteration: self.cursor.load(Ordering::SeqCst),
        });
    }
}

/// Translate a harness [`ModelRequest`] into openhuman's message list + tool
/// specs (shared by the buffered and streaming paths).
fn build_chat_inputs(
    request: &ModelRequest,
    native_tools: bool,
) -> (Vec<ChatMessage>, Vec<ToolSpec>) {
    // Native-tool providers need assistant tool calls + tool results encoded in
    // the provider's native envelope so a tool round round-trips; prompt-guided
    // providers need tool results folded into a `[Tool results]` user turn.
    let messages = if native_tools {
        request
            .messages
            .iter()
            .map(super::convert::message_to_native_chat_message)
            .collect()
    } else {
        super::convert::messages_to_text_mode_chat(&request.messages)
    };
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

/// Build a [`PFormatRegistry`](crate::openhuman::agent::pformat::PFormatRegistry)
/// from the tool schemas advertised on a [`ModelRequest`] (issue #4465).
///
/// The text-mode fallback parse needs each tool's positional parameter layout
/// to reconstruct named JSON arguments from a P-Format `name[a|b]` body. The
/// harness always populates `request.tools` (schemas are rendered into the
/// prompt for prompt-guided providers, or advertised natively otherwise), so
/// the registry is available in both modes. An empty registry (no tools
/// advertised) makes the P-Format-aware parser short-circuit to the canonical
/// grammar, so this is behaviour-neutral when there are no tools.
fn pformat_registry_from_request(
    request: &ModelRequest,
) -> crate::openhuman::agent::pformat::PFormatRegistry {
    request
        .tools
        .iter()
        .map(|t| {
            (
                t.name.clone(),
                crate::openhuman::agent::pformat::PFormatToolParams::from_schema(&t.parameters),
            )
        })
        .collect()
}

/// Translate an openhuman [`ChatResponse`] into a harness [`ModelResponse`]
/// (visible text + tool calls + token usage).
///
/// Native `tool_calls` take precedence; when absent, the response text is parsed
/// for prompt-guided (`<tool_call>…` / p-format) calls — matching the legacy
/// dispatcher — so text-mode models drive the tinyagents loop too. The visible
/// text is the prose with any tool-call markup stripped.
///
/// `pformat_registry` carries the advertised tools' positional layouts so the
/// text-mode fallback can recover P-Format (`name[a|b]`) calls that ~10 builtin
/// prompts still teach — the migrated parse path had dropped that grammar and
/// silently lost those calls (issue #4465). It is empty for the native-tool
/// path (where `response.tool_calls` is used directly) and for tool-less turns.
///
/// Unknown-tool recovery is handled by `RunPolicy::unknown_tool`, so the model
/// adapter preserves the provider-requested tool name.
fn response_to_model_response(
    response: &ChatResponse,
    pformat_registry: &crate::openhuman::agent::pformat::PFormatRegistry,
) -> ModelResponse {
    let (visible_text, tool_calls): (String, Vec<TaToolCall>) = if !response.tool_calls.is_empty() {
        let calls = response
            .tool_calls
            .iter()
            .map(|tc| TaToolCall {
                id: tc.id.clone(),
                name: tc.name.clone(),
                arguments: serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null),
            })
            .collect();
        (response.text.clone().unwrap_or_default(), calls)
    } else if let Some(text) = response.text.as_deref() {
        let (prose, parsed) =
            crate::openhuman::agent::harness::parse_tool_calls_with_pformat(text, pformat_registry);
        if parsed.is_empty() {
            (text.to_string(), Vec::new())
        } else {
            let calls = parsed
                .into_iter()
                .enumerate()
                .map(|(i, p)| TaToolCall {
                    // Prompt-guided calls carry no provider id; synthesize a
                    // stable one so tool results correlate in the harness.
                    id: p.id.unwrap_or_else(|| format!("call_{i}")),
                    name: p.name,
                    arguments: p.arguments,
                })
                .collect();
            (prose, calls)
        }
    } else {
        (String::new(), Vec::new())
    };

    let mut content = Vec::new();
    if !visible_text.is_empty() {
        content.push(ContentBlock::Text(visible_text));
    }
    // Thinking models return `reasoning_content` separately from the visible
    // reply. Preserve it as a typed thinking block so it stays out of
    // `Message::text()` but survives persistence and the next turn's request,
    // where thinking-mode providers require it back.
    if let Some(block) =
        super::convert::reasoning_content_block(response.reasoning_content.as_deref())
    {
        content.push(block);
    }
    let usage = response.usage.as_ref().map(|u| {
        // Carry the provider's cached-prefix input count through the crate
        // `Usage` (it has a `cache_read_tokens` field) so downstream cost
        // accounting can price it at the cached rate. `Usage::new` seeds
        // input/output/total; set the cache field on top. (`charged_amount_usd`
        // has no crate home; the event bridge estimates cost from token counts.)
        let mut usage = Usage::new(u.input_tokens, u.output_tokens);
        usage.cache_read_tokens = u.cached_input_tokens;
        usage
    });
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

/// Forward one openhuman [`ProviderDelta`]. Visible text, reasoning, and
/// tool-call **argument** fragments all become harness [`ModelStreamItem`]s (so
/// the [`OpenhumanEventBridge`](super::OpenhumanEventBridge) mirrors them as
/// progress deltas from the crate stream alone): text/reasoning as
/// [`MessageDelta`], and each argument fragment as
/// [`ModelStreamItem::ToolCallDelta`] correlated by `call_id`. The crate
/// `ToolDelta` has no `tool_name`, so the tool-call **start** marker (which
/// carries the name and opens the UI timeline row) still rides the out-of-band
/// [`ThinkingForwarder`]; it also records the name into the map shared with the
/// bridge so the streamed fragments stay labelled. The model adapter still
/// assembles the final native tool calls from the `Completed` response (the
/// `StreamAccumulator` treats it as authoritative), so these fragments are
/// progress-only — the UI can show the call being composed.
fn forward_delta(
    tx: &UnboundedSender<ModelStreamItem>,
    thinking: Option<&ThinkingForwarder>,
    delta: ProviderDelta,
) {
    match delta {
        ProviderDelta::TextDelta { delta } => {
            if !delta.is_empty() {
                let _ = tx.send(ModelStreamItem::MessageDelta(MessageDelta::text(delta)));
            }
        }
        ProviderDelta::ThinkingDelta { delta } => {
            if !delta.is_empty() {
                let _ = tx.send(ModelStreamItem::MessageDelta(MessageDelta::reasoning(
                    delta,
                )));
            }
        }
        ProviderDelta::ToolCallStart { call_id, tool_name } => {
            if let Some(forwarder) = thinking {
                forwarder.note_tool_call(call_id, tool_name);
            }
        }
        ProviderDelta::ToolCallArgsDelta { call_id, delta } => {
            if !delta.is_empty() {
                tracing::trace!(
                    call_id = call_id.as_str(),
                    len = delta.len(),
                    "[stream] forwarding tool-arg fragment onto crate ToolCallDelta"
                );
                let _ = tx.send(ModelStreamItem::ToolCallDelta(ToolDelta {
                    call_id,
                    content: delta,
                }));
            }
        }
    }
}

/// A harness chat model backed by an openhuman [`Provider`].
///
/// The application `State` is `()` — openhuman tools and providers carry no
/// harness-visible shared state — so this adapter implements
/// `ChatModel<()>`.
/// Shared slot that preserves the most recent original provider error.
///
/// tinyagents carries errors as `TinyAgentsError::Model(String)`, which would
/// stringify openhuman's typed `anyhow::Error` (e.g. `AgentError::PermissionDenied`
/// / `MaxIterationsExceeded`) and break the downcast the caller relies on for
/// Sentry suppression and `AgentError`-tagged events. The adapter stashes the
/// original error here before returning the stringified one to the harness, so
/// the runner can re-surface the downcastable error after the run fails.
pub(super) type ProviderErrorSlot = Arc<Mutex<Option<anyhow::Error>>>;

pub(super) struct ProviderModel {
    provider: Arc<dyn Provider>,
    model: String,
    temperature: f64,
    max_tokens: Option<u32>,
    /// When set, the adapter forwards tool-argument progress and post-hoc
    /// non-streaming reasoning onto the progress sink.
    thinking: Option<ThinkingForwarder>,
    /// Preserves the last original provider error for the runner to re-surface.
    error_slot: ProviderErrorSlot,
    /// FIFO side-channel shared with the event bridge: on each successful chat
    /// response the adapter pushes the provider `UsageInfo` (which carries the
    /// backend-charged USD + context window + cache-creation/reasoning tokens
    /// the crate `Usage` mapping drops), and the bridge pops it when recording
    /// that call's usage — restoring charged-USD precedence (#4467, item 1).
    usage_carry: ProviderUsageCarry,
    /// Capability profile derived from the wrapped provider (issue #4249,
    /// Phase 2): lets the crate validate a request against the model's actual
    /// capabilities (vision, tool calling, streaming, token limits) *before*
    /// a network call, and drives capability-aware registry resolution.
    profile: ModelProfile,
}

/// Builds a `ChatModel<()>` capability over an openhuman [`Provider`], pinned to
/// `model`/`temperature`, ready to register into a `tinyagents`
/// [`CapabilityRegistry`](tinyagents::registry::CapabilityRegistry) — e.g. the
/// `.ragsh` REPL bridge (`crate::openhuman::rlm`). [`ProviderModel::new`] is
/// `pub(super)`; this is the crate-visible factory for that one bridge use.
pub(crate) fn provider_chat_model(
    provider: Arc<dyn Provider>,
    model: impl Into<String>,
    temperature: f64,
) -> Arc<dyn ChatModel<()>> {
    Arc::new(ProviderModel::new(provider, model, temperature))
}

impl ProviderModel {
    /// Build a model adapter for `provider`, pinned to `model`/`temperature`.
    ///
    /// The adapter's [`ModelProfile`] is derived from the provider's declared
    /// capabilities at construction: vision → `modalities.image_in`, native
    /// tool calling → `tool_calling`/`parallel_tool_calls` (openhuman's
    /// `ChatResponse` carries multiple tool calls per response), and
    /// `supports_streaming` → `streaming`. `streaming_tool_chunks` stays
    /// `false` — [`ProviderModel::stream`] forwards text deltas only and
    /// reconstructs tool calls from the final response. Token limits are
    /// threaded in by the runner via [`ProviderModel::with_context_window`] /
    /// [`ProviderModel::with_max_tokens`].
    pub(super) fn new(
        provider: Arc<dyn Provider>,
        model: impl Into<String>,
        temperature: f64,
    ) -> Self {
        let model = model.into();
        // Read the canonical accessor methods (not `capabilities()` directly):
        // several providers override `supports_native_tools`/`supports_vision`
        // without overriding the `capabilities()` struct.
        let native_tools = provider.supports_native_tools();
        let profile = ModelProfile {
            provider: Some(
                if provider.is_local_provider_for_model(&model) {
                    "local"
                } else {
                    "remote"
                }
                .to_string(),
            ),
            model: Some(model.clone()),
            modalities: Modalities {
                image_in: provider.supports_vision(),
                ..Modalities::default()
            },
            tool_calling: native_tools,
            parallel_tool_calls: native_tools,
            streaming: provider.supports_streaming(),
            ..ModelProfile::default()
        };
        Self {
            provider,
            model,
            temperature,
            max_tokens: None,
            thinking: None,
            error_slot: Arc::new(Mutex::new(None)),
            usage_carry: Arc::default(),
            profile,
        }
    }

    /// A handle to the shared error slot (clone before moving `self` into the
    /// harness, so the runner can recover the typed provider error on failure).
    pub(super) fn error_slot(&self) -> ProviderErrorSlot {
        self.error_slot.clone()
    }

    /// Attach the shared provider-usage carry the event bridge drains, so the
    /// backend-charged USD + context window this adapter observes reach the cost
    /// accounting (#4467, item 1). Clone the same handle into the bridge.
    pub(super) fn with_usage_carry(mut self, carry: ProviderUsageCarry) -> Self {
        self.usage_carry = carry;
        self
    }

    /// Cap the output tokens requested from the provider for every call.
    pub(super) fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self.profile.max_output_tokens = Some(u64::from(max_tokens));
        self
    }

    /// Record the model's effective context window on the profile so the crate
    /// can validate/select on input capacity before dispatch. Metadata only —
    /// history trimming stays with the context middlewares.
    pub(super) fn with_context_window(mut self, window: u64) -> Self {
        self.profile.max_input_tokens = Some(window);
        self
    }

    /// Override the profile's image-input (vision) capability.
    ///
    /// [`ProviderModel::new`] seeds `modalities.image_in` from the provider's
    /// *provider-wide* `supports_vision()`, but a workload-route projection
    /// (issue #4249, Workstream 02.1 — see [`super::routes`]) knows the
    /// per-route vision capability (e.g. the dedicated `vision-v1` tier is
    /// multimodal while `chat-v1` is text-only). This lets the route adapter
    /// record the accurate per-route modality so capability gating can reject a
    /// non-vision route for an image turn before dispatch.
    pub(super) fn with_vision(mut self, image_in: bool) -> Self {
        self.profile.modalities.image_in = image_in;
        self
    }

    /// Override the profile's reasoning/thinking capability. Set by the
    /// workload-route projection ([`super::routes`]) for reasoning-tier routes so
    /// a request that requires reasoning resolves to a reasoning-capable model.
    pub(super) fn with_reasoning(mut self, reasoning: bool) -> Self {
        self.profile.reasoning = reasoning;
        self
    }

    /// Forward provider thinking/tool-argument progress onto a progress sink via
    /// `forwarder` (parent or sub-agent scoped). See [`ThinkingForwarder`].
    pub(super) fn with_thinking(mut self, forwarder: ThinkingForwarder) -> Self {
        self.thinking = Some(forwarder);
        self
    }
}

#[async_trait]
impl ChatModel<()> for ProviderModel {
    fn profile(&self) -> Option<&ModelProfile> {
        Some(&self.profile)
    }

    async fn invoke(
        &self,
        _state: &(),
        request: ModelRequest,
    ) -> tinyagents::Result<ModelResponse> {
        let native = self.provider.supports_native_tools();
        let (messages, specs) = build_chat_inputs(&request, native);
        // Positional layouts for the text-mode P-Format fallback (issue #4465);
        // empty (and thus behaviour-neutral) when no tools are advertised.
        let pformat_registry = pformat_registry_from_request(&request);
        let chat_request = ChatRequest {
            messages: &messages,
            // Only advertise structured tool specs to native providers. Prompt-
            // guided providers (Ollama/LM Studio profiles) get the tool catalogue
            // folded into the transcript instead; sending a `tools`/`tool_choice`
            // payload would defeat the opt-out and get rejected/ignored.
            tools: (native && !specs.is_empty()).then_some(&specs),
            stream: None,
            max_tokens: self.max_tokens,
        };

        tracing::debug!(
            model = %self.model,
            messages = messages.len(),
            tools = specs.len(),
            "[tinyagents] provider.chat via harness model adapter"
        );

        let response = match self
            .provider
            .chat(chat_request, &self.model, self.temperature)
            .await
        {
            Ok(response) => {
                // #4457 (defect B): the error slot preserves the last provider
                // error for the runner to re-surface as the typed turn failure.
                // A call that *succeeds* — including one the provider fallback
                // chain recovered after an inner error — must clear any stale
                // error so a later, unrelated run failure (e.g. the model-call
                // cap) is not misclassified as that recovered provider error.
                if self.error_slot.lock().unwrap().take().is_some() {
                    tracing::debug!(
                        model = %self.model,
                        "[models] provider chat succeeded; cleared stale error_slot — #4457 defect B"
                    );
                }
                response
            }
            Err(e) => {
                // Classify with OpenHuman's product error taxonomy (issue #4249,
                // Workstream 02.2): a permanent config/auth rejection, billing/quota
                // exhaustion, or context-window overflow is mapped to a *non-retryable*
                // `TinyAgentsError::Validation` (crate `is_retryable` → false), while a
                // transient 5xx/429/network blip stays a retryable `Model` error. This
                // is the same `reliable::is_non_retryable` classifier `ReliableProvider`
                // uses, keeping OpenHuman as the single `ProviderError` mapper. With the
                // retry pin at a single attempt the mapping is behavior-neutral today; it
                // stages honest retry semantics for when the crate loop owns retries.
                let non_retryable =
                    crate::openhuman::inference::provider::reliable::is_non_retryable(&e);
                tracing::debug!(
                    model = %self.model,
                    non_retryable,
                    "[models] provider chat failed; classifying error for tinyagents retry/fallback"
                );
                // Preserve the original (downcastable) error for the runner, then
                // hand the harness a stringified copy to stop the loop.
                let msg = format!("openhuman provider chat failed: {e}");
                *self.error_slot.lock().unwrap() = Some(e);
                return Err(if non_retryable {
                    tinyagents::TinyAgentsError::Validation(msg)
                } else {
                    tinyagents::TinyAgentsError::Model(msg)
                });
            }
        };
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
        // Push this call's provider usage onto the shared carry so the event
        // bridge records charged USD / context window with provider precedence
        // (#4467, item 1). One push per successful response, matching the single
        // `UsageRecorded` the crate emits for this call.
        if let Some(u) = &response.usage {
            self.usage_carry
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push_back(u.clone());
        }
        Ok(response_to_model_response(&response, &pformat_registry))
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
        let native = self.provider.supports_native_tools();
        let (messages, specs) = build_chat_inputs(&request, native);
        // Positional layouts for the text-mode P-Format fallback (issue #4465);
        // built here so it can move into the `'static` producer task below.
        let pformat_registry = pformat_registry_from_request(&request);
        let provider = self.provider.clone();
        let model = self.model.clone();
        let temperature = self.temperature;
        let max_tokens = self.max_tokens;
        let thinking = self.thinking.clone();
        let error_slot = self.error_slot.clone();
        // Captured for the spawned producer (task-locals/`self` do not cross the
        // spawn): the streaming path pushes provider usage onto the same carry
        // the buffered path uses, so charged USD reaches the bridge (#4467, item 1).
        let usage_carry = self.usage_carry.clone();

        let (item_tx, item_rx) = tokio::sync::mpsc::unbounded_channel::<ModelStreamItem>();

        // #4460: the producer below runs in a detached `tokio::spawn`, and
        // `tokio::task_local`s do NOT propagate across a spawn boundary. Capture
        // the two ambient task-locals the provider call depends on *here*, on the
        // caller's task, and re-establish them inside the spawn:
        //   - `thread_id`  → the managed backend's `thread_id` extension
        //     (`compatible_request::outbound_thread_id`) so streamed requests stay
        //     attributed to the right chat / prompt-cache group.
        //   - resolved-route audit slot → so `record_resolved_provider_route`
        //     calls inside `provider.chat` write back to the caller's scope and the
        //     channel audit reports the *resolved* route, not the requested one.
        let thread_id = current_thread_id();
        let route_slot = current_route_slot();
        // Label for the abort-on-drop debug log; the moved-in `model` clone is
        // consumed by the producer body.
        let abort_label = model.clone();
        tracing::debug!(
            model = %model,
            thread_id = thread_id.as_deref().unwrap_or("<none>"),
            route_slot = route_slot.is_some(),
            "[tinyagents] spawning streamed provider producer; re-establishing task-locals across spawn — #4460"
        );

        // Producer: run the provider call while forwarding its incremental
        // deltas, then emit the terminal item. Everything captured is owned, so
        // the task is `'static`.
        let producer = async move {
            let _ = item_tx.send(ModelStreamItem::Started);
            let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<ProviderDelta>(64);
            let chat_fut = async {
                let req = ChatRequest {
                    messages: &messages,
                    // Prompt-guided providers get the tool catalogue in the
                    // transcript, not a structured `tools` payload (see the
                    // buffered path). `native` is captured by the async move.
                    tools: (native && !specs.is_empty()).then_some(&specs),
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
                    // #4457 (defect B): a successful streaming call — including
                    // one recovered by the provider fallback chain — clears any
                    // stale error preserved in the slot so a later unrelated run
                    // failure is not misclassified as that recovered error.
                    if error_slot.lock().unwrap().take().is_some() {
                        tracing::debug!(
                            model = %model,
                            "[models] streaming provider chat succeeded; cleared stale error_slot — #4457 defect B"
                        );
                    }
                    // Fallback for streaming providers that return reasoning only
                    // on the aggregated response (no incremental thinking
                    // deltas): emit it once through the native crate stream so
                    // the bridge handles scope consistently with live reasoning.
                    if !streamed_thinking {
                        if let Some(reasoning) =
                            resp.reasoning_content.as_ref().filter(|r| !r.is_empty())
                        {
                            let _ = item_tx.send(ModelStreamItem::MessageDelta(
                                MessageDelta::reasoning(reasoning.clone()),
                            ));
                        }
                    }
                    // Push provider usage onto the shared carry (#4467, item 1),
                    // mirroring the buffered path — before building the terminal
                    // item, so it is queued ahead of the crate `UsageRecorded`.
                    if let Some(u) = &resp.usage {
                        usage_carry
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .push_back(u.clone());
                    }
                    ModelStreamItem::Completed(response_to_model_response(&resp, &pformat_registry))
                }
                Err(e) => {
                    // Streaming failures ride `ModelStreamItem::Failed(String)`, which
                    // carries no retryable flag (the harness treats it as a retryable
                    // `Model` error), so the non-retryable mapping applied on the
                    // buffered path cannot be expressed here — a crate limitation. With
                    // the retry pin at a single attempt this has no effect today; logged
                    // under `[models]` for parity/auditability (issue #4249, 02.2).
                    let non_retryable =
                        crate::openhuman::inference::provider::reliable::is_non_retryable(&e);
                    tracing::debug!(
                        model = %model,
                        non_retryable,
                        "[models] streaming provider chat failed; harness will treat as retryable Model error"
                    );
                    // Preserve the original (downcastable) error for the runner.
                    let msg = format!("openhuman provider chat failed: {e}");
                    *error_slot.lock().unwrap() = Some(e);
                    ModelStreamItem::Failed(msg)
                }
            };
            let _ = item_tx.send(terminal);
        };

        // Re-establish the captured task-locals inside the spawned task (#4460).
        // `with_thread_id` normalizes an absent id to `None`, so it is a no-op
        // when there was no ambient thread; the route slot is only re-scoped when
        // an enclosing `with_resolved_provider_route_scope` supplied one.
        let handle = tokio::spawn(async move {
            let scoped = with_thread_id(thread_id.unwrap_or_default(), producer);
            match route_slot {
                Some(slot) => with_route_slot(slot, scoped).await,
                None => scoped.await,
            }
        });

        // #4460: tie the producer's lifetime to the consumer. Moving the
        // abort-on-drop guard into the stream state means that dropping the
        // stream (the turn future being hard-cancelled via `AbortHandle`, or
        // dropped for any other reason) aborts the in-flight `provider.chat` call
        // instead of letting it run — and bill — to completion in the background.
        let guard = AbortOnDrop::new(handle, abort_label);
        let stream = futures_util::stream::unfold((item_rx, guard), |(mut rx, guard)| async move {
            rx.recv().await.map(|item| (item, (rx, guard)))
        });
        Ok(Box::pin(stream))
    }
}
