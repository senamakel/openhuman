//! [`PromptCacheSegmentMiddleware`]: declare the turn's stable prompt prefix
//! (system prompt + tool schemas) as cache segments with content-fingerprint
//! ids, so the crate `PromptCacheGuardMiddleware` has a prefix to protect.

use async_trait::async_trait;
use sha2::{Digest, Sha256};

use tinyagents_harness::context::RunContext;
use tinyagents_harness::error::Result as TaResult;
use tinyagents_harness::middleware::Middleware;
use tinyinference::message::Message as TaMessage;
use tinyinference::model::{ModelRequest, PromptSegment, SegmentRole};

/// Stable SHA-256 fingerprint over canonical JSON. TinyAgents' prompt builder
/// uses the same shape for `ModelRequest::prompt_fingerprint`; OpenHuman builds
/// requests directly, so this adapter must stamp equivalent content-derived
/// segment ids and request fingerprints.
fn stable_prefix_fingerprint(value: &serde_json::Value) -> String {
    let mut hasher = Sha256::new();
    if serde_json::to_writer(&mut hasher, value).is_err() {
        hasher = Sha256::new();
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// `before_model`: declare the turn's stable prompt prefix (system prompt + tool
/// schemas) as [`PromptSegment`]s on the [`ModelRequest`] (issue #4249, 03.2).
///
/// OpenHuman assembles the request's messages/tools directly rather than through
/// the crate prompt builder, so `cache_segments` would otherwise stay empty and
/// the crate `PromptCacheGuardMiddleware` (installed immediately after this)
/// would have no prefix to protect. This stamps the segments with
/// **content-fingerprint ids**: an unchanged system prompt + full tool-schema set
/// yields a stable prefix, while an injected timestamp/uuid/etc. or changed tool
/// schema flips it and the guard records a
/// [`CacheLayoutEvent`](tinyagents_harness::cache::CacheLayoutEvent). This is
/// the structured, crate-native replacement for the deleted warn-only
/// `CacheAlignMiddleware` volatile-token scan (C3): the crate
/// `PromptCacheGuardMiddleware` now owns KV-cache-prefix drift detection via
/// recorded `CacheLayoutEvent`s. Read-only w.r.t. the transcript — only sets
/// `cache_segments` / `prompt_fingerprint`.
pub(crate) struct PromptCacheSegmentMiddleware;

#[async_trait]
impl Middleware<()> for PromptCacheSegmentMiddleware {
    fn name(&self) -> &str {
        "prompt_cache_segments"
    }

    async fn before_model(
        &self,
        _ctx: &mut RunContext<()>,
        _state: &(),
        request: &mut ModelRequest,
    ) -> TaResult<()> {
        let mut segments: Vec<PromptSegment> = Vec::new();
        // 1. System prompt — the cache-hottest stable prefix segment.
        if let Some(sys) = request
            .messages
            .iter()
            .find(|m| matches!(m, TaMessage::System(_)))
        {
            let fp = stable_prefix_fingerprint(&serde_json::json!({
                "role": "system",
                "messages": [sys],
            }));
            segments.push(PromptSegment {
                id: format!("system:{fp}"),
                role: SegmentRole::System,
                cacheable: true,
            });
        }
        // 2. Tool schemas — advertised tool surface identity (full schemas, in
        //    registration order) forms the next stable prefix segment. A changed
        //    tool surface legitimately busts the prefix; an unchanged one keeps
        //    it stable.
        if !request.tools.is_empty() {
            let fp = stable_prefix_fingerprint(&serde_json::json!({
                "role": "tools",
                "tools": &request.tools,
            }));
            segments.push(PromptSegment {
                id: format!("tools:{fp}"),
                role: SegmentRole::Tools,
                cacheable: true,
            });
        }
        if !segments.is_empty() {
            request.prompt_fingerprint = Some(stable_prefix_fingerprint(&serde_json::json!({
                "segments": &segments,
                "tools": &request.tools,
            })));
            tracing::debug!(
                segment_count = segments.len(),
                fingerprint = request.prompt_fingerprint.as_deref().unwrap_or(""),
                "[cache] declared stable prompt-prefix segments for KV-cache guard"
            );
            request.cache_segments = segments;
        }
        Ok(())
    }
}
