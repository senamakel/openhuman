//! The bus endpoint of the storage host: how `tinystoragedrivers-busproxy`
//! reaches [`StorageHost`], and how a caller's watch invalidations get from the
//! caller's bound server to the proxy's unicast forwarder.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tinybus::{
    BusName, Connection, InterfaceName, MatchRule, MemberName, ObjectPath, WeakConnection,
    BUS_INTERFACE, BUS_NAME, BUS_PATH,
};
use tinystoragedrivers::BlobReader;
use tinystoragedrivers_bus::{BlobKeyRequest, BlobStreamHead, PutBlobStreamRequest, WatchChanged};
use tinystoragedrivers_busproxy::{HostFailure, StorageCall, StorageEndpoint};
use tokio::sync::broadcast;

use super::{Binding, HostError, StorageHost};

/// Relays kept for callers that never became a bound module.
const MAX_IDLE_RELAYS: usize = 256;

/// Where one caller's watch invalidations are collected for the bus endpoint.
///
/// The endpoint asks for a caller's signals synchronously and before the
/// caller's first `WatchOpen` reaches the host, when the host has not yet
/// resolved which module the caller is. So the relay exists per caller from
/// the first ask, and [`StorageHost::handle`] feeds it from the caller's bound
/// server as soon as the caller is a known module (before the call is
/// forwarded, so no signal is missed).
pub(super) struct Relay {
    pub(super) tx: broadcast::Sender<WatchChanged>,
    /// The binding currently feeding `tx`, so a rebind starts a new bridge.
    pub(super) fed_by: Option<std::sync::Weak<Binding>>,
    pub(super) bridge: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        if let Some(bridge) = self.bridge.take() {
            bridge.abort();
        }
    }
}

/// Signals buffered per caller before the slowest reader loses the oldest.
const RELAY_BUFFER: usize = 256;

impl StorageHost {
    /// The watch invalidations of `sender`, for the bus endpoint to unicast.
    pub(super) fn watch_signals(
        &self,
        sender: &BusName,
    ) -> Option<broadcast::Receiver<WatchChanged>> {
        let mut relays = self.relays.lock().ok()?;
        if !relays.contains_key(sender) {
            // Callers that never became a module leave an idle relay behind;
            // keep those bounded.
            if relays.len() >= MAX_IDLE_RELAYS {
                relays.retain(|_, relay| relay.bridge.is_some() || relay.tx.receiver_count() > 0);
            }
            if relays.len() >= MAX_IDLE_RELAYS {
                return None;
            }
            let (tx, _) = broadcast::channel(RELAY_BUFFER);
            relays.insert(
                sender.clone(),
                Relay {
                    tx,
                    fed_by: None,
                    bridge: None,
                },
            );
        }
        relays.get(sender).map(|relay| relay.tx.subscribe())
    }

    /// Drop the signals relay for a peer the broker has detached.
    pub(super) fn release_relay(&self, sender: &BusName) {
        if let Ok(mut relays) = self.relays.lock() {
            relays.remove(sender);
        }
    }

    /// Starts feeding `sender`'s relay from `binding`'s server, replacing a
    /// bridge from an earlier binding. The bridge subscribes before it is
    /// spawned, so nothing the server signals afterwards is lost.
    pub(super) fn feed_relay(&self, sender: &BusName, binding: &Arc<Binding>) {
        let Ok(mut relays) = self.relays.lock() else {
            return;
        };
        let Some(relay) = relays.get_mut(sender) else {
            // Nobody asked for this caller's signals (it never opened a watch
            // through the bus), so there is nothing to feed.
            return;
        };
        let current = relay
            .fed_by
            .as_ref()
            .is_some_and(|fed| fed.ptr_eq(&Arc::downgrade(binding)));
        if current
            && relay
                .bridge
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        if let Some(stale) = relay.bridge.take() {
            stale.abort();
        }
        let mut signals = binding.server.signals();
        let tx = relay.tx.clone();
        relay.fed_by = Some(Arc::downgrade(binding));
        relay.bridge = Some(
            // The forwarder only moves signals between channels and reads no
            // scope, so it needs no tenant.
            crate::core::runtime::DispatchCtx::host_default().spawn(async move {
                loop {
                    match signals.recv().await {
                        Ok(signal) => {
                            // No receiver just means the bus forwarder is gone.
                            let _ = tx.send(signal);
                        }
                        Err(broadcast::error::RecvError::Lagged(missed)) => {
                            // The watch ids in the skipped batch are gone; the
                            // signal carries no reset form, so record the loss.
                            tracing::warn!(
                                target: "storage_host",
                                missed,
                                "[storage-host] watch relay lagged; dropped watch signals"
                            );
                        }
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }),
        );
    }
}

/// The host as the bus endpoint `tinystoragedrivers-busproxy` serves the four
/// objects from. The proxy owns the wire (positional arguments, the peer-bound
/// stream accept and open, unicast watch signals); every decision stays in
/// [`StorageHost`].
pub(super) struct HostEndpoint {
    host: Arc<StorageHost>,
    connection: WeakConnection,
    /// Where the work runs when the objects are served from a linked module's
    /// own runtime: the host's, which the backend handles were opened on.
    runtime: Option<tokio::runtime::Handle>,
}

impl HostEndpoint {
    pub(super) fn new(
        host: Arc<StorageHost>,
        connection: WeakConnection,
        runtime: Option<tokio::runtime::Handle>,
    ) -> Self {
        Self {
            host,
            connection,
            runtime,
        }
    }

    /// Runs `work` on the host's runtime when one was given.
    async fn on_host<T, F>(&self, work: F) -> Result<T, HostFailure>
    where
        T: Send + 'static,
        F: std::future::Future<Output = Result<T, HostError>> + Send + 'static,
    {
        match &self.runtime {
            None => work.await.map_err(Into::into),
            Some(runtime) => runtime
                .spawn(work)
                .await
                .map_err(|_| HostFailure::Denied("call-cancelled"))?
                .map_err(Into::into),
        }
    }
}

#[async_trait]
impl StorageEndpoint for HostEndpoint {
    async fn call(&self, call: StorageCall<'_>) -> Result<Value, HostFailure> {
        let host = Arc::clone(&self.host);
        let (object, request, admission) = (call.object, call.request, call.admission);
        let selector = call.selector.to_string();
        let is_watch_open = selector == "WatchOpen";
        let sender = call.sender.cloned();
        let sender_for_liveness = sender.clone();
        let result = self
            .on_host(async move {
                host.handle(object, &selector, request, sender.as_ref(), admission)
                    .await
            })
            .await;
        if is_watch_open {
            if let Some(sender) = sender_for_liveness {
                // The bus dispatches calls concurrently with signals. A queued
                // WatchOpen can reach this endpoint after its detach signal
                // already removed the relay, so check liveness after handling
                // the call as well as listening for detach events.
                if let Some(connection) = self.connection.upgrade() {
                    match connection.name_owner(sender.as_str()).await {
                        Ok(Some(owner)) if owner == sender => {}
                        Ok(_) => self.host.release_peer(&sender),
                        Err(error) => tracing::debug!(
                            target: "storage_host",
                            %error,
                            "[storage-host] could not confirm watch caller liveness"
                        ),
                    }
                } else {
                    self.host.release_peer(&sender);
                }
            }
        }
        result
    }

    async fn put_blob_stream(
        &self,
        sender: Option<&BusName>,
        head: PutBlobStreamRequest,
        body: BlobReader,
    ) -> Result<Value, HostFailure> {
        let host = Arc::clone(&self.host);
        let sender = sender.cloned();
        self.on_host(async move { host.put_blob_stream(sender.as_ref(), head, body).await })
            .await
    }

    async fn get_blob_stream(
        &self,
        sender: Option<&BusName>,
        request: BlobKeyRequest,
    ) -> Result<Option<(BlobStreamHead, BlobReader)>, HostFailure> {
        let host = Arc::clone(&self.host);
        let sender = sender.cloned();
        self.on_host(async move { host.get_blob_stream(sender.as_ref(), request).await })
            .await
    }

    fn watch_signals(&self, sender: &BusName) -> Option<broadcast::Receiver<WatchChanged>> {
        self.host.watch_signals(sender)
    }
}

/// Removes caller relays when the broker reports their unique peer detached.
/// Well-known-name releases from live peers do not match this predicate.
pub(super) async fn watch_peer_detach(
    connection: &Connection,
    host: Arc<StorageHost>,
) -> tinybus::Result<()> {
    let mut rule = MatchRule::new()
        .signals()
        .sender(BusName::new(BUS_NAME)?)
        .interface(InterfaceName::new(BUS_INTERFACE)?)
        .member(MemberName::new("NameOwnerChanged")?);
    rule.path = Some(ObjectPath::new(BUS_PATH)?);
    let mut signals = connection.add_match(rule.clone()).await?;
    let connection = connection.downgrade();
    tokio::spawn(async move {
        loop {
            match signals.recv().await {
                Ok(signal) => {
                    if !rule.matches(&signal) {
                        continue;
                    }
                    let Ok((name, old_owner, new_owner)) =
                        serde_json::from_value::<(BusName, Option<BusName>, Option<BusName>)>(
                            signal.body,
                        )
                    else {
                        tracing::warn!(
                            target: "storage_host",
                            "[storage-host] ignored malformed name-owner notification"
                        );
                        continue;
                    };
                    if name.is_unique() && old_owner.as_ref() == Some(&name) && new_owner.is_none()
                    {
                        host.release_peer(&name);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    tracing::warn!(
                        target: "storage_host",
                        missed,
                        "[storage-host] peer-detach notifications lagged"
                    );
                    let candidates: HashSet<_> = host.peer_candidates();
                    let Some(connection) = connection.upgrade() else {
                        return;
                    };
                    if let Ok(names) = connection.list_names().await {
                        for peer in candidates {
                            if !names.contains(&peer) {
                                host.release_peer(&peer);
                            }
                        }
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    Ok(())
}
