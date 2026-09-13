//! Multi-turn chat through the local model.

use crate::config::Config;
use crate::inference::local as local_ai;
use crate::rpc::RpcOutcome;

use super::turn_guards::enforce_user_prompt_or_reject;

/// A single message in a local AI chat conversation.
#[derive(Debug, serde::Deserialize)]
pub struct LocalAiChatMessage {
    /// The role of the message sender (e.g., "user", "assistant").
    pub role: String,
    /// The text content of the message.
    pub content: String,
}

/// Executes a multi-turn chat conversation using the local model.
pub async fn local_ai_chat(
    config: &Config,
    messages: Vec<LocalAiChatMessage>,
    max_tokens: Option<u32>,
) -> Result<RpcOutcome<String>, String> {
    tracing::debug!(
        message_count = messages.len(),
        "[local_ai:chat] local_ai_chat op: validating"
    );

    if messages.is_empty() {
        return Err("messages must not be empty".to_string());
    }

    let mut ollama_messages: Vec<crate::inference::local::ollama::OllamaChatMessage> =
        Vec::with_capacity(messages.len());

    for msg in messages.into_iter() {
        let normalized_role = msg.role.trim().to_ascii_lowercase();
        match normalized_role.as_str() {
            "user" => {
                enforce_user_prompt_or_reject(msg.content.as_str(), "local_ai.ops.local_ai_chat")?;
            }
            "system" | "assistant" => {}
            _ => {
                return Err(format!(
                    "unsupported message role: '{}'; expected one of: user, system, assistant",
                    msg.role.trim()
                ));
            }
        }

        ollama_messages.push(crate::inference::local::ollama::OllamaChatMessage {
            role: normalized_role,
            content: msg.content,
        });
    }

    let service = local_ai::global(config);
    let reply = service
        .chat_with_history_interactive(config, ollama_messages, max_tokens)
        .await?;

    tracing::debug!(
        reply_len = reply.len(),
        "[local_ai:chat] local_ai_chat op: done"
    );
    Ok(RpcOutcome::single_log(reply, "local ai chat completed"))
}
