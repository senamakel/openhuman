//! Admission of one inbound message: module preparation, the
//! `ChannelMessageReceived` event, and the runtime-command and approval-reply
//! intercepts that answer a message without an agent turn.

use crate::channels::context::ChannelRuntimeContext;
use crate::channels::routes::handle_runtime_command_if_needed;
use crate::core::bus::BUS;
use crate::core::events::DomainEvent;
use crate::util::truncate_with_ellipsis;
use std::sync::Arc;
use tinychannels_bus as traits;

use super::approval::{channel_has_approval_surface, try_route_approval_reply};
use super::RuntimeChannelMessage;

/// A prepared inbound message that still needs an agent turn: not a runtime
/// command and not an approval reply.
#[derive(Debug, Clone)]
pub(crate) struct AdmittedInbound {
    pub(crate) msg: traits::ChannelMessage,
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
        inbound_envelope,
        authenticated_relay,
        prepared_inbound,
    } = runtime_msg;
    let prepared = match prepared_inbound {
        Some(prepared) => prepared,
        None => {
            let source = if message.channel.is_empty() {
                let envelope = inbound_envelope?;
                ChannelInboundSource::Envelope {
                    envelope,
                    timestamp: 0,
                }
            } else {
                ChannelInboundSource::Legacy {
                    message,
                    envelope: inbound_envelope,
                }
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
    let msg = prepared.message;
    let history_key = prepared.history_key;
    let mut envelope = prepared.envelope;
    envelope.access.delivered_via_upstream_relay = authenticated_relay;

    println!(
        "  💬 [{}] from {}: {}",
        msg.channel,
        msg.sender,
        truncate_with_ellipsis(&msg.content, 80)
    );

    BUS.publish(DomainEvent::ChannelMessageReceived {
        channel: msg.channel.clone(),
        message_id: msg.id.clone(),
        sender: msg.sender.clone(),
        reply_target: msg.reply_target.clone(),
        content: msg.content.clone(),
        thread_ts: msg.thread_ts.clone(),
        inbound_envelope: Some(envelope),
        workspace_dir: ctx.workspace_dir.as_ref().clone(),
    });

    let target_channel = ctx.channels_by_name.get(&msg.channel).cloned();
    if handle_runtime_command_if_needed(ctx.as_ref(), &msg, target_channel.as_ref(), &history_key)
        .await
    {
        return None;
    }

    // A yes/no reply to an approval parked on this conversation decides it;
    // a fresh turn would cancel the parked tool call.
    if channel_has_approval_surface(&msg.channel)
        && try_route_approval_reply(&msg, &history_key).await
    {
        return None;
    }

    Some(AdmittedInbound { msg, history_key })
}
