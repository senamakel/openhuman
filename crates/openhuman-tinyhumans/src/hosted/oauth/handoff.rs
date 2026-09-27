//! Decryption of the backend's one-time OAuth token handoff
//! (`POST /auth/integrations/{id}/tokens` returns the tokens AES-256-GCM
//! encrypted under a key the caller supplied).

use anyhow::{Context, Result};
use base64::Engine;

/// AES-256-GCM decrypt compatible with backend `encryptMessageFromString` (IV 16 + tag 16 + ciphertext, base64).
pub fn decrypt_handoff_blob(b64_ciphertext: &str, key_str: &str) -> Result<String> {
    let key = key_bytes_from_string(key_str)?;
    let combined = base64::engine::general_purpose::STANDARD
        .decode(b64_ciphertext.trim())
        .context("base64-decode encrypted payload")?;
    if combined.len() < 32 {
        anyhow::bail!("encrypted payload too short");
    }
    let iv = &combined[0..16];
    let tag = &combined[16..32];
    let ciphertext = &combined[32..];

    // aes-gcm expects ciphertext || tag
    let mut ct_with_tag = Vec::with_capacity(ciphertext.len() + tag.len());
    ct_with_tag.extend_from_slice(ciphertext);
    ct_with_tag.extend_from_slice(tag);

    use aes_gcm::aead::generic_array::typenum::U16;
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::aes::Aes256;
    use aes_gcm::AesGcm;
    type Aes256Gcm16 = AesGcm<Aes256, U16>;

    let cipher =
        Aes256Gcm16::new_from_slice(&key).map_err(|e| anyhow::anyhow!("invalid AES key: {e}"))?;
    let nonce = aes_gcm::aead::generic_array::GenericArray::from_slice(iv);
    let plain = cipher
        .decrypt(nonce, ct_with_tag.as_ref())
        .map_err(|e| anyhow::anyhow!("AES-GCM decrypt failed: {e}"))?;

    String::from_utf8(plain).context("handoff plaintext is not UTF-8")
}

/// Decode the shared encryption key into 32 raw AES bytes.
///
/// Accepts, in order of preference:
/// 1. base64url without padding — the current backend format (e.g.
///    a 43-char alphanumeric string using `-` / `_`). This must be tried
///    BEFORE standard base64 because `-`/`_` are invalid in the standard
///    alphabet and would fail cleanly, whereas a standard-base64 string
///    never contains `-`/`_` so base64url_no_pad will still decode it
///    correctly as long as there's no padding.
/// 2. base64url with padding.
/// 3. Standard base64 with padding (legacy backend format).
/// 4. Standard base64 without padding.
/// 5. A raw 32-byte ASCII key (len == 32, used as-is).
///
/// NOTE: the key is only decoded locally for AES-GCM key material in
/// `decrypt_handoff_blob`. The key sent back to the backend (in the
/// `{ key: ... }` POST body of `fetch_integration_tokens_handoff`) is the
/// original string — never re-encoded — so base64url keys stay base64url
/// on the wire.
fn key_bytes_from_string(key: &str) -> Result<Vec<u8>> {
    let trimmed = key.trim();

    // Raw 32-byte ASCII key
    if trimmed.len() == 32 && !trimmed.contains(['+', '/', '-', '_', '=']) {
        return Ok(trimmed.as_bytes().to_vec());
    }

    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

    // `base64::Engine` has generic methods and therefore isn't
    // dyn-compatible, so we unroll the attempts instead of looping over
    // a slice of trait objects.
    macro_rules! try_decode {
        ($engine:expr) => {
            if let Ok(decoded) = $engine.decode(trimmed) {
                if decoded.len() == 32 {
                    return Ok(decoded);
                }
            }
        };
    }
    try_decode!(URL_SAFE_NO_PAD);
    try_decode!(URL_SAFE);
    try_decode!(STANDARD);
    try_decode!(STANDARD_NO_PAD);

    anyhow::bail!(
        "encryption key must decode to 32 raw bytes (raw, base64, or base64url accepted; got len={})",
        trimmed.len()
    );
}

#[cfg(test)]
#[path = "handoff_tests.rs"]
mod tests;
