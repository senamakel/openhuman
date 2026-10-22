//! Shared-secret tickets that bind a Socket.IO connection to one SaaS profile.

use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const SECRET_ENV: &str = "OPENHUMAN_SOCKET_TICKET_SECRET";
pub const AUDIENCE: &str = "openhuman.socketio.v1";
pub const TTL_SECS: u64 = 30;
type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub aud: String,
    pub profile: String,
    pub exp: u64,
}

pub fn secret() -> Option<String> {
    std::env::var(SECRET_ENV)
        .ok()
        .filter(|value| value.len() >= 32)
}

pub fn mint(profile: &str, now: u64, secret: &str) -> Result<String, String> {
    if secret.len() < 32 {
        return Err("socket ticket secret must contain at least 32 bytes".into());
    }
    let claims = Claims {
        aud: AUDIENCE.into(),
        profile: profile.into(),
        exp: now.saturating_add(TTL_SECS),
    };
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&claims).map_err(|e| e.to_string())?);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
    mac.update(payload.as_bytes());
    let signature =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(format!("{payload}.{signature}"))
}

pub fn verify(ticket: &str, now: u64, secret: &str) -> Result<Claims, &'static str> {
    let (payload, signature) = ticket.split_once('.').ok_or("malformed ticket")?;
    let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| "malformed ticket")?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| "invalid key")?;
    mac.update(payload.as_bytes());
    mac.verify_slice(&signature)
        .map_err(|_| "invalid ticket signature")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "malformed ticket")?;
    let claims: Claims = serde_json::from_slice(&bytes).map_err(|_| "malformed ticket")?;
    if claims.aud != AUDIENCE {
        return Err("wrong ticket audience");
    }
    if claims.exp < now {
        return Err("expired ticket");
    }
    Ok(claims)
}
