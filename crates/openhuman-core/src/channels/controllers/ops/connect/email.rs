//! Building, verifying, and persisting the email (IMAP/SMTP) channel config.

use serde_json::Value;

use crate::channels::email_channel::{EmailChannel, EmailConfig};
use crate::config::Config;
use tinychannels_bus::traits::Channel as _;

use super::shared::parse_optional_bool;

/// Read a required non-empty string credential field.
fn require_cred_str(creds: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    creds
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("missing required field: {key}"))
}

/// Read an optional non-empty string credential field.
fn optional_cred_str(creds: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    creds
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Parse a `u16` port from a string/number credential field, falling back to
/// `default` when the field is absent or blank. Non-numeric values are a hard
/// error so a typo surfaces at connect time rather than silently reverting.
pub(crate) fn parse_port_field(
    creds: &serde_json::Map<String, Value>,
    key: &str,
    default: u16,
) -> Result<u16, String> {
    // Port 0 is the OS "any" sentinel — never a valid mailbox port — so reject it
    // up front rather than letting it fail later with a generic connect error.
    let invalid = || format!("invalid {key}: must be a port number 1-65535");
    match creds.get(key) {
        Some(Value::Number(n)) => n
            .as_u64()
            .filter(|v| (1..=u64::from(u16::MAX)).contains(v))
            .map(|v| v as u16)
            .ok_or_else(invalid),
        Some(Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Ok(default)
            } else {
                match trimmed.parse::<u16>() {
                    Ok(0) | Err(_) => Err(invalid()),
                    Ok(port) => Ok(port),
                }
            }
        }
        None | Some(Value::Null) => Ok(default),
        _ => Err(invalid()),
    }
}

/// Parse the email `allowed_senders` allowlist from a comma/newline-separated
/// credential field. Unlike [`parse_allowed_users`](super::shared::parse_allowed_users),
/// this preserves a leading `@` (the domain-match syntax `@example.com` the
/// email channel relies on) and does not force lowercase beyond what the
/// channel already does at match time. An absent field defaults to `["*"]`
/// (allow any) so a freshly-connected mailbox actually receives — the channel
/// treats an *empty* list as deny-all.
pub(crate) fn parse_email_senders(value: Option<&Value>) -> Vec<String> {
    let raw = match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(","),
        _ => return vec!["*".to_string()],
    };

    let mut out: Vec<String> = Vec::new();
    for part in raw.split([',', '\n', '\r']) {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.iter().any(|e| e.eq_ignore_ascii_case(trimmed)) {
            out.push(trimmed.to_string());
        }
    }
    if out.is_empty() {
        out.push("*".to_string());
    }
    out
}

/// Build an [`EmailConfig`] from the connect form's credential map, filling
/// sensible defaults (ports 993/465, TLS on, folder INBOX, `from_address` =
/// username, allowlist = `*`). Field keys map 1:1 to the `email` channel
/// definition. Reuses `existing` only for the IDLE timeout so an advanced
/// hand-set value survives a UI reconnect.
pub(super) fn build_email_config(
    creds: &serde_json::Map<String, Value>,
    existing: Option<&EmailConfig>,
) -> Result<EmailConfig, String> {
    let username = require_cred_str(creds, "username")?;
    let from_address = optional_cred_str(creds, "from_address").unwrap_or_else(|| username.clone());
    Ok(EmailConfig {
        imap_host: require_cred_str(creds, "imap_host")?,
        imap_port: parse_port_field(creds, "imap_port", 993)?,
        imap_folder: optional_cred_str(creds, "imap_folder").unwrap_or_else(|| "INBOX".to_string()),
        smtp_host: require_cred_str(creds, "smtp_host")?,
        smtp_port: parse_port_field(creds, "smtp_port", 465)?,
        smtp_tls: parse_optional_bool(creds.get("smtp_tls")).unwrap_or(true),
        username,
        password: require_cred_str(creds, "password")?,
        from_address,
        idle_timeout_secs: existing.map_or(1740, |c| c.idle_timeout_secs),
        allowed_senders: parse_email_senders(creds.get("allowed_senders")),
    })
}

/// Live-verify IMAP credentials by attempting a login. Runs before persistence
/// so a wrong host/password fails fast in the UI instead of silently wedging
/// the listener on the next core restart.
pub(super) async fn verify_email_credentials(cfg: &EmailConfig) -> Result<(), String> {
    // The probe dials IMAP + logs in over the network on the connect/test RPC
    // path, so bound it: a blackholed host or stalled TLS handshake must not
    // hang the UI. `health_check` has its own inner budget; this is a hard outer
    // cap that also distinguishes a timeout from an auth failure for the user.
    let probe = EmailChannel::new(cfg.clone());
    match tokio::time::timeout(std::time::Duration::from_secs(20), probe.health_check()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(
            "IMAP connection failed — check the host, port, email address, and app password"
                .to_string(),
        ),
        Err(_) => Err(format!(
            "IMAP connection to {} timed out — check the host and port",
            cfg.imap_host
        )),
    }
}

/// Persist an already-built + verified [`EmailConfig`] into
/// `channels_config.email` so the supervised IMAP/SMTP listener picks it up on
/// the next restart. Kept separate from the verify step so persistence is unit
/// testable without a live mailbox.
///
/// The `password` is deliberately **not** written to `config.toml` — the secret
/// lives only in the encrypted credentials store (written on the generic connect
/// path under `channel:email:api_key`) and is re-hydrated at startup by
/// `resolve_email_password`. Mirrors the Yuanbao `app_secret` handling.
pub(crate) async fn persist_email_config(
    config: &Config,
    mut email_cfg: EmailConfig,
) -> Result<(), String> {
    let allowed_senders_count = email_cfg.allowed_senders.len();
    let smtp_tls = email_cfg.smtp_tls;
    // Strip the secret before it ever touches disk.
    email_cfg.password = String::new();

    let mut persisted = config.clone();
    persisted.channels_config.email = Some(email_cfg);
    persisted
        .save()
        .await
        .map_err(|e| format!("failed to persist email config.toml: {e}"))?;

    tracing::info!(
        target: "openhuman::channels",
        allowed_senders_count,
        smtp_tls,
        "[email] connect_channel: wrote channels_config.email (password kept in credentials store); restart core for IMAP/SMTP listener"
    );
    Ok(())
}
