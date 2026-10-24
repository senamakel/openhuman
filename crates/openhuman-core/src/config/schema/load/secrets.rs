use super::super::Config;
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set whenever a legacy `enc:` (XOR) secret is force-migrated to `enc2:`
/// during a `decrypt_config_secrets` pass. The flag is scoped to a single pass:
/// `decrypt_config_secrets` clears it on entry and reads (and clears) it on exit
/// so the caller can persist the upgraded config (audit C8). The worst case
/// under a (rare) concurrent load is an extra, idempotent `config.save()` — the
/// re-encryption is a no-op once values are already `enc2:`, so an occasional
/// spurious save is harmless and never loses data.
fn migration_flag() -> &'static AtomicBool {
    static MIGRATED: AtomicBool = AtomicBool::new(false);
    &MIGRATED
}

thread_local! {
    /// Fields a `decrypt_config_secrets` pass on this thread could not open
    /// (and cleared). The pass is synchronous, so a thread-local is exact.
    static DECRYPT_FAILURES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Decrypt one optional secret in place.
///
/// When the stored value uses the legacy, insecure `enc:` (XOR) format this
/// performs a **forced one-time migration**: the field is decrypted via
/// `decrypt_and_migrate` (which rewrites it to the `enc2:` / ChaCha20-Poly1305
/// format) and the process-wide migration flag is raised so the caller can
/// persist the upgraded config back to disk. This stops legacy XOR ciphertext
/// from persisting indefinitely (see audit C8). Reading existing `enc:` values
/// still succeeds throughout the migration.
fn decrypt_optional_secret(
    store: &crate::security::secret_storage::SecretStore,
    value: &mut Option<String>,
    field_name: &str,
) -> Result<()> {
    if let Some(raw) = value.clone() {
        if crate::security::secret_storage::SecretStore::is_encrypted(&raw) {
            // Legacy `enc:` values are migrated to `enc2:` on read so they are
            // rewritten on the next config save instead of lingering forever.
            match store.decrypt_and_migrate(&raw) {
                Ok((plaintext, migrated)) => {
                    if migrated.is_some() {
                        log::warn!(
                            "[security][config] migrated legacy enc: secret '{field_name}' \
                             to enc2: (ChaCha20-Poly1305); config will be re-saved to persist"
                        );
                        migration_flag().store(true, Ordering::Relaxed);
                    }
                    *value = Some(plaintext);
                }
                Err(e) => {
                    // Decryption key is inaccessible (e.g. rotated, keyring reset, or
                    // migrated across machines). Clear the field so config loads
                    // successfully — the affected integration will be disabled until
                    // the user re-enters the credential. A hard error here would block
                    // every config load and make the app unusable.
                    log::warn!(
                        "[config] Failed to decrypt {field_name} — field cleared (key inaccessible): {e}"
                    );
                    crate::security::keyring_consent::policy::notify_decrypt_failure(
                        field_name,
                        &e.to_string(),
                    );
                    DECRYPT_FAILURES.with(|failures| failures.set(failures.get() + 1));
                    *value = None;
                }
            }
        }
    }
    Ok(())
}

/// [`decrypt_config_secrets`] for a config about to be written somewhere
/// other nodes read (the shared config document): every process-key field must
/// open. The load path clears a field it cannot decrypt so the app stays
/// usable; saving that cleared value over the shared document would turn a
/// transient keychain failure into lost credentials, so this fails instead.
pub(super) fn decrypt_config_secrets_strict(
    config: &mut Config,
    openhuman_dir: &Path,
) -> Result<()> {
    DECRYPT_FAILURES.with(|failures| failures.set(0));
    decrypt_config_secrets(config, openhuman_dir)?;
    let failed = DECRYPT_FAILURES.with(|failures| failures.replace(0));
    if failed > 0 {
        anyhow::bail!(
            "{failed} config secret(s) are sealed under this node's key and could not be opened; \
             not saving them to the shared config document"
        );
    }
    Ok(())
}

fn encrypt_optional_secret(
    store: &crate::security::secret_storage::SecretStore,
    value: &mut Option<String>,
    field_name: &str,
) -> Result<()> {
    if let Some(raw) = value.clone() {
        // An empty value means "no key"; sealing it would make it look set.
        if !raw.is_empty() && !crate::security::secret_storage::SecretStore::is_encrypted(&raw) {
            *value = Some(
                store
                    .encrypt(&raw)
                    .with_context(|| format!("Failed to encrypt {field_name}"))?,
            );
        }
    }
    Ok(())
}

/// Decrypt all secret fields in the configuration that are marked as encrypted.
///
/// Called during config load when `secrets.encrypt` is true. Only decrypts
/// values that have the `enc:` or `enc2:` prefix; plaintext values are
/// returned as-is. This is a no-op when encryption is disabled.
///
/// Returns `true` if any field used the legacy, insecure `enc:` format and was
/// force-migrated to `enc2:` during this pass. The caller should persist the
/// config (e.g. `config.save()`) when this is `true` so the upgraded ciphertext
/// is written to disk and the legacy XOR value stops persisting (audit C8).
pub(super) fn decrypt_config_secrets(config: &mut Config, openhuman_dir: &Path) -> Result<bool> {
    if !config.secrets.encrypt {
        return Ok(false);
    }
    // Reset the per-pass migration flag before decrypting any field.
    migration_flag().store(false, Ordering::Relaxed);
    let store = crate::security::secret_storage::SecretStore::new(openhuman_dir, true);

    // A MongoDB URL carries the database password.
    // Fail closed: if the sealed URL cannot be opened, keep the ciphertext so
    // the storage backend fails to open at boot instead of the field being
    // cleared into the "no URL" sentinel (the classic on-disk layout).
    let sealed_storage_url = config.storage.url.clone();
    decrypt_optional_secret(&store, &mut config.storage.url, "storage.url")?;
    if config.storage.url.is_none() && sealed_storage_url.is_some() {
        config.storage.url = sealed_storage_url;
    }

    for_each_secret(config, &mut |value, name| {
        decrypt_optional_secret(&store, value, name)
    })?;

    Ok(migration_flag().swap(false, Ordering::Relaxed))
}

/// Encrypt all secret fields in the configuration before writing to disk.
///
/// Called during `Config::save()` when `secrets.encrypt` is true. Only
/// encrypts values that are NOT already encrypted. This is a no-op when
/// encryption is disabled.
pub(super) fn encrypt_config_secrets(config: &mut Config) -> Result<()> {
    if !config.secrets.encrypt {
        return Ok(());
    }
    let parent_dir = config
        .config_path
        .parent()
        .context("Config path must have a parent directory")?;
    let store = crate::security::secret_storage::SecretStore::new(parent_dir, true);

    // A MongoDB URL carries the database password.
    encrypt_optional_secret(&store, &mut config.storage.url, "storage.url")?;

    for_each_secret(config, &mut |value, name| {
        encrypt_optional_secret(&store, value, name)
    })?;

    Ok(())
}

/// Every config field that holds a credential, listed once. Save and load both
/// walk this list, so a field cannot be sealed on one path and missed on the
/// other. `storage.url` is not here: its decrypt fails closed, so each pass
/// handles it itself.
fn for_each_secret(
    config: &mut Config,
    f: &mut dyn FnMut(&mut Option<String>, &str) -> Result<()>,
) -> Result<()> {
    f(&mut config.api_key, "api_key")?;
    f(&mut config.seltz.api_key, "seltz.api_key")?;
    for provider in [
        "parallel", "brave", "querit", "exa", "tavily", "gemini", "tinyfish",
    ] {
        if let Some(creds) = config.search.credentials_mut(provider) {
            f(&mut creds.api_key, &format!("search.{provider}.api_key"))?;
        }
    }
    f(&mut config.local_ai.api_key, "local_ai.api_key")?;
    f(&mut config.composio.api_key, "composio.api_key")?;
    let integrations = &mut config.integrations;
    for (name, toggle) in [
        ("google_places", &mut integrations.google_places),
        ("parallel", &mut integrations.parallel),
        ("tinyfish", &mut integrations.tinyfish),
        ("stock_prices", &mut integrations.stock_prices),
    ] {
        f(&mut toggle.api_key, &format!("integrations.{name}.api_key"))?;
    }
    // Empty means "read the environment", so it stays empty.
    string_secret(&mut config.hosting.api_key, "hosting.api_key", f)?;

    let ch = &mut config.channels_config;
    if let Some(ref mut tg) = ch.telegram {
        string_secret(&mut tg.bot_token, "telegram.bot_token", f)?;
    }
    if let Some(ref mut d) = ch.discord {
        string_secret(&mut d.bot_token, "discord.bot_token", f)?;
    }
    if let Some(ref mut s) = ch.slack {
        string_secret(&mut s.bot_token, "slack.bot_token", f)?;
        f(&mut s.app_token, "slack.app_token")?;
    }
    if let Some(ref mut m) = ch.mattermost {
        string_secret(&mut m.bot_token, "mattermost.bot_token", f)?;
    }
    if let Some(ref mut w) = ch.webhook {
        f(&mut w.secret, "webhook.secret")?;
    }
    if let Some(ref mut mx) = ch.matrix {
        string_secret(&mut mx.access_token, "matrix.access_token", f)?;
    }
    if let Some(ref mut wa) = ch.whatsapp {
        f(&mut wa.access_token, "whatsapp.access_token")?;
        f(&mut wa.verify_token, "whatsapp.verify_token")?;
        f(&mut wa.app_secret, "whatsapp.app_secret")?;
    }
    if let Some(ref mut lq) = ch.linq {
        string_secret(&mut lq.api_token, "linq.api_token", f)?;
    }
    if let Some(ref mut irc) = ch.irc {
        f(&mut irc.server_password, "irc.server_password")?;
        f(&mut irc.nickserv_password, "irc.nickserv_password")?;
        f(&mut irc.sasl_password, "irc.sasl_password")?;
    }
    if let Some(ref mut lk) = ch.lark {
        string_secret(&mut lk.app_secret, "lark.app_secret", f)?;
        f(&mut lk.encrypt_key, "lark.encrypt_key")?;
        f(&mut lk.verification_token, "lark.verification_token")?;
    }
    if let Some(ref mut dt) = ch.dingtalk {
        string_secret(&mut dt.client_secret, "dingtalk.client_secret", f)?;
    }
    if let Some(ref mut qq) = ch.qq {
        string_secret(&mut qq.app_secret, "qq.app_secret", f)?;
    }
    Ok(())
}

/// Runs `f` on a credential stored as a plain `String`, skipping it when empty.
fn string_secret(
    value: &mut String,
    field_name: &str,
    f: &mut dyn FnMut(&mut Option<String>, &str) -> Result<()>,
) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let mut slot = Some(value.clone());
    f(&mut slot, field_name)?;
    *value = slot.unwrap_or_default();
    Ok(())
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
