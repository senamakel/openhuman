//! Source-compatible facade for callers that still construct the former
//! OpenAI-compatible provider directly.
//!
//! The facade owns configuration only. Every call builds a tinyagents
//! `OpenAiModel` and delegates through [`CrateBackedProvider`]; no host wire
//! client remains.

use async_trait::async_trait;

pub use super::auth::AuthStyle;
use super::crate_openai::{build_crate_openai_model, CrateOpenAiConfig};
use super::crate_provider::CrateBackedProvider;
use super::traits::{ChatMessage, ChatRequest, ChatResponse, Provider, ProviderCapabilities};

pub struct OpenAiCompatibleProvider {
    name: String,
    base_url: String,
    credential: String,
    auth_style: AuthStyle,
    temperature_unsupported_models: Vec<String>,
    temperature_override: Option<f64>,
    merge_system_into_user: bool,
    extra_headers: Vec<(String, String)>,
    extra_query_params: Vec<(String, String)>,
    user_agent: Option<String>,
    responses_api_primary: bool,
    supports_responses_fallback: bool,
    native_tool_calling: Option<bool>,
    vision: Option<bool>,
    default_provider_options: Option<serde_json::Value>,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
    ) -> Self {
        Self::configured(name, base_url, credential, auth_style, false, None)
    }

    pub fn new_no_responses_fallback(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
    ) -> Self {
        let mut provider = Self::new(name, base_url, credential, auth_style);
        provider.supports_responses_fallback = false;
        provider
    }

    pub fn new_merge_system_into_user(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
    ) -> Self {
        Self::configured(name, base_url, credential, auth_style, true, None)
    }

    pub fn new_with_user_agent(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
        user_agent: &str,
    ) -> Self {
        Self::configured(
            name,
            base_url,
            credential,
            auth_style,
            false,
            Some(user_agent),
        )
    }

    fn configured(
        name: &str,
        base_url: &str,
        credential: Option<&str>,
        auth_style: AuthStyle,
        merge_system_into_user: bool,
        user_agent: Option<&str>,
    ) -> Self {
        Self {
            name: name.to_string(),
            base_url: base_url.to_string(),
            credential: credential.unwrap_or_default().to_string(),
            auth_style,
            temperature_unsupported_models: Vec::new(),
            temperature_override: None,
            merge_system_into_user,
            extra_headers: Vec::new(),
            extra_query_params: Vec::new(),
            user_agent: user_agent.map(str::to_string),
            responses_api_primary: false,
            supports_responses_fallback: true,
            native_tool_calling: None,
            vision: None,
            default_provider_options: None,
        }
    }

    pub fn with_temperature_unsupported_models(mut self, models: Vec<String>) -> Self {
        self.temperature_unsupported_models = models;
        self
    }

    pub fn with_temperature_override(mut self, temperature: Option<f64>) -> Self {
        self.temperature_override = temperature;
        self
    }

    pub fn with_native_tool_calling(mut self, enabled: bool) -> Self {
        self.native_tool_calling = Some(enabled);
        self
    }

    pub fn with_vision(mut self, enabled: bool) -> Self {
        self.vision = Some(enabled);
        self
    }

    pub fn with_ollama_num_ctx(mut self, num_ctx: Option<u32>) -> Self {
        self.default_provider_options =
            num_ctx.map(|num_ctx| serde_json::json!({ "options": { "num_ctx": num_ctx } }));
        self
    }

    pub fn with_extra_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.push((name.into(), value.into()));
        self
    }

    pub fn with_extra_query_param(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.extra_query_params.push((name.into(), value.into()));
        self
    }

    pub fn with_user_agent(mut self, value: impl Into<String>) -> Self {
        self.user_agent = Some(value.into());
        self
    }

    pub fn with_responses_api_primary(mut self) -> Self {
        self.responses_api_primary = true;
        self
    }

    pub fn with_openhuman_thread_id(self) -> Self {
        self
    }

    fn inner_with_responses(
        &self,
        model: &str,
        responses_api_primary: bool,
    ) -> CrateBackedProvider {
        let chat = build_crate_openai_model(CrateOpenAiConfig {
            provider_name: &self.name,
            endpoint: &self.base_url,
            api_key: &self.credential,
            auth_style: self.auth_style.clone(),
            model,
            temperature_unsupported_models: &self.temperature_unsupported_models,
            temperature_override: self.temperature_override,
            merge_system_into_user: self.merge_system_into_user,
            extra_headers: &self.extra_headers,
            native_tool_calling: self.native_tool_calling,
            vision: self.vision,
            default_provider_options: self.default_provider_options.clone(),
            responses_api_primary,
            responses_omit_max_output_tokens: responses_api_primary,
            extra_query_params: &self.extra_query_params,
            user_agent: self.user_agent.as_deref(),
        });
        CrateBackedProvider::new(chat, self.name.clone())
    }

    fn inner(&self, model: &str) -> CrateBackedProvider {
        self.inner_with_responses(model, self.responses_api_primary)
    }

    fn should_retry_responses(&self, error: &anyhow::Error) -> bool {
        self.supports_responses_fallback
            && !self.responses_api_primary
            && error.to_string().contains("404")
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    fn telemetry_provider_id(&self) -> String {
        self.name.clone()
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.inner("").capabilities()
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let first = self
            .inner(model)
            .chat_with_system(system_prompt, message, model, temperature)
            .await;
        match first {
            Err(error) if self.should_retry_responses(&error) => {
                tracing::debug!(provider = %self.name, "[inference][legacy-facade] retrying 404 through crate Responses API");
                self.inner_with_responses(model, true)
                    .chat_with_system(system_prompt, message, model, temperature)
                    .await
            }
            result => result,
        }
    }

    async fn chat_with_history(
        &self,
        messages: &[ChatMessage],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let first = self
            .inner(model)
            .chat_with_history(messages, model, temperature)
            .await;
        match first {
            Err(error) if self.should_retry_responses(&error) => {
                self.inner_with_responses(model, true)
                    .chat_with_history(messages, model, temperature)
                    .await
            }
            result => result,
        }
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        let first = self.inner(model).chat(request, model, temperature).await;
        match first {
            Err(error) if self.should_retry_responses(&error) => {
                self.inner_with_responses(model, true)
                    .chat(request, model, temperature)
                    .await
            }
            result => result,
        }
    }

    fn supports_streaming(&self) -> bool {
        false
    }
}
