//! Event bus handlers for the devices domain.
//!
//! Subscribes to `tunnel:peer-status` and `tunnel:frame` events published by
//! `socket::event_handlers` and drives:
//! - Updating `PEER_STATUS` in `rpc.rs`.
//! - Completing the X25519 handshake when the device sends its pubkey.
//! - Persisting the `PairedDevice` record after a successful handshake.
//! - Publishing `DomainEvent::DevicePaired / DevicePeerOnline / DevicePeerOffline`.
//! - Resolving `tunnel:registered` acks for `tunnel_client`.

use crate::core::event_bus::{publish_global, DomainEvent, EventHandler};
use crate::openhuman::devices::rpc::{PEER_STATUS, PENDING_KEYPAIRS, PENDING_SESSIONS};
use crate::openhuman::devices::store;
use crate::openhuman::devices::tunnel_client::{resolve_register_ack, TunnelRegisterResponse};
use async_trait::async_trait;

/// Subscribes to device tunnel events from the event bus.
pub struct DeviceTunnelSubscriber;

impl DeviceTunnelSubscriber {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DeviceTunnelSubscriber {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventHandler for DeviceTunnelSubscriber {
    fn name(&self) -> &str {
        "device::tunnel"
    }

    fn domains(&self) -> Option<&[&str]> {
        Some(&["device"])
    }

    async fn handle(&self, event: &DomainEvent) {
        match event {
            DomainEvent::DevicePeerOnline { channel_id } => {
                handle_peer_online(channel_id).await;
            }
            DomainEvent::DevicePeerOffline { channel_id } => {
                handle_peer_offline(channel_id);
            }
            DomainEvent::DeviceTunnelFrame {
                channel_id,
                payload_b64,
            } => {
                handle_tunnel_frame(channel_id, payload_b64).await;
            }
            DomainEvent::DeviceTunnelRegistered {
                channel_id,
                pairing_token,
                session_token,
            } => {
                handle_registered(channel_id, pairing_token, session_token);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn handle_peer_online(channel_id: &str) {
    log::info!("[devices/bus] peer online channel_id={}", channel_id);
    PEER_STATUS
        .lock()
        .unwrap()
        .insert(channel_id.to_string(), true);
    // No re-publish: the event was already published by socket::event_handlers.
}

fn handle_peer_offline(channel_id: &str) {
    log::info!("[devices/bus] peer offline channel_id={}", channel_id);
    PEER_STATUS
        .lock()
        .unwrap()
        .insert(channel_id.to_string(), false);
    // No re-publish: the event was already published by socket::event_handlers.
}

/// Handle an incoming `tunnel:frame` — first frame from the device contains its
/// X25519 public key sealed to the core's public key. After successful decryption
/// we derive the shared secret and persist the `PairedDevice`.
async fn handle_tunnel_frame(channel_id: &str, payload_b64: &str) {
    log::debug!(
        "[devices/bus] tunnel:frame channel_id={} payload_len={}",
        channel_id,
        payload_b64.len()
    );

    // Look up the pending keypair for this channel.
    let keypair = {
        let map = PENDING_KEYPAIRS.lock().unwrap();
        map.get(channel_id).cloned()
    };

    let Some(keypair) = keypair else {
        log::debug!(
            "[devices/bus] no pending keypair for channel_id={} — frame ignored",
            channel_id
        );
        return;
    };

    // Decode the base64url payload.
    let frame_bytes = match crate::openhuman::devices::crypto::base64url_decode(payload_b64) {
        Ok(b) => b,
        Err(e) => {
            log::warn!(
                "[devices/bus] bad base64url in tunnel:frame channel_id={}: {e}",
                channel_id
            );
            return;
        }
    };

    // The first frame format (handshake): the device seals its pubkey (32 bytes)
    // to the core's pubkey using X25519 + XChaCha20-Poly1305. For v1 we treat
    // the frame payload as the raw device pubkey (base64url string in plaintext
    // after decoding the outer base64url layer). Full E2E encryption of the
    // handshake frame is a Layer 2 concern.
    //
    // v1 handshake: payload = base64url(device_x25519_pubkey_bytes)
    let device_pubkey_b64 = match String::from_utf8(frame_bytes) {
        Ok(s) => s.trim().to_string(),
        Err(_) => {
            log::warn!(
                "[devices/bus] tunnel:frame payload not valid UTF-8 for channel_id={}",
                channel_id
            );
            return;
        }
    };

    log::info!(
        "[devices/bus] handshake frame received channel_id={} device_pubkey_len={}",
        channel_id,
        device_pubkey_b64.len()
    );

    // Derive shared secret — if this fails the device sent a bad pubkey.
    if let Err(e) = keypair.derive_shared_secret(&device_pubkey_b64) {
        log::error!(
            "[devices/bus] X25519 key agreement failed channel_id={}: {e}",
            channel_id
        );
        return;
    }

    // Persist the paired device.
    let label = PENDING_SESSIONS
        .lock()
        .unwrap()
        .get(channel_id)
        .map(|s| s.channel_id.clone()) // use channel_id as fallback label
        .unwrap_or_else(|| channel_id.to_string());

    let session_token_hash = hash_session_token(
        &PENDING_SESSIONS
            .lock()
            .unwrap()
            .get(channel_id)
            .map(|s| s.core_session_token.clone())
            .unwrap_or_default(),
    );

    // Load config from global env (best-effort; pairing persists even if config
    // loading is slow — the UI will see the device on next list call).
    if let Ok(config) = crate::openhuman::config::rpc::load_config_with_timeout().await {
        match store::insert_device(
            &config,
            channel_id,
            &label,
            &device_pubkey_b64,
            &session_token_hash,
        ) {
            Ok(device) => {
                log::info!(
                    "[devices/bus] device persisted channel_id={} label={}",
                    device.channel_id,
                    device.label
                );
                publish_global(DomainEvent::DevicePaired {
                    channel_id: channel_id.to_string(),
                    device_pubkey: device_pubkey_b64,
                    label: Some(label),
                });
            }
            Err(e) => {
                log::error!(
                    "[devices/bus] failed to persist device channel_id={}: {e}",
                    channel_id
                );
            }
        }
    } else {
        log::warn!(
            "[devices/bus] could not load config to persist device channel_id={}",
            channel_id
        );
    }
}

/// Resolve the pending `tunnel:register` ack in `tunnel_client`.
fn handle_registered(channel_id: &str, pairing_token: &str, session_token: &str) {
    log::debug!(
        "[devices/bus] tunnel:registered channel_id={} token_len={}",
        channel_id,
        pairing_token.len()
    );
    resolve_register_ack(TunnelRegisterResponse {
        channel_id: channel_id.to_string(),
        pairing_token: pairing_token.to_string(),
        session_token: session_token.to_string(),
    });
}

fn hash_session_token(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    format!("{:x}", hasher.finalize())
}
