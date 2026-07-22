//! Deterministic offline `Provider` mocks installed via
//! `test_provider_override` (honoured only under the `rss-bench` feature).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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

/// Latency-configurable text-only mock used by the `fleet` scenario. Before
/// returning its fixed answer it sleeps a sampled latency: a mean from
/// `OPENHUMAN_PROFILE_MOCK_LATENCY_MS` (default `0` = no sleep) with jitter
/// `± OPENHUMAN_PROFILE_MOCK_JITTER_MS` (default `mean / 4`). Per-call jitter is
/// derived from a seeded xorshift counter — deterministic and dependency-free
/// (no `rand` crate).
pub struct LatencyMock {
    text: String,
    mean_ms: u64,
    jitter_ms: u64,
    counter: AtomicU64,
    pub prompts: Mutex<Vec<String>>,
}

/// Read `key` as a `u64`, falling back to `default` when unset/unparsable.
fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

impl LatencyMock {
    /// Build from the standard env knobs.
    pub fn from_env(text: impl Into<String>) -> Arc<Self> {
        let mean_ms = env_u64("OPENHUMAN_PROFILE_MOCK_LATENCY_MS", 0);
        let jitter_ms = env_u64("OPENHUMAN_PROFILE_MOCK_JITTER_MS", mean_ms / 4);
        eprintln!("[library-profile] LatencyMock mean_ms={mean_ms} jitter_ms={jitter_ms}");
        Arc::new(Self {
            text: text.into(),
            mean_ms,
            jitter_ms,
            counter: AtomicU64::new(0),
            prompts: Mutex::new(Vec::new()),
        })
    }

    /// Sample `mean ± jitter` (clamped at zero) via a seeded xorshift step. The
    /// seed advances per call so successive turns get distinct latencies.
    pub fn sample_latency_ms(&self) -> u64 {
        if self.mean_ms == 0 && self.jitter_ms == 0 {
            return 0;
        }
        let seed = self.counter.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
        // xorshift64 — deterministic, dependency-free pseudo-randomness.
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let span = self.jitter_ms.saturating_mul(2).saturating_add(1);
        let delta = (x % span) as i64 - self.jitter_ms as i64;
        (self.mean_ms as i64 + delta).max(0) as u64
    }

    async fn sleep_sampled(&self) {
        let ms = self.sample_latency_ms();
        if ms > 0 {
            tokio::time::sleep(Duration::from_millis(ms)).await;
        }
    }
}

#[async_trait]
impl Provider for LatencyMock {
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
        self.sleep_sampled().await;
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
        self.sleep_sampled().await;
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
