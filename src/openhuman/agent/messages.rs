//! Durable OpenHuman transcript records.
//!
//! Runtime model calls use [`tinyagents::harness::message::Message`]. These
//! compact records preserve the stable JSONL/thread storage contract used by
//! existing installations and carry OpenHuman-only message metadata.

use serde::{Deserialize, Serialize};

use crate::openhuman::inference::provider::ToolCall;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    #[serde(default, skip_serializing)]
    pub id: Option<String>,
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing)]
    pub extra_metadata: Option<serde_json::Value>,
    /// Ascending byte offsets into [`Self::content`] at which the provider may
    /// place a prompt-cache breakpoint. Only meaningful on the system message.
    ///
    /// `skip_serializing` like `id` and `extra_metadata` above: these are a
    /// property of *this call*, derived from the freshly assembled prompt, and
    /// writing them into the JSONL transcript would persist offsets that stop
    /// matching the moment the prompt is rebuilt. `serde(default)` keeps every
    /// record already on disk loadable.
    #[serde(default, skip_serializing)]
    pub cache_breakpoints: Vec<usize>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            id: None,
            role: "system".into(),
            content: content.into(),
            extra_metadata: None,
            cache_breakpoints: Vec::new(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            id: None,
            role: "user".into(),
            content: content.into(),
            extra_metadata: None,
            cache_breakpoints: Vec::new(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            id: None,
            role: "assistant".into(),
            content: content.into(),
            extra_metadata: None,
            cache_breakpoints: Vec::new(),
        }
    }

    pub fn tool(content: impl Into<String>) -> Self {
        Self {
            id: None,
            role: "tool".into(),
            content: content.into(),
            extra_metadata: None,
            cache_breakpoints: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultMessage {
    pub tool_call_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ConversationMessage {
    Chat(ChatMessage),
    AssistantToolCalls {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extra_metadata: Option<serde_json::Value>,
    },
    ToolResults(Vec<ToolResultMessage>),
}
