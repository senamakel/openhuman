use super::*;

#[async_trait]
impl ChatModel<()> for OpenHumanBackendModel {
    fn profile(&self) -> Option<&ModelProfile> {
        Some(&self.profile)
    }

    fn supports_input(
        &self,
        modality: tinyinference_llm::model::InputModality,
        mime: &str,
        source: tinyinference_llm::model::InputSource,
    ) -> bool {
        use tinyinference_llm::model::{InputModality, InputSource};
        matches!(
            (modality, source),
            (InputModality::Image, InputSource::Base64 | InputSource::Url)
        ) && matches!(
            mime,
            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
        ) || matches!(
            (modality, source),
            (InputModality::Audio, InputSource::Base64)
        ) && matches!(
            mime,
            "audio/wav" | "audio/x-wav" | "audio/mpeg" | "audio/mp3"
        )
    }

    /// Identity for harness response-cache scoping: the backend base URL and
    /// the default tier/model. The session JWT is deliberately absent — it
    /// rotates, and a key derived from it would never hit twice — and the
    /// backend resolves the tier per account anyway, so two accounts sharing
    /// a cache would need their own namespace, not a credential in the key.
    fn cache_identity(&self) -> Option<String> {
        self.base_url()
            .ok()
            .map(|base| format!("openhuman:{base}:{}", self.default_model))
    }

    async fn invoke(
        &self,
        state: &(),
        request: ModelRequest,
    ) -> tinyinference_llm::Result<ModelResponse> {
        let model = self.build_wire_model()?;
        let response = match model
            .invoke(
                state,
                with_thread_id(apply_reasoning_hint(request), self.thread_id.as_deref()),
            )
            .await
        {
            Ok(response) => response,
            Err(e) => {
                log_managed_dispatch_error(&e, "invoke");
                maybe_publish_session_expired(&e, "invoke");
                return Err(e);
            }
        };
        Ok(project_managed_usage(response))
    }

    async fn stream(
        &self,
        state: &(),
        request: ModelRequest,
    ) -> tinyinference_llm::Result<ModelStream> {
        let model = self.build_wire_model()?;
        // The backend sends its charge on an `openhuman-metadata` frame before
        // `[DONE]`; the crate's SSE parser keeps that envelope on the terminal
        // response's `raw`, so the streamed `Completed` is projected exactly like
        // the `invoke` path above and carries the charged USD.
        match model
            .stream(
                state,
                with_thread_id(apply_reasoning_hint(request), self.thread_id.as_deref()),
            )
            .await
        {
            // A failure can also arrive *inside* an HTTP 200 stream as an SSE
            // `{"error":…}` payload; it never reaches the `Err` arm (#6724).
            Ok(stream) => Ok(stream.map_items(|item| {
                observe_in_band_failure(&item);
                match item {
                    tinyinference_llm::model::ModelStreamItem::Completed(response) => {
                        tinyinference_llm::model::ModelStreamItem::Completed(project_managed_usage(
                            response,
                        ))
                    }
                    other => other,
                }
            })),
            Err(e) => {
                log_managed_dispatch_error(&e, "stream");
                maybe_publish_session_expired(&e, "stream");
                Err(e)
            }
        }
    }
}
