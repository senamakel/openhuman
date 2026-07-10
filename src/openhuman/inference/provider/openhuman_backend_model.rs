//! Crate-native managed OpenHuman backend as a host [`ChatModel`] (issue #4727,
//! Motion B).
//!
//! The managed backend can't be a plain crate `OpenAiModel` preset: it uses a
//! **dynamic** session JWT (fetched per call), emits the `thread_id` extension so
//! the backend groups InferenceLog entries + aligns KV-cache keys, and relies on
//! the `openhuman.usage/billing` response envelope for charged-USD / cached-token
//! accounting. This host `ChatModel` bridges all three onto the crate wire client:
//!
//! * **Dynamic JWT** — [`invoke`](ChatModel::invoke)/[`stream`](ChatModel::stream)
//!   resolve the current bearer via [`OpenHumanBackendProvider::resolve_bearer`]
//!   and build a fresh crate `OpenAiModel` (Bearer) per call.
//! * **`thread_id`** — injected into `ModelRequest.provider_options` so the crate
//!   flattens it into the request body as the top-level `thread_id` field (parity
//!   with the host `with_openhuman_thread_id`).
//! * **Billing envelope** — the crate `parse_response` preserves the full response
//!   JSON on `ModelResponse.raw`, so the seam's `usage_info_from_response` still
//!   recovers `openhuman.usage.charged_amount_usd` / cached tokens downstream.
//!
//! This is the bespoke-provider rewrite that gates deleting `compatible*.rs` (the
//! managed backend was its last non-BYOK consumer).

use async_trait::async_trait;
use serde_json::Value;

use tinyagents::harness::model::{
    ChatModel, ModelProfile, ModelRequest, ModelResponse, ModelStream,
};
use tinyagents::harness::providers::openai::OpenAiModel;
use tinyagents::{Result as TaResult, TinyAgentsError};

use super::openhuman_backend::{OpenHumanBackendProvider, PROVIDER_LABEL};
use super::thread_context;

/// The managed OpenHuman backend as a crate [`ChatModel`]. Holds the backend
/// provider (for JWT + base-URL resolution) and the default model id sent when a
/// request doesn't override it.
pub struct OpenHumanBackendModel {
    backend: OpenHumanBackendProvider,
    default_model: String,
}

impl OpenHumanBackendModel {
    /// Wrap a resolved [`OpenHumanBackendProvider`] with the default model id.
    pub fn new(backend: OpenHumanBackendProvider, default_model: impl Into<String>) -> Self {
        Self {
            backend,
            default_model: default_model.into(),
        }
    }

    /// Resolve the current JWT + base URL and build a fresh crate `OpenAiModel`
    /// (Bearer). Rebuilt per call because the session JWT rotates.
    fn build_wire_model(&self) -> TaResult<OpenAiModel> {
        let token = self
            .backend
            .resolve_bearer()
            .map_err(|e| TinyAgentsError::Model(e.to_string()))?;
        let base_url = self
            .backend
            .base_url()
            .map_err(|e| TinyAgentsError::Model(e.to_string()))?;
        // The hosted API is chat-completions only (no `/v1/responses`); auth is a
        // plain bearer JWT. The tier/model rides `request.model`, which the backend
        // resolves — the baked default only applies when a request omits it.
        Ok(OpenAiModel::compatible_provider(
            PROVIDER_LABEL,
            token,
            base_url,
            &self.default_model,
        ))
    }
}

/// Inject the ambient `thread_id` (when set) into the request's
/// `provider_options` so the crate emits it as a top-level `thread_id` body field
/// — parity with the host `with_openhuman_thread_id` extension.
fn with_thread_id(mut request: ModelRequest) -> ModelRequest {
    let Some(thread_id) = thread_context::current_thread_id() else {
        return request;
    };
    let mut options = request.provider_options.clone();
    if !options.is_object() {
        options = Value::Object(serde_json::Map::new());
    }
    if let Some(map) = options.as_object_mut() {
        map.insert("thread_id".to_string(), Value::String(thread_id));
    }
    request = request.with_provider_options(options);
    request
}

#[async_trait]
impl ChatModel<()> for OpenHumanBackendModel {
    fn profile(&self) -> Option<&ModelProfile> {
        // The managed backend serves every workload tier (the tier rides
        // `request.model`), so it advertises no single static capability profile;
        // vision gating is enforced by the seam's RequiredCapabilitiesMiddleware.
        None
    }

    async fn invoke(&self, state: &(), request: ModelRequest) -> TaResult<ModelResponse> {
        let model = self.build_wire_model()?;
        model.invoke(state, with_thread_id(request)).await
    }

    async fn stream(&self, state: &(), request: ModelRequest) -> TaResult<ModelStream> {
        let model = self.build_wire_model()?;
        model.stream(state, with_thread_id(request)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::inference::provider::ProviderRuntimeOptions;
    use tinyagents::harness::message::Message;

    fn backend() -> OpenHumanBackendModel {
        let provider = OpenHumanBackendProvider::new(
            Some("https://api.example.test"),
            &ProviderRuntimeOptions::default(),
        );
        OpenHumanBackendModel::new(provider, "reasoning-v1")
    }

    #[tokio::test]
    async fn with_thread_id_injects_when_ambient_thread_present() {
        thread_context::with_thread_id("thread-42", async {
            let request = ModelRequest::new(vec![Message::user("hi")]);
            let updated = with_thread_id(request);
            assert_eq!(
                updated.provider_options["thread_id"],
                serde_json::json!("thread-42")
            );
        })
        .await;
    }

    #[test]
    fn with_thread_id_is_noop_without_ambient_thread() {
        // No thread scope active → provider_options stays whatever it was (null).
        let request = ModelRequest::new(vec![Message::user("hi")]);
        let updated = with_thread_id(request);
        assert!(updated.provider_options.get("thread_id").is_none());
    }

    #[test]
    fn managed_model_has_no_static_profile() {
        assert!(backend().profile().is_none());
    }
}
