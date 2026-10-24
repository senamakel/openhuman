//! Per-sender routing and runtime command handling.

use super::context::{clear_sender_history, ChannelRouteSelection, ChannelRuntimeContext};
use super::{outbound::text_message, Channel};
use std::sync::Arc;
use tinychannels_bus as traits;
use tinychannels_bus::module_control::{
    ChannelControlRequest, ChannelControlResult, ProviderDescriptor,
};

fn provider_descriptors(config: &crate::config::Config) -> Vec<ProviderDescriptor> {
    config
        .cloud_providers
        .iter()
        .map(|provider| ProviderDescriptor {
            name: provider.slug.clone(),
            aliases: if crate::config::schema::cloud_providers::is_hosted_cloud_identity(
                &provider.slug,
            ) || crate::config::schema::cloud_providers::is_hosted_auth_style(
                provider.auth_style,
            ) {
                vec!["backend".to_string(), "openhuman-backend".to_string()]
            } else {
                Vec::new()
            },
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn register_hosted_route_test_policy() {
    use crate::config::schema::cloud_providers::AuthStyle;
    crate::config::schema::cloud_providers::register_hosted_cloud_provider_policy(
        Arc::new(|slug| slug == "openhuman"),
        Arc::new(|style| style == AuthStyle::OpenhumanJwt),
    );
}

fn default_route_selection(ctx: &ChannelRuntimeContext) -> ChannelRouteSelection {
    ChannelRouteSelection {
        provider: ctx.default_provider.as_str().to_string(),
        model: ctx.model.as_str().to_string(),
    }
}

pub(crate) fn get_route_selection(
    ctx: &ChannelRuntimeContext,
    sender_key: &str,
) -> ChannelRouteSelection {
    ctx.route_overrides
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(sender_key)
        .cloned()
        .unwrap_or_else(|| default_route_selection(ctx))
}

fn set_route_selection(ctx: &ChannelRuntimeContext, sender_key: &str, next: ChannelRouteSelection) {
    let default_route = default_route_selection(ctx);
    let mut routes = ctx
        .route_overrides
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if next == default_route {
        routes.remove(sender_key);
    } else {
        routes.insert(sender_key.to_string(), next);
    }
}

pub(crate) async fn get_or_create_turn_model_source(
    ctx: &ChannelRuntimeContext,
    provider_name: &str,
) -> anyhow::Result<crate::agent::tinyagents::TurnModelSource> {
    if provider_name == ctx.default_provider.as_str() {
        return ctx.turn_model_source.as_ref().cloned().ok_or_else(|| {
            anyhow::anyhow!("no injected channel model source for '{provider_name}'")
        });
    }

    if let Some(existing) = ctx
        .turn_model_source_cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(provider_name)
        .cloned()
    {
        return Ok(existing);
    }

    anyhow::bail!(
        "no injected channel model source for '{provider_name}'; production routes use crate-native model sources"
    )
}

struct RouteCallbacks {
    context: ChannelRuntimeContext,
    core_context: Option<Arc<crate::core::runtime::CoreContext>>,
    tenant: crate::core::runtime::Tenant,
    cancellation: tokio_util::sync::CancellationToken,
}

/// Project the v22 envelope into the legacy shape still required by the
/// module's `ChannelControlRequest::Route` contract.
#[allow(
    deprecated,
    reason = "TinyChannels v22 Route requests still declare ChannelMessage"
)]
pub(crate) fn route_compat_message(msg: &traits::ChannelInboundEnvelope) -> traits::ChannelMessage {
    traits::ChannelMessage {
        id: msg.message_id.clone(),
        sender: msg.sender.id.clone(),
        reply_target: msg.conversation.id.clone(),
        content: msg.text.clone(),
        channel: msg.channel.id.clone(),
        timestamp: msg.timestamp_unix_ms.unwrap_or_default(),
        thread_ts: msg.conversation.thread_id.clone(),
        reply_to_id: msg.reply_to_id.clone(),
        topic_id: msg.conversation.topic_id.clone(),
        sender_name: msg.sender.name.clone(),
    }
}

impl RouteCallbacks {
    fn new(
        context: ChannelRuntimeContext,
        cancellation: tokio_util::sync::CancellationToken,
    ) -> Self {
        let ctx = context.dispatch_ctx();
        Self {
            core_context: ctx.resolved(),
            tenant: crate::core::runtime::tenant::isolated_unless_tenant(
                ctx.tenant(),
                "channel_remote_control",
            ),
            context,
            cancellation,
        }
    }
}

#[tinybus::interface(name = "ai.tinyhumans.tinychannels.ChannelsHost")]
impl RouteCallbacks {
    async fn dispatch_controller_backend(
        &self,
        request: tinychannels_bus::ChannelBackendRequest,
    ) -> tinybus::Result<tinychannels_bus::ChannelBackendResult> {
        match request {
            tinychannels_bus::ChannelBackendRequest::RemoteControl(request) => {
                // The captured context is the carrier; without one the host's
                // default context (none in SaaS, which fails closed).
                let dispatch_ctx = match &self.core_context {
                    Some(core) => crate::core::runtime::DispatchCtx::new(Arc::clone(core)),
                    None => crate::core::runtime::DispatchCtx::host_default(),
                };
                let dispatch = async {
                    tokio::select! {
                        biased;
                        _ = self.cancellation.cancelled() => {
                            Err(tinybus::Error::failed("channel session ended"))
                        }
                        result = super::host::remote_control::dispatch_callback(
                            &dispatch_ctx,
                            &self.context,
                            &self.tenant,
                            &self.cancellation,
                            request,
                        ) => result.map(tinychannels_bus::ChannelBackendResult::RemoteControl),
                    }
                };
                match &self.core_context {
                    Some(core_context) => {
                        crate::core::runtime::CoreContext::scope(Arc::clone(core_context), dispatch)
                            .await
                    }
                    None => dispatch.await,
                }
            }
            _ => Err(tinybus::Error::failed("channel callback unavailable")),
        }
    }
}

pub(crate) async fn handle_runtime_command_if_needed(
    ctx: &ChannelRuntimeContext,
    msg: &traits::ChannelInboundEnvelope,
    target_channel: Option<&Arc<dyn Channel>>,
    sender_key: &str,
) -> bool {
    // This is a host dispatch hint; the module alone parses and recognizes
    // commands. Ordinary messages avoid creating a private callback object.
    if !msg.text.trim_start().starts_with('/') {
        return false;
    }
    let current = get_route_selection(ctx, sender_key);
    let history_len = ctx
        .conversation_histories
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(sender_key)
        .map(Vec::len)
        .unwrap_or(0);
    let client = super::context::module_client(ctx).await;
    let operation = async {
        let lease = crate::modules::channel_runtime::ChannelRuntimeLease::open(
            client,
            RouteCallbacks::new(ctx.clone(), super::runtime::session::channel_session()),
        )
        .await?;
        let response: Result<ChannelControlResult, _> = lease
            .object()
            .call_confidential(
                tinychannels_bus::methods::PREPARE_CHANNEL_CONTROL,
                (ChannelControlRequest::Route {
                    workspace_dir: ctx.workspace_dir.to_string_lossy().into_owned(),
                    message: Box::new(route_compat_message(msg)),
                    capabilities: target_channel.map(|channel| Box::new(channel.capabilities())),
                    route: tinychannels_bus::module_remote::RemoteRoute {
                        provider: current.provider,
                        model: current.model,
                    },
                    history_len,
                    providers: ctx
                        .config
                        .as_deref()
                        .map(provider_descriptors)
                        .unwrap_or_default(),
                },),
            )
            .await;
        let close = lease.close().await;
        let response = response?;
        close?;
        Ok::<_, tinybus::module::ModuleCallError>(response)
    }
    .await;
    let (reply, next_route, clear_history) = match operation {
        Ok(ChannelControlResult::Route {
            reply,
            next_route,
            clear_history,
        }) => (reply, next_route, clear_history),
        Ok(_) => {
            crate::modules::report::malformed_reply("tinychannels");
            return true;
        }
        Err(_) => {
            if let Some(channel) = target_channel {
                let _ = channel
                    .send_typed(&text_message(
                        &msg.channel.id,
                        &msg.conversation.id,
                        "Channel module unavailable.",
                        msg.conversation.thread_id.clone(),
                    ))
                    .await;
            }
            return true;
        }
    };
    if let Some(route) = next_route {
        set_route_selection(
            ctx,
            sender_key,
            ChannelRouteSelection {
                provider: route.provider,
                model: route.model,
            },
        );
    }
    if clear_history {
        clear_sender_history(&ctx.conversation_histories, sender_key);
    }
    let Some(reply) = reply else {
        return false;
    };
    if let Some(channel) = target_channel {
        if channel
            .send_typed(&text_message(
                &msg.channel.id,
                &msg.conversation.id,
                reply,
                msg.conversation.thread_id.clone(),
            ))
            .await
            .is_err()
        {
            tracing::warn!("channel command response failed");
        }
    }
    true
}

#[cfg(test)]
#[path = "routes_tests.rs"]
mod tests;
