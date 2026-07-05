//! OpenHuman-side implementations of the portable `tinychannels::host`
//! capability traits.
//!
//! Each adapter wraps existing OpenHuman internals (voice factory, inference
//! ops, approval gate, conversation store, shutdown registry, web event bus)
//! and exposes them through the portable, `Config`-free trait surface a ported
//! channel provider consumes. See [`super::build_channel_host`].

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use tinychannels::host::{
    ApprovalDecision, ApprovalGate, ConversationMessage, ConversationStore, EventSink,
    LifecycleRegistry, ReactionDecision, ReactionGate, ReactionQuery, ShutdownHook, SpeechRequest,
    SpeechResult, SpeechSynthesizer, Transcriber, TranscriptionRequest, TranscriptionResult,
};

use crate::openhuman::config::Config;

const LOG_PREFIX: &str = "[channels_host]";

// ---------------------------------------------------------------------------
// LifecycleRegistry → core::shutdown
// ---------------------------------------------------------------------------

/// Bridges [`LifecycleRegistry`] onto the process-global async shutdown hook
/// registry. Providers register teardown that runs once on shutdown.
pub struct CoreShutdownRegistry;

impl LifecycleRegistry for CoreShutdownRegistry {
    fn register_shutdown(&self, name: &str, hook: ShutdownHook) {
        let name = name.to_string();
        // `core::shutdown::register` takes a re-callable `Fn`, but our hook is a
        // one-shot `FnOnce`; guard it behind a take-once slot so a (theoretical)
        // second invocation is a no-op.
        let slot = Arc::new(Mutex::new(Some(hook)));
        crate::core::shutdown::register(move || {
            let slot = Arc::clone(&slot);
            let name = name.clone();
            async move {
                let taken = slot.lock().take();
                if let Some(hook) = taken {
                    tracing::debug!("{LOG_PREFIX} running shutdown hook: {name}");
                    hook().await;
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Transcriber → voice STT factory
// ---------------------------------------------------------------------------

/// Speech-to-text backed by the OpenHuman voice provider factory.
pub struct VoiceTranscriber {
    pub config: Arc<Config>,
}

#[async_trait]
impl Transcriber for VoiceTranscriber {
    fn name(&self) -> &str {
        "openhuman-voice"
    }

    async fn transcribe(
        &self,
        request: TranscriptionRequest,
    ) -> anyhow::Result<TranscriptionResult> {
        let provider = crate::openhuman::voice::effective_stt_provider(&self.config);
        tracing::debug!(
            "{LOG_PREFIX} transcribe provider={provider} bytes_b64={}",
            request.audio_base64.len()
        );
        // Empty model → factory substitutes DEFAULT_WHISPER_MODEL.
        let stt = crate::openhuman::voice::create_stt_provider(&provider, "", &self.config)?;
        let outcome = stt
            .transcribe(
                &self.config,
                &request.audio_base64,
                request.mime_type.as_deref(),
                request.file_name.as_deref(),
                request.language.as_deref(),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(TranscriptionResult {
            text: outcome.value.text,
            language: request.language,
            duration_secs: None,
        })
    }
}

// ---------------------------------------------------------------------------
// SpeechSynthesizer → voice reply_speech
// ---------------------------------------------------------------------------

/// Text-to-speech backed by the hosted reply-speech synthesizer.
pub struct VoiceSynthesizer {
    pub config: Arc<Config>,
}

#[async_trait]
impl SpeechSynthesizer for VoiceSynthesizer {
    async fn synthesize(&self, request: SpeechRequest) -> anyhow::Result<SpeechResult> {
        tracing::debug!(
            "{LOG_PREFIX} synthesize chars={} voice={:?}",
            request.text.len(),
            request.voice
        );
        let opts = crate::openhuman::voice::reply_speech::ReplySpeechOptions {
            voice_id: request.voice,
            model_id: None,
            output_format: request.format,
            voice_settings: None,
        };
        let outcome = crate::openhuman::voice::reply_speech::synthesize_reply(
            &self.config,
            &request.text,
            &opts,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        let value = outcome.value;
        let visemes = serde_json::to_value(&value.visemes).ok();
        Ok(SpeechResult {
            audio_base64: value.audio_base64,
            mime_type: value.audio_mime,
            visemes,
        })
    }
}

// ---------------------------------------------------------------------------
// ReactionGate → inference should_react
// ---------------------------------------------------------------------------

/// Inference-driven reaction gate backed by the local-AI should-react op.
pub struct InferenceReactionGate {
    pub config: Arc<Config>,
}

#[async_trait]
impl ReactionGate for InferenceReactionGate {
    async fn should_react(&self, query: ReactionQuery) -> anyhow::Result<ReactionDecision> {
        // Honour the runtime gate: when the local model runtime is disabled we
        // never react (matches presentation's prior inline guard).
        if !self.config.local_ai.runtime_enabled {
            tracing::debug!("{LOG_PREFIX} should_react skipped (local runtime disabled)");
            return Ok(ReactionDecision::default());
        }
        let outcome = crate::openhuman::inference::ops::inference_should_react(
            &self.config,
            &query.message,
            &query.channel_type,
        )
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(ReactionDecision {
            should_react: outcome.value.should_react,
            emoji: outcome.value.emoji,
            reason: None,
        })
    }
}

// ---------------------------------------------------------------------------
// ApprovalGate → approval reply parsing
// ---------------------------------------------------------------------------

/// Parses inbound approval replies via the OpenHuman approval gate. Raising
/// interactive approvals stays host-internal (the tool gate), so only
/// [`ApprovalGate::parse_reply`] is implemented.
pub struct CoreApprovalGate;

impl ApprovalGate for CoreApprovalGate {
    fn parse_reply(&self, message: &str) -> Option<ApprovalDecision> {
        crate::openhuman::approval::parse_approval_reply(message).map(|decision| {
            use crate::openhuman::approval::ApprovalDecision as Core;
            match decision {
                Core::ApproveOnce => ApprovalDecision::Approve,
                Core::ApproveAlwaysForTool => {
                    ApprovalDecision::Choice("approve_always_for_tool".to_string())
                }
                Core::Deny => ApprovalDecision::Deny,
            }
        })
    }
}

// ---------------------------------------------------------------------------
// ConversationStore → memory_conversations
// ---------------------------------------------------------------------------

/// Durable conversation history backed by the OpenHuman conversation store.
pub struct ConversationHistoryStore {
    pub workspace_dir: std::path::PathBuf,
}

#[async_trait]
impl ConversationStore for ConversationHistoryStore {
    async fn history(
        &self,
        session_key: &str,
        limit: usize,
    ) -> anyhow::Result<Vec<ConversationMessage>> {
        let messages = crate::openhuman::memory_conversations::get_messages(
            self.workspace_dir.clone(),
            session_key,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        let start = messages.len().saturating_sub(limit);
        Ok(messages[start..]
            .iter()
            .map(|m| ConversationMessage {
                role: m.message_type.clone(),
                content: m.content.clone(),
                timestamp: None,
            })
            .collect())
    }

    async fn append(&self, session_key: &str, message: ConversationMessage) -> anyhow::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        // `append_message` requires the thread to exist; create-or-noop first.
        crate::openhuman::memory_conversations::ensure_thread(
            self.workspace_dir.clone(),
            crate::openhuman::memory_conversations::CreateConversationThread {
                id: session_key.to_string(),
                title: session_key.to_string(),
                created_at: now.clone(),
                parent_thread_id: None,
                labels: None,
                personality_id: None,
            },
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        let stored = crate::openhuman::memory_conversations::ConversationMessage {
            id: uuid::Uuid::new_v4().to_string(),
            content: message.content,
            message_type: message.role.clone(),
            extra_metadata: serde_json::Value::Null,
            sender: message.role,
            created_at: now,
        };
        crate::openhuman::memory_conversations::append_message(
            self.workspace_dir.clone(),
            session_key,
            stored,
        )
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EventSink → web channel event bus
// ---------------------------------------------------------------------------

/// Publishes provider events onto the web channel's `WebChannelEvent`
/// broadcast bus. The `payload` must deserialize into a `WebChannelEvent`
/// (the presentation provider builds that shape as JSON).
pub struct WebChannelEventSink;

#[async_trait]
impl EventSink for WebChannelEventSink {
    async fn publish(
        &self,
        domain: &str,
        kind: &str,
        payload: serde_json::Value,
    ) -> anyhow::Result<()> {
        let event: crate::core::socketio::WebChannelEvent = serde_json::from_value(payload)
            .map_err(|e| {
                anyhow::anyhow!(
                    "{LOG_PREFIX} event payload not a WebChannelEvent ({domain}/{kind}): {e}"
                )
            })?;
        crate::openhuman::channels::providers::web::publish_web_channel_event(event);
        Ok(())
    }
}
