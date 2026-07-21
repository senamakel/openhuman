//! Deterministic offline `Provider` mocks installed via
//! `test_provider_override` (honoured only under the `rss-bench` feature).

use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use openhuman_core::openhuman::inference::provider::traits::{
    ChatRequest, ChatResponse, ProviderCapabilities, ToolCall,
};
use openhuman_core::openhuman::inference::provider::Provider;

pub const SUBAGENT_A: &str = "LIB_PROFILE_SUBAGENT_A";
pub const SUBAGENT_B: &str = "LIB_PROFILE_SUBAGENT_B";

/// A plain `ChatResponse` carrying only text (no tool calls).
pub fn response(text: &str) -> ChatResponse {
    ChatResponse {
        text: Some(text.into()),
        tool_calls: Vec::new(),
        usage: None,
        reasoning_content: None,
    }
}

/// Records every prompt it sees so scenarios can assert what ran.
fn record(prompts: &Mutex<Vec<String>>, joined: &str) {
    prompts
        .lock()
        .expect("mock prompt lock")
        .push(joined.into());
}

/// Orchestration mock: the first (orchestrator) turn emits a
/// `spawn_parallel_agents` tool call fanning out to two researchers; the
/// researcher turns and the final merge return plain text.
pub struct SubagentMock {
    pub prompts: Mutex<Vec<String>>,
}

impl SubagentMock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            prompts: Mutex::new(Vec::new()),
        })
    }

    fn reply(&self, joined: &str) -> ChatResponse {
        record(&self.prompts, joined);
        if joined.contains("Finding A") && joined.contains("Finding B") {
            return response("Merged both researcher findings.");
        }
        if joined.contains(SUBAGENT_B) {
            return response("Finding B: rollback controls are complete.");
        }
        if joined.contains(SUBAGENT_A) {
            return response("Finding A: migration capacity is healthy.");
        }
        ChatResponse {
            text: Some("Delegating to two researchers.".into()),
            tool_calls: vec![ToolCall {
                id: "profile-parallel-call".into(),
                name: "spawn_parallel_agents".into(),
                arguments: serde_json::json!({
                    "tasks": [
                        {
                            "agent_id": "researcher",
                            "prompt": format!("{SUBAGENT_A}: inspect migration capacity"),
                            "ownership": "scope: capacity"
                        },
                        {
                            "agent_id": "researcher",
                            "prompt": format!("{SUBAGENT_B}: inspect rollback controls"),
                            "ownership": "scope: rollback"
                        }
                    ]
                })
                .to_string(),
                extra_content: None,
            }],
            usage: None,
            reasoning_content: None,
        }
    }
}

#[async_trait]
impl Provider for SubagentMock {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: false,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        _temperature: f64,
    ) -> Result<String> {
        Ok(self
            .reply(&format!("{}\n{message}", system_prompt.unwrap_or("")))
            .text
            .unwrap_or_default())
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        _model: &str,
        _temperature: f64,
    ) -> Result<ChatResponse> {
        let joined = request
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(self.reply(&joined))
    }
}

/// Text-only mock: always returns a fixed direct answer, never a tool call.
/// Used by the single-turn / workflow scenarios that must NOT delegate.
pub struct PlainTextMock {
    text: String,
    pub prompts: Mutex<Vec<String>>,
}

impl PlainTextMock {
    pub fn new(text: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            text: text.into(),
            prompts: Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl Provider for PlainTextMock {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            native_tool_calling: true,
            vision: false,
        }
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        _model: &str,
        _temperature: f64,
    ) -> Result<String> {
        record(
            &self.prompts,
            &format!("{}\n{message}", system_prompt.unwrap_or("")),
        );
        Ok(self.text.clone())
    }

    async fn chat(
        &self,
        request: ChatRequest<'_>,
        _model: &str,
        _temperature: f64,
    ) -> Result<ChatResponse> {
        let joined = request
            .messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        record(&self.prompts, &joined);
        Ok(response(&self.text))
    }
}
