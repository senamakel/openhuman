//! Admission of one inbound message: module preparation, the
//! `ChannelMessageReceived` event, and the runtime-command and approval-reply
//! intercepts that answer a message without an agent turn.

use crate::channels::context::ChannelRuntimeContext;
use crate::channels::routes::handle_runtime_command_if_needed;
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::util::truncate_with_ellipsis;
use std::sync::Arc;
use tinychannels_bus::ChannelInboundEnvelope;

use super::approval::consumes_approval_reply;
use super::RuntimeChannelMessage;

/// A prepared inbound message that still needs an agent turn: not a runtime
/// command and not an approval reply.
#[derive(Debug, Clone)]
pub(crate) struct AdmittedInbound {
    pub(crate) msg: ChannelInboundEnvelope,
    pub(crate) history_key: String,
}

/// Prepare an inbound message, publish it, and answer it directly when it is
/// a runtime command or an approval reply. Returns the message when it still
/// needs an agent turn.
pub(crate) async fn admit_runtime_message(
    ctx: Arc<ChannelRuntimeContext>,
    runtime_msg: RuntimeChannelMessage,
) -> Option<AdmittedInbound> {
    use tinychannels_bus::module_inbound::{ChannelInboundSource, PrepareChannelInboundRequest};
    let RuntimeChannelMessage {
        message,
        authenticated_relay,
        prepared_inbound,
    } = runtime_msg;
    let prepared = match prepared_inbound {
        Some(prepared) => prepared,
        None => {
            let source = ChannelInboundSource::Envelope {
                envelope: message,
                timestamp: 0,
            };
            match crate::modules::channel_controller::prepare_inbound(
                &crate::channels::context::module_client(&ctx).await,
                PrepareChannelInboundRequest {
                    source,
                    namespace: "main".into(),
                    session_policy: Default::default(),
                },
            )
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    tracing::warn!("[dispatch] channel preparation failed: {error}");
                    return None;
                }
            }
        }
    };
    let msg = prepared.envelope;
    let history_key = prepared.history_key;
    let mut msg = msg;
    msg.access.delivered_via_upstream_relay = authenticated_relay;

    println!(
        "  💬 [{}] from {}: {}",
        msg.channel.id,
        msg.sender.id,
        truncate_with_ellipsis(&msg.text, 80)
    );

    BUS.publish(DomainEvent::ChannelMessageReceived {
        channel: msg.channel.id.clone(),
        message_id: msg.message_id.clone(),
        sender: msg.sender.id.clone(),
        reply_target: msg.conversation.id.clone(),
        content: msg.text.clone(),
        thread_ts: msg.conversation.thread_id.clone(),
        inbound_envelope: Some(msg.clone()),
        workspace_dir: ctx.workspace_dir.as_ref().clone(),
    });

    let target_channel = ctx.channels_by_name.get(&msg.channel.id).cloned();
    if handle_runtime_command_if_needed(ctx.as_ref(), &msg, target_channel.as_ref(), &history_key)
        .await
    {
        return None;
    }

    // A yes/no reply to an approval parked on this conversation decides it;
    // a fresh turn would cancel the parked tool call.
    let gate = crate::security::approval::ApprovalGate::try_global();
    if consumes_approval_reply(target_channel.as_ref(), gate.as_deref(), &msg, &history_key) {
        return None;
    }

    Some(AdmittedInbound { msg, history_key })
}
