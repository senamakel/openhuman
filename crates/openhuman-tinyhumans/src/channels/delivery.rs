//! Progressive delivery (send, edit, delete, typing) on the hosted client's
//! channel routes.
//!
//! Transport refusals are typed facts mapped by [`map_backend_error`];
//! recovery and retry budgets belong to the channel module.

use openhuman_embed::__host::channel_callback::ChannelCallbackCtx;
use openhuman_embed::__host::security::credentials::session_support::{
    get_session_token, is_local_session_token, BackendCredential,
};
use reqwest::Method;
use serde_json::{json, Map, Value};
use tinychannels_bus::module_progressive::{
    ProgressiveTransportRequest, ProgressiveTransportResult,
};

use crate::client::{BackendClient, HostedApiError};

/// Maps a client error onto the module's transport facts.
pub(super) fn map_backend_error(error: anyhow::Error) -> ProgressiveTransportResult {
    match error.downcast_ref::<HostedApiError>() {
        Some(HostedApiError::ChannelEditUnsupported { .. }) => {
            ProgressiveTransportResult::EditUnsupported
        }
        Some(HostedApiError::MessageNotFound { .. }) => ProgressiveTransportResult::MessageGone,
        _ => ProgressiveTransportResult::Failed,
    }
}

fn request_channel(request: &ProgressiveTransportRequest) -> &str {
    match request {
        ProgressiveTransportRequest::OutputEvent { channel, .. }
        | ProgressiveTransportRequest::Send { channel, .. }
        | ProgressiveTransportRequest::SendTyped { channel, .. }
        | ProgressiveTransportRequest::Edit { channel, .. }
        | ProgressiveTransportRequest::Delete { channel, .. }
        | ProgressiveTransportRequest::Typing { channel, .. } => channel,
    }
}

fn transport_context(
    turn_id: Option<String>,
    channel_ref: Option<tinychannels_bus::ChannelRef>,
    conversation: Option<tinychannels_bus::ConversationRef>,
    sequence: Option<u64>,
    idempotency_key: Option<String>,
    receipt: Option<tinychannels_bus::channel::MessageReceipt>,
    segment_index: Option<u32>,
) -> Map<String, Value> {
    let mut context = Map::new();
    if let Some(value) = turn_id {
        context.insert("turnId".into(), json!(value));
    }
    if let Some(value) = channel_ref {
        context.insert("channelRef".into(), json!(value));
    }
    if let Some(value) = conversation {
        context.insert("conversation".into(), json!(value));
    }
    if let Some(value) = sequence {
        context.insert("sequence".into(), json!(value));
    }
    if let Some(value) = idempotency_key {
        context.insert("idempotencyKey".into(), json!(value));
    }
    if let Some(value) = receipt {
        context.insert("receipt".into(), json!(value));
    }
    if let Some(value) = segment_index {
        context.insert("segmentIndex".into(), json!(value));
    }
    context
}

fn attach_transport_context(mut body: Value, context: Map<String, Value>) -> Value {
    if context.is_empty() {
        return body;
    }
    if let Some(object) = body.as_object_mut() {
        object.insert("transportContext".into(), Value::Object(context));
        body
    } else {
        json!({"message": body, "transportContext": context})
    }
}

/// The hosted session credential, never the offline local token.
fn hosted_credential(
    config: &openhuman_embed::__host::config::Config,
) -> Option<BackendCredential> {
    match get_session_token(config) {
        Ok(Some(token)) if !is_local_session_token(&token) => {
            Some(BackendCredential::Session(token))
        }
        _ => None,
    }
}

/// Runs one progressive transport request for the session `ctx` describes.
pub(super) async fn execute(
    ctx: &ChannelCallbackCtx,
    request: ProgressiveTransportRequest,
) -> ProgressiveTransportResult {
    if matches!(&request, ProgressiveTransportRequest::OutputEvent { .. }) {
        log::debug!("[tinyhumans][channels] hosted callback does not project module output events");
        return ProgressiveTransportResult::Failed;
    }
    if ctx.session.is_cancelled() {
        log::debug!("[tinyhumans][channels] session closed before the transport call");
        return ProgressiveTransportResult::Unavailable;
    }
    if let Some(channel) = &ctx.channel {
        if request_channel(&request) != channel {
            log::debug!("[tinyhumans][channels] request names a different channel; refused");
            return ProgressiveTransportResult::Failed;
        }
    }
    let Some(credential) = hosted_credential(&ctx.config) else {
        log::debug!("[tinyhumans][channels] hosted channel credential unavailable");
        return ProgressiveTransportResult::Unavailable;
    };
    let client = match BackendClient::from_config(&ctx.config) {
        Ok(client) => client,
        Err(error) => {
            log::debug!("[tinyhumans][channels] backend client unavailable: {error:#}");
            return ProgressiveTransportResult::Failed;
        }
    };
    tokio::select! {
        biased;
        _ = ctx.session.cancelled() => ProgressiveTransportResult::Unavailable,
        result = send(&client, &credential, request) => result,
    }
}

async fn send(
    client: &BackendClient,
    credential: &BackendCredential,
    request: ProgressiveTransportRequest,
) -> ProgressiveTransportResult {
    let result = match request {
        ProgressiveTransportRequest::OutputEvent { .. } => {
            // This callback performs hosted REST operations; it is not the
            // observation/event subscriber described by Acknowledged.
            return ProgressiveTransportResult::Failed;
        }
        ProgressiveTransportRequest::Send {
            channel,
            turn_id,
            channel_ref,
            conversation,
            sequence,
            idempotency_key,
            message,
        } => {
            let context = transport_context(
                turn_id,
                channel_ref,
                conversation,
                sequence,
                idempotency_key,
                None,
                None,
            );
            client
                .send_channel_message(
                    &channel,
                    credential.clone(),
                    attach_transport_context(message, context),
                )
                .await
        }
        ProgressiveTransportRequest::SendTyped {
            channel,
            turn_id,
            channel_ref,
            conversation,
            sequence,
            segment_index,
            idempotency_key,
            parts,
            text,
        } => {
            let context = transport_context(
                turn_id,
                channel_ref,
                conversation,
                sequence,
                idempotency_key,
                None,
                segment_index,
            );
            client
                .send_channel_message(
                    &channel,
                    credential.clone(),
                    attach_transport_context(json!({"text": text, "parts": parts}), context),
                )
                .await
        }
        ProgressiveTransportRequest::Edit {
            channel,
            turn_id,
            channel_ref,
            conversation,
            message_id,
            receipt,
            text,
            ..
        } => {
            let context = transport_context(
                turn_id,
                channel_ref,
                conversation,
                None,
                None,
                receipt,
                None,
            );
            client
                .send_channel_edit(
                    &channel,
                    &message_id,
                    credential.clone(),
                    attach_transport_context(json!({"text": text}), context),
                )
                .await
        }
        ProgressiveTransportRequest::Delete {
            channel,
            turn_id,
            channel_ref,
            conversation,
            message_id,
            receipt,
        } => {
            let context = transport_context(
                turn_id,
                channel_ref,
                conversation,
                None,
                None,
                receipt,
                None,
            );
            if context.is_empty() {
                client
                    .send_channel_delete(&channel, &message_id, credential.clone())
                    .await
            } else {
                let channel = urlencoding::encode(channel.trim().trim_matches('/'));
                let message_id = urlencoding::encode(&message_id);
                client
                    .authed_json(
                        credential.clone(),
                        Method::DELETE,
                        &format!("channels/{channel}/messages/{message_id}"),
                        Some(json!({"transportContext": context})),
                    )
                    .await
            }
        }
        ProgressiveTransportRequest::Typing {
            channel,
            turn_id,
            channel_ref,
            conversation,
        } => {
            let context =
                transport_context(turn_id, channel_ref, conversation, None, None, None, None);
            if context.is_empty() {
                client
                    .send_channel_typing(&channel, credential.clone())
                    .await
            } else {
                let channel = urlencoding::encode(channel.trim().trim_matches('/'));
                client
                    .authed_json(
                        credential.clone(),
                        Method::POST,
                        &format!("channels/{channel}/typing"),
                        Some(json!({"transportContext": context})),
                    )
                    .await
            }
        }
    };
    match result {
        Ok(value) => ProgressiveTransportResult::Success(value),
        Err(error) => map_backend_error(error),
    }
}
