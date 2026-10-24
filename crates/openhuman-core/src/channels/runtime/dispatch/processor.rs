//! Core message processing loop for the channel runtime.
//!
//! Contains:
//! * [`channel_has_approval_surface`] — gate controlling per-channel approval
//!   context scoping.
//! * [`admit`] — preparation, the received event, and the runtime-command and
//!   approval-reply intercepts that run before any debounce.
//! * [`consumes_approval_reply`] (in [`approval`]) — intercepts yes/no
//!   approval replies before dispatching a fresh agent turn.
//! * [`process_channel_message`] — full per-message pipeline: typing, ACK
//!   reaction, history, agent turn, draft updates, reply.
//! * [`bus_turn`] — the orchestrator path: an unbound channel's turn over
//!   the native bus. A channel bound to a host agent runs through
//!   [`super::host_agent`] instead.
//! * [`run_message_dispatch_loop`] — admits inbound messages, debounces them
//!   per conversation through [`crate::inbound_debounce`], runs one turn per
//!   batch with a bounded set of host workers, and drains everything when the
//!   inbound queue closes.

mod admit;
mod approval;
mod bus_turn;
mod in_order;
mod turn;

use super::inbound_batch::{merge_channel_batch, QueuedInbound};
use crate::channels::context::ChannelRuntimeContext;
use crate::config::InboundDebounceConfig;
use crate::inbound_debounce::{KeyedDebouncer, Push};
use in_order::InOrder;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tinychannels_bus::ChannelInboundEnvelope;
use tokio::task::JoinSet;
use tokio::time::Instant;

/// Host-side inbound work item. Relay authentication remains out-of-band and
/// can only be set by the native callback after receiving the trusted module
/// callback; it is never reconstructed from envelope metadata.
#[derive(Clone)]
pub(crate) struct RuntimeChannelMessage {
    pub(crate) message: ChannelInboundEnvelope,
    pub(crate) authenticated_relay: bool,
    pub(crate) prepared_inbound: Option<tinychannels_bus::module_inbound::PreparedChannelInbound>,
}

impl RuntimeChannelMessage {
    pub(crate) fn with_inbound_envelope(message: ChannelInboundEnvelope) -> Self {
        Self {
            message,
            authenticated_relay: false,
            prepared_inbound: None,
        }
    }

    pub(crate) fn from_authenticated_relay(
        delivery: tinychannels_bus::module_relay::RelayInboundDelivery,
    ) -> Self {
        Self {
            message: delivery.envelope,
            authenticated_relay: delivery.delivered_via_authenticated_relay,
            prepared_inbound: None,
        }
    }
}

use admit::admit_runtime_message;
pub(crate) use admit::AdmittedInbound;
#[cfg(test)]
pub(crate) use approval::channel_has_approval_surface;
#[cfg(test)]
pub(crate) use turn::process_channel_message;
pub(crate) use turn::process_channel_runtime_message;
use turn::run_admitted_turn;

/// Run every inbound message through the agent pipeline until `rx` closes.
///
/// Admitted messages are debounced per conversation (`history_key`): a burst
/// becomes one turn, and messages that arrive while a conversation's turn is
/// running are collected into one follow-up turn. At most
/// `max_in_flight_messages` turns run at once, and the loop stops receiving
/// while [`intake_budget`] admitted messages are waiting for a turn, so a
/// sustained sender is held back by the bounded inbound queue.
pub(crate) async fn run_message_dispatch_loop(
    rx: tokio::sync::mpsc::Receiver<RuntimeChannelMessage>,
    ctx: Arc<ChannelRuntimeContext>,
    max_in_flight_messages: usize,
    debounce: InboundDebounceConfig,
) {
    let max_in_flight = max_in_flight_messages.max(1);
    let budget = intake_budget(max_in_flight, &debounce);
    let mut held: usize = 0;
    let mut rx = rx;
    let mut inbound_open = true;
    let mut next_seq: u64 = 0;
    let mut admitting = JoinSet::new();
    let mut admit_seqs: HashMap<tokio::task::Id, u64> = HashMap::new();
    let mut admission_order = InOrder::new();
    let mut turns = JoinSet::new();
    let mut turn_keys: HashMap<tokio::task::Id, String> = HashMap::new();
    let mut typing = JoinSet::new();
    let mut debouncer: KeyedDebouncer<String, QueuedInbound> = KeyedDebouncer::new();
    let mut ready: VecDeque<(String, Vec<QueuedInbound>)> = VecDeque::new();

    loop {
        while turns.len() < max_in_flight {
            let Some((key, batch)) = ready.pop_front() else {
                break;
            };
            let batch_len = batch.len();
            held = held.saturating_sub(batch_len);
            match merge_channel_batch(batch) {
                Some(admitted) => {
                    tracing::debug!(
                        channel = %admitted.msg.channel.id,
                        message_id = %admitted.msg.message_id,
                        batch_len,
                        in_flight = turns.len() + 1,
                        held,
                        "[dispatch][debounce] starting turn"
                    );
                    let handle = turns.spawn(run_admitted_turn(Arc::clone(&ctx), admitted));
                    turn_keys.insert(handle.id(), key);
                }
                None => debouncer.complete(&key),
            }
        }
        while let Some(result) = typing.try_join_next() {
            log_join_error("typing", result.err());
        }
        if !inbound_open
            && admitting.is_empty()
            && turns.is_empty()
            && ready.is_empty()
            && debouncer.is_empty()
        {
            break;
        }

        let wakeup = debouncer.next_wakeup();
        tokio::select! {
            received = rx.recv(), if inbound_open
                && admitting.len() < max_in_flight
                && admitting.len() + admission_order.waiting() + held < budget => {
                match received {
                    Some(message) => {
                        let seq = next_seq;
                        next_seq += 1;
                        let handle = admitting.spawn(admit_runtime_message(Arc::clone(&ctx), message));
                        admit_seqs.insert(handle.id(), seq);
                    }
                    None => {
                        tracing::debug!(
                            buffered_keys = debouncer.tracked(),
                            "[dispatch][debounce] inbound queue closed; flushing buffered messages"
                        );
                        inbound_open = false;
                    }
                }
            }
            Some(joined) = admitting.join_next_with_id(), if !admitting.is_empty() => {
                let (id, result) = match joined {
                    Ok((id, result)) => (id, result),
                    Err(error) => {
                        let id = error.id();
                        log_join_error("admission", Some(error));
                        (id, None)
                    }
                };
                if let Some(seq) = admit_seqs.remove(&id) {
                    for (seq, inbound) in admission_order.finish(seq, result) {
                        held += 1;
                        buffer_admitted(&ctx, &debounce, &mut debouncer, &mut typing, seq, inbound);
                    }
                }
            }
            Some(joined) = turns.join_next_with_id(), if !turns.is_empty() => {
                let id = match joined {
                    Ok((id, ())) => id,
                    Err(error) => {
                        let id = error.id();
                        log_join_error("turn", Some(error));
                        id
                    }
                };
                if let Some(key) = turn_keys.remove(&id) {
                    debouncer.complete(&key);
                }
            }
            _ = tokio::time::sleep_until(wakeup.unwrap_or_else(Instant::now)), if wakeup.is_some() => {}
        }

        let now = Instant::now();
        if !inbound_open && admitting.is_empty() {
            debouncer.flush_all(now);
        }
        ready.extend(debouncer.poll_ready(now));
    }
}

/// Admitted messages the loop holds before it stops receiving: a full batch
/// for every turn worker.
fn intake_budget(max_in_flight: usize, debounce: &InboundDebounceConfig) -> usize {
    max_in_flight.saturating_mul(debounce.max_batch_messages.max(1))
}

fn buffer_admitted(
    ctx: &Arc<ChannelRuntimeContext>,
    debounce: &InboundDebounceConfig,
    debouncer: &mut KeyedDebouncer<String, QueuedInbound>,
    typing: &mut JoinSet<()>,
    seq: u64,
    inbound: AdmittedInbound,
) {
    let settings = debounce.settings_for(&inbound.msg.channel.id);
    let channel = inbound.msg.channel.id.clone();
    let reply_target = inbound.msg.conversation.id.clone();
    let key = inbound.history_key.clone();
    let push = debouncer.push(
        key,
        QueuedInbound { seq, inbound },
        &settings,
        Instant::now(),
    );
    tracing::debug!(
        channel = %channel,
        seq,
        outcome = ?push,
        tracked = debouncer.tracked(),
        "[dispatch][debounce] buffered inbound message"
    );
    if push != Push::Opened {
        return;
    }
    if let Some(target) = ctx.channels_by_name.get(&channel).cloned() {
        typing.spawn(async move {
            if let Err(error) = target.start_typing(&reply_target).await {
                tracing::debug!(
                    "[dispatch][debounce] typing start failed on {}: {error}",
                    target.name()
                );
            }
        });
    }
}

fn log_join_error(stage: &str, error: Option<tokio::task::JoinError>) {
    if let Some(error) = error {
        tracing::warn!("channel dispatch {stage} worker failed: {error}");
    }
}
