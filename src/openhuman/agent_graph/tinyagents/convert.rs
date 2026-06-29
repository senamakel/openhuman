//! Conversions between openhuman's flat [`ChatMessage`]/[`ToolSpec`]/[`ToolCall`]
//! wire types and the `tinyagents` harness' rich [`Message`]/[`ToolSchema`]/
//! [`TaToolCall`] equivalents (issue #4249).
//!
//! The two sides model the same concepts with different shapes:
//!
//! - openhuman `ChatMessage` is `{ role: String, content: String }` — tool
//!   calls and tool-result correlation ids are not first-class fields; the
//!   legacy loop threads them through provider-native encoding instead.
//! - `tinyagents::harness::message::Message` is a typed enum
//!   (`System`/`User`/`Assistant`/`Tool`) whose `Assistant` arm carries
//!   structured `tool_calls` and whose `Tool` arm carries a `tool_call_id`.
//!
//! These helpers bridge the seed history into the harness and the harness'
//! resulting transcript back out, so a turn can run on the `tinyagents`
//! agent-loop while callers keep speaking openhuman's `ChatMessage` vocabulary.

use tinyagents::harness::message::{
    AssistantMessage, ContentBlock, Message, SystemMessage, ToolMessage, UserMessage,
};
use tinyagents::harness::tool::{ToolCall as TaToolCall, ToolSchema};

use crate::openhuman::inference::provider::ChatMessage;
use crate::openhuman::tools::ToolSpec;

/// Convert one openhuman [`ChatMessage`] into a harness [`Message`].
///
/// Role strings map onto the typed arms; the `tool` role uses the message id
/// (when present) as the correlation id so a pre-threaded tool result keeps its
/// `tool_call_id`. Assistant tool-call structure is not recoverable from the
/// flat type, so an assistant message becomes plain text — the harness re-emits
/// any new tool calls itself from the model adapter.
pub(super) fn chat_message_to_message(msg: &ChatMessage) -> Message {
    let text = msg.content.clone();
    match msg.role.as_str() {
        "system" => Message::System(SystemMessage {
            content: vec![ContentBlock::Text(text)],
        }),
        "assistant" => Message::Assistant(AssistantMessage {
            id: msg.id.clone(),
            content: vec![ContentBlock::Text(text)],
            tool_calls: Vec::new(),
            usage: None,
        }),
        "tool" => Message::Tool(ToolMessage {
            tool_call_id: msg.id.clone().unwrap_or_default(),
            content: vec![ContentBlock::Text(text)],
        }),
        // "user" and any unrecognized role default to a user turn — the safest
        // mapping for a free-form inbound message.
        _ => Message::User(UserMessage {
            content: vec![ContentBlock::Text(text)],
        }),
    }
}

/// Convert a seed history into the harness `input` transcript.
pub(super) fn history_to_messages(history: &[ChatMessage]) -> Vec<Message> {
    history.iter().map(chat_message_to_message).collect()
}

/// Convert a harness [`Message`] back into an openhuman [`ChatMessage`].
///
/// Assistant tool calls are flattened to their text (the loop already executed
/// them and appended `Tool` result messages), and a tool message preserves its
/// correlation id on [`ChatMessage::id`] so downstream persistence keeps it.
pub(super) fn message_to_chat_message(msg: &Message) -> ChatMessage {
    match msg {
        Message::System(_) => ChatMessage::system(msg.text()),
        Message::User(_) => ChatMessage::user(msg.text()),
        Message::Assistant(_) => ChatMessage::assistant(msg.text()),
        Message::Tool(t) => {
            let mut cm = ChatMessage::tool(msg.text());
            cm.id = Some(t.tool_call_id.clone());
            cm
        }
    }
}

/// Convert a harness transcript back into openhuman history.
pub(super) fn messages_to_history(messages: &[Message]) -> Vec<ChatMessage> {
    messages.iter().map(message_to_chat_message).collect()
}

/// Convert an openhuman [`ToolSpec`] into a harness [`ToolSchema`].
pub(super) fn spec_to_schema(spec: &ToolSpec) -> ToolSchema {
    ToolSchema {
        name: spec.name.clone(),
        description: spec.description.clone(),
        parameters: spec.parameters.clone(),
    }
}

/// Convert a harness [`TaToolCall`] into an openhuman [`ToolCall`].
///
/// The harness models arguments as parsed JSON; openhuman carries them as the
/// raw JSON string the provider emitted, so we re-serialize.
pub(super) fn ta_call_to_oh_call(
    call: &TaToolCall,
) -> crate::openhuman::inference::provider::ToolCall {
    crate::openhuman::inference::provider::ToolCall {
        id: call.id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.to_string(),
        extra_content: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_round_trip_through_the_bridge() {
        let history = vec![
            ChatMessage::system("you are helpful"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("hi there"),
        ];
        let messages = history_to_messages(&history);
        assert!(matches!(messages[0], Message::System(_)));
        assert!(matches!(messages[1], Message::User(_)));
        assert!(matches!(messages[2], Message::Assistant(_)));

        let back = messages_to_history(&messages);
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].role, "system");
        assert_eq!(back[1].content, "hello");
        assert_eq!(back[2].role, "assistant");
    }

    #[test]
    fn tool_message_preserves_correlation_id() {
        let messages = vec![Message::Tool(ToolMessage {
            tool_call_id: "call-7".into(),
            content: vec![ContentBlock::Text("done".into())],
        })];
        let back = messages_to_history(&messages);
        assert_eq!(back[0].role, "tool");
        assert_eq!(back[0].content, "done");
        assert_eq!(back[0].id.as_deref(), Some("call-7"));
    }

    #[test]
    fn spec_and_tool_call_convert() {
        let spec = ToolSpec {
            name: "echo".into(),
            description: "echoes".into(),
            parameters: serde_json::json!({"type": "object"}),
        };
        let schema = spec_to_schema(&spec);
        assert_eq!(schema.name, "echo");
        assert_eq!(schema.parameters, serde_json::json!({"type": "object"}));

        let ta = TaToolCall {
            id: "c1".into(),
            name: "echo".into(),
            arguments: serde_json::json!({"msg": "hi"}),
        };
        let oh = ta_call_to_oh_call(&ta);
        assert_eq!(oh.id, "c1");
        assert_eq!(oh.name, "echo");
        assert_eq!(oh.arguments, r#"{"msg":"hi"}"#);
    }
}
