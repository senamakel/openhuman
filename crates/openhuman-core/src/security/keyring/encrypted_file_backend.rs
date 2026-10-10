//! Encrypted-file keyring backend.
//!
//! Stores secrets in one ChaCha20-Poly1305-encrypted file. [`init_master_key`]
//! loads and caches the app-scoped key once, using [`MASTER_KEY_ENV`] or
//! [`MASTER_KEY_FILE_ENV`] for headless deployments, otherwise the OS keychain.
//! The backend itself never accesses the OS keychain, avoiding repeated macOS
//! permission prompts.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::security::keyring::adapter;
use crate::security::keyring::backend::KeyringBackend;
use crate::security::keyring::crypto::{self, KEY_LEN};
use crate::security::keyring::error::KeyringError;
use crate::security::keyring::file_store;
use crate::security::keyring::store::BackendKind;
use tinystoragedrivers::secrets::EncryptedFileSecrets;
use zeroize::Zeroizing;

const KEYCHAIN_SERVICE: &str = "openhuman";
const KEYCHAIN_MASTER_KEY_USERNAME: &str = "app:master_key";
/// Environment variable carrying the master key inline as `2 * KEY_LEN` hex
/// characters (`openssl rand -hex 32`). Lets a headless `openhuman-core
/// serve` — a container with no Secret Service or keychain — keep the
/// `encrypted_file` backend instead of falling back to the plaintext `file`
/// backend (#6926). Operators inject it from their secret manager the same
/// way they inject `OPENHUMAN_CORE_TOKEN`.
pub const MASTER_KEY_ENV: &str = "OPENHUMAN_KEYRING_MASTER_KEY";
/// Environment variable naming a file whose contents are the master key in
/// the same hex form (surrounding whitespace ignored), for Docker/Kubernetes
/// secret mounts. Mutually exclusive with [`MASTER_KEY_ENV`].
pub const MASTER_KEY_FILE_ENV: &str = "OPENHUMAN_KEYRING_MASTER_KEY_FILE";
const SECRETS_FILENAME: &str = "secrets.enc";
const LEGACY_DEV_KEYCHAIN: &str = "dev-keychain.json";

/// Outcome of the one-time master-key initialization: the key (`None` when
/// the backend needs none or the OS keychain could not provide it), or the
/// configuration error that rejected an operator-supplied source.
type MasterKeyInit = Result<Option<[u8; KEY_LEN]>, String>;

/// Process-wide master-key outcome, set once by [`init_master_key`].
static MASTER_KEY: OnceLock<MasterKeyInit> = OnceLock::new();

/// Set when [`init_master_key`] found the OS keychain unable to provide the
/// key, so storage secrets reuse that outcome instead of retrying the
/// keychain (and its prompt) on their first operation.
static KEYCHAIN_UNAVAILABLE: OnceLock<()> = OnceLock::new();

// ── Public API for core startup ──────────────────────────────────────────────

/// Initialize the keyring subsystem: set the workspace directory and load
/// the master encryption key (staging/production only) — from
/// [`MASTER_KEY_ENV`] or [`MASTER_KEY_FILE_ENV`] when an operator set one,
/// otherwise from the OS keychain.
///
/// Call this once at core startup before any keyring operations. In dev
/// environments the master key is not loaded (the plain file backend is
/// used instead). The result is cached process-wide; subsequent calls are
/// no-ops. Which source supplied the key is logged at `info`; the key never
/// is.
///
/// # Errors
///
/// Returns `Err` only when an operator-supplied source ([`MASTER_KEY_ENV`] /
/// [`MASTER_KEY_FILE_ENV`]) is set but unusable — both set, unreadable file,
/// wrong length, not hex. That is a configuration error the process should
/// not start with: continuing would run with secrets unreadable and fail
/// later, on the first store, with a less specific message. An OS-keychain
/// failure is **not** an error here: it keeps the #3311 behaviour (log,
/// notify the frontend, run with secrets inaccessible until keychain access
/// is restored). The outcome, error included, is cached process-wide, so
/// every later call after a configuration error returns the same `Err`.
pub fn init_master_key() -> Result<(), String> {
    // Ensure workspace dir is set for the backend before anything else.
    let dir = crate::security::keyring::store::workspace_dir_for_file_backend();
    log::info!(
        "[keyring] init_master_key: resolved workspace_dir={}",
        dir.display()
    );
    crate::security::keyring::init_workspace(&dir);

    init_once(&MASTER_KEY, || {
        let backend_kind = crate::security::keyring::store::effective_backend_kind();
        if backend_kind != BackendKind::EncryptedFile {
            log::debug!(
                "[keyring:encrypted_file] skipping master key init backend={backend_kind:?}"
            );
            return Ok(None);
        }

        match try_load_master_key() {
            Ok((key, source)) => {
                log::info!("[keyring:encrypted_file] master key loaded from {source}");
                Ok(Some(key))
            }
            Err(MasterKeyError::Configured(e)) => {
                log::error!(
                    "[keyring:encrypted_file] operator-supplied master key rejected; refusing \
                     to start with secrets unreadable. Cause: {e}"
                );
                Err(e)
            }
            Err(MasterKeyError::Keychain(e)) => {
                log::error!(
                    "[keyring:encrypted_file] master key load FAILED — refusing to mint a \
                     replacement (that would orphan existing secrets, #3311). Secrets are \
                     inaccessible this session and recover once OS keychain access is \
                     restored. Cause: {e}"
                );
                // Surface the denied state to the frontend instead of silently
                // resetting — this is the "warn before reset" the issue asks for.
                crate::security::keyring_consent::policy::notify_master_key_unavailable(&e);
                let _ = KEYCHAIN_UNAVAILABLE.set(());
                Ok(None)
            }
        }
    })
}

/// The master key that encrypts secrets on a configured storage backend
/// ([`crate::storage::secrets`]): the key [`init_master_key`] loaded when
/// there is one, otherwise the same resolution run once for storage —
/// [`MASTER_KEY_ENV`] / [`MASTER_KEY_FILE_ENV`] first, then the OS keychain.
///
/// # Errors
///
/// When no source can provide the key. Storage secrets then fail closed:
/// they are never written unencrypted or under a freshly minted key that
/// would orphan the ones already stored.
pub(crate) fn storage_master_key() -> Result<[u8; KEY_LEN], String> {
    // Only a loaded key is cached: a failure (locked keychain, denied prompt)
    // is retried on the next call so secrets recover once access is restored.
    static STORAGE_MASTER_KEY: OnceLock<[u8; KEY_LEN]> = OnceLock::new();
    if let Some(Ok(Some(key))) = MASTER_KEY.get() {
        return Ok(*key);
    }
    // `init_master_key` already tried the keychain this session and it
    // failed: reuse that outcome, do not prompt again. (`Ok(None)` alone also
    // means "backend needs no key / init skipped", which must still load.)
    if KEYCHAIN_UNAVAILABLE.get().is_some() {
        return Err("OS keychain master key unavailable this session".into());
    }
    if let Some(key) = STORAGE_MASTER_KEY.get() {
        return Ok(*key);
    }
    match try_load_master_key() {
        Ok((key, source)) => {
            log::info!("[keyring:storage] master key loaded from {source}");
            Ok(*STORAGE_MASTER_KEY.get_or_init(|| key))
        }
        Err(MasterKeyError::Configured(_) | MasterKeyError::Keychain(_)) => {
            // Fixed message: the underlying error can carry a path taken
            // from `MASTER_KEY_FILE_ENV`.
            log::error!("[keyring:storage] master key unavailable");
            Err("master key unavailable".into())
        }
    }
}

/// Runs `init` at most once per `cell` and reports its outcome on every call.
///
/// A configuration error is stored in the cell rather than leaving it empty
/// or storing `None`: `OnceLock::get_or_init` never reruns its closure, so a
/// later call (a second embedded boot in the same process) must still see the
/// error instead of a silent `Ok` with no key loaded.
fn init_once(
    cell: &OnceLock<MasterKeyInit>,
    init: impl FnOnce() -> MasterKeyInit,
) -> Result<(), String> {
    match cell.get_or_init(init) {
        Ok(_) => Ok(()),
        Err(e) => Err(e.clone()),
    }
}

/// Why the master key could not be loaded. The two kinds are handled
/// differently at startup — see [`init_master_key`].
#[derive(Debug)]
enum MasterKeyError {
    /// An operator-supplied source is set but unusable. Fatal at startup.
    Configured(String),
    /// The OS keychain could not provide (or safely mint) the key. Not fatal:
    /// the process runs with secrets inaccessible, as before.
    Keychain(String),
}

/// Abstraction over the OS-keychain entry that holds the master key.
///
/// Exists solely so the load-vs-mint decision in [`load_or_mint_master_key`]
/// can be unit-tested against injected `keyring::Error` variants. A real
/// `keyring::Entry` cannot be exercised non-interactively under `cargo test`
/// (the first access blocks on a GUI permission prompt), so the decision logic
/// is split out behind this trait and tested with a fake.
trait MasterKeyEntry {
    fn get_password(&self) -> Result<String, keyring::Error>;
    fn set_password(&self, value: &str) -> Result<(), keyring::Error>;
}

impl MasterKeyEntry for keyring::Entry {
    fn get_password(&self) -> Result<String, keyring::Error> {
        keyring::Entry::get_password(self)
    }
    fn set_password(&self, value: &str) -> Result<(), keyring::Error> {
        keyring::Entry::set_password(self, value)
    }
}

/// Loads the master key, returning it with a human-readable description of
/// the source it came from (for the startup log; never the value).
///
/// Uncached: every call reads the environment (and the key file) afresh.
/// Only [`init_master_key`] stores an outcome in [`MASTER_KEY`].
///
/// The environment is consulted first so a headless deployment never touches
/// the OS keychain. An environment variable that is set but unusable is an
/// error, not a fall-through: silently continuing to the keychain would mask
/// the misconfiguration and, in a container, fail later with a less specific
/// "master key unavailable".
fn try_load_master_key() -> Result<([u8; KEY_LEN], String), MasterKeyError> {
    let inline = env_value(MASTER_KEY_ENV, std::env::var(MASTER_KEY_ENV))
        .map_err(MasterKeyError::Configured)?;
    let file = env_value(MASTER_KEY_FILE_ENV, std::env::var(MASTER_KEY_FILE_ENV))
        .map_err(MasterKeyError::Configured)?;
    if let Some(from_env) =
        master_key_from_env(inline.as_deref(), file.as_deref(), read_master_key_file)
            .map_err(MasterKeyError::Configured)?
    {
        return Ok(from_env);
    }
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_MASTER_KEY_USERNAME)
        .map_err(|e| MasterKeyError::Keychain(format!("keychain entry creation failed: {e}")))?;
    load_or_mint_master_key(&entry)
        .map(|key| (key, "OS keychain".to_string()))
        .map_err(MasterKeyError::Keychain)
}

/// Interprets one `std::env::var` result for a master-key variable.
///
/// Only `NotPresent` means unset. A value that is not valid Unicode is a
/// misconfiguration and must not be treated as unset: that would fall
/// through to the OS keychain, where [`load_or_mint_master_key`] could mint
/// a different key and orphan every secret in `secrets.enc`. The rejected
/// value is never formatted into the error (`VarError`'s `Display` would
/// include it).
fn env_value(
    name: &str,
    raw: Result<String, std::env::VarError>,
) -> Result<Option<String>, String> {
    match raw {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err(format!("{name} is set but is not valid Unicode"))
        }
    }
}

/// Resolves an operator-supplied master key from the two environment
/// sources, given their raw values.
///
/// `Ok(None)` when neither is set (an empty or whitespace-only value counts
/// as unset, matching how Compose passes an undefined `${VAR}`), so the
/// caller falls through to the OS keychain. `Err` when a source is set but
/// unusable: both set at once, an unreadable file, or a value that is not
/// exactly `2 * KEY_LEN` hex characters. Error messages and the returned
/// source label name the variable and the problem but never include its
/// value — not even the file path, which may be a mistakenly pasted key.
///
/// `read_file` is injected so the decision logic is testable without touching
/// the filesystem; production passes [`read_master_key_file`].
fn master_key_from_env(
    inline: Option<&str>,
    file: Option<&str>,
    read_file: impl FnOnce(&Path) -> Result<String, String>,
) -> Result<Option<([u8; KEY_LEN], String)>, String> {
    let inline = inline.map(str::trim).filter(|value| !value.is_empty());
    let file = file.map(str::trim).filter(|value| !value.is_empty());
    match (inline, file) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(format!(
            "{MASTER_KEY_ENV} and {MASTER_KEY_FILE_ENV} are both set; set exactly one"
        )),
        (Some(hex), None) => parse_master_key_hex(hex)
            .map(|key| Some((key, MASTER_KEY_ENV.to_string())))
            .map_err(|e| format!("{MASTER_KEY_ENV}: {e}")),
        (None, Some(path)) => {
            // Named by the variable, never by its value: an operator who puts
            // the key itself in the `_FILE` variable would otherwise have it
            // copied into the error and the startup log.
            let contents =
                read_file(Path::new(path)).map_err(|e| format!("{MASTER_KEY_FILE_ENV}: {e}"))?;
            parse_master_key_hex(contents.trim())
                .map(|key| Some((key, MASTER_KEY_FILE_ENV.to_string())))
                .map_err(|e| format!("{MASTER_KEY_FILE_ENV}: {e}"))
        }
    }
}

/// Reads the file named by [`MASTER_KEY_FILE_ENV`].
///
/// On Unix a key file must not be writable by other users. Read-only group or
/// other permissions are supported for container secret mounts, where the
/// runtime may add group-read access for a non-root core.
fn read_master_key_file(path: &Path) -> Result<String, String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path).map_err(|e| {
            format!("cannot inspect master key file permissions ({MASTER_KEY_FILE_ENV}): {e}")
        })?;
        let mode = metadata.permissions().mode() & 0o777;
        if key_file_mode_is_other_writable(mode) {
            return Err(format!(
                "master key file ({MASTER_KEY_FILE_ENV}) is writable by other users; \
                 restrict it to a read-only secret mount"
            ));
        }
    }
    std::fs::read_to_string(path).map_err(|e| format!("cannot read master key file: {e}"))
}

#[cfg(unix)]
fn key_file_mode_is_other_writable(mode: u32) -> bool {
    mode & 0o002 != 0
}

/// Decodes a master key supplied as exactly `2 * KEY_LEN` hex characters.
/// The value never appears in the error.
fn parse_master_key_hex(hex: &str) -> Result<[u8; KEY_LEN], String> {
    let expected = 2 * KEY_LEN;
    let got = hex.chars().count();
    if got != expected {
        return Err(format!("expected {expected} hex characters, got {got}"));
    }
    // `hex_decode` slices by byte; a non-ASCII value of the right character
    // count would panic there instead of being rejected.
    if !hex.is_ascii() {
        return Err("value is not valid hex".to_string());
    }
    let bytes = crypto::hex_decode(hex).map_err(|_| "value is not valid hex".to_string())?;
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Load the existing master key, mint a fresh one, or fail safe.
///
/// **Only a genuine absence (`NoEntry`) may mint a new key.** Every other error
/// — access denied, keychain locked, platform failure — returns `Err` WITHOUT
/// minting or calling `set_password`, leaving the keychain entry untouched.
///
/// This is the fix for #3311. A macOS app update can change the binary's
/// code-signing identity (or the keychain item's ACL trust), so reading the
/// *existing* master key fails with an access error rather than `NoEntry`. The
/// previous code conflated the two and minted a brand-new key on access
/// denial, orphaning every secret encrypted under the old key — a silent
/// API-key wipe plus disconnected connectors, with no warning. Failing safe
/// keeps the ciphertext intact so it recovers on the next launch once keychain
/// access is restored. The catch-all `Err(e)` arm makes this independent of
/// which exact `keyring` error variant macOS returns on the denial.
fn load_or_mint_master_key<E: MasterKeyEntry>(entry: &E) -> Result<[u8; KEY_LEN], String> {
    match entry.get_password() {
        Ok(hex_str) => {
            let bytes = crypto::hex_decode(hex_str.trim())?;
            if bytes.len() != KEY_LEN {
                return Err(format!(
                    "master key has wrong length ({} bytes, expected {KEY_LEN})",
                    bytes.len()
                ));
            }
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            Ok(key)
        }
        Err(keyring::Error::NoEntry) => {
            let key_bytes = crypto::generate_random_bytes(KEY_LEN);
            let hex_value = crypto::hex_encode(&key_bytes);
            entry
                .set_password(&hex_value)
                .map_err(|e| format!("failed to store new master key in keychain: {e}"))?;

            let readback = entry
                .get_password()
                .map_err(|e| format!("master key readback failed: {e}"))?;
            if readback.trim() != hex_value {
                return Err("master key write verification failed".to_string());
            }

            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&key_bytes);
            log::info!(
                "[keyring:encrypted_file] no existing master key — generated and stored a new one"
            );
            Ok(key)
        }
        Err(e) => Err(format!(
            "OS keychain access unavailable; refusing to mint a replacement master key so \
             existing secrets are preserved (#3311): {e}"
        )),
    }
}

/// Get a reference to the cached master key, if available.
fn master_key() -> Option<&'static [u8; KEY_LEN]> {
    MASTER_KEY
        .get()
        .and_then(|init| init.as_ref().ok())
        .and_then(Option::as_ref)
}

pub(super) fn master_key_available() -> bool {
    master_key().is_some()
}

// ── Backend ──────────────────────────────────────────────────────────────────

/// Every secret in one ChaCha20-Poly1305 file: an adapter over
/// `tinystoragedrivers`' [`EncryptedFileSecrets`] (the `SecretStore` port),
/// keeping the file format (`secrets.enc`: one `nonce ‖ ciphertext ‖ tag` blob
/// over a JSON object of strings) and the `secrets.enc.lock` advisory lock
/// byte-for-byte, so a workspace written by either side reads on the other.
///
/// What stays here, because it is a desktop policy and not a storage format:
///
/// - the master key (env, else OS keychain; see [`init_master_key`]);
/// - the one-time import of a legacy plaintext `dev-keychain.json`;
/// - **corruption recovery**. The driver fails closed on a file that does not
///   decrypt or parse and leaves it untouched, which is right for a server
///   that has an operator. A desktop has none: failing closed would wedge
///   every `set` (so sign-in) forever. This adapter keeps today's behaviour
///   instead: log, move the bytes aside as `secrets.enc.corrupt.<ts>` (never
///   deleted, so the secrets stay recoverable with the right key) and carry
///   on with an empty store. See [`adapter::recover_corrupt_file`].
pub struct EncryptedFileBackend {
    path: PathBuf,
    workspace_dir: PathBuf,
}

impl EncryptedFileBackend {
    pub fn new(workspace_dir: &Path) -> Self {
        Self {
            path: workspace_dir.join(SECRETS_FILENAME),
            workspace_dir: workspace_dir.to_path_buf(),
        }
    }

    fn store(&self, key: &[u8; KEY_LEN]) -> EncryptedFileSecrets {
        EncryptedFileSecrets::at_path(self.path.clone(), Zeroizing::new(*key))
    }

    /// Run one driver call, recovering once from a corrupt file.
    fn run<T, F, Fut>(&self, key: &[u8; KEY_LEN], op: F) -> Result<T, KeyringError>
    where
        T: Send + 'static,
        F: Fn(EncryptedFileSecrets) -> Fut,
        Fut: std::future::Future<Output = tinystoragedrivers::Result<T>> + Send + 'static,
    {
        self.import_legacy_dev_keychain(key)?;
        match crate::storage::block_on(op(self.store(key))) {
            Err(error) if adapter::is_corruption(&error) => {
                adapter::recover_corrupt_file(&self.path, key, &error)?;
                crate::storage::block_on(op(self.store(key))).map_err(adapter::backend_error)
            }
            result => result.map_err(adapter::backend_error),
        }
    }

    /// Import `dev-keychain.json` into a missing `secrets.enc`, once.
    fn import_legacy_dev_keychain(&self, key: &[u8; KEY_LEN]) -> Result<(), KeyringError> {
        let legacy_path = self.workspace_dir.join(LEGACY_DEV_KEYCHAIN);
        let migrated_path = legacy_path.with_extension("json.migrated");
        if !legacy_path.exists() && !migrated_path.exists() {
            return Ok(());
        }
        // Use one lock order everywhere: encrypted destination, current legacy
        // source, then an older migrated source. The plaintext backend takes
        // the source lock, so its writers cannot race this read-and-remove.
        let _guard = file_store::lock_for_write(&self.path)?;
        let _legacy_guard = file_store::lock_for_write(&legacy_path)?;
        let _migrated_guard = file_store::lock_for_write(&migrated_path)?;
        if !legacy_path.exists() && !migrated_path.exists() {
            return Ok(());
        }

        // A legacy file can remain after a previous publication/cleanup error.
        // It must not make an otherwise valid encrypted store unavailable.
        if self.path.exists() {
            let existing_sources = [legacy_path.exists(), migrated_path.exists()];
            for (source, existed_before_cleanup) in [&legacy_path, &migrated_path]
                .into_iter()
                .zip(existing_sources)
            {
                if !existed_before_cleanup {
                    continue;
                }
                if let Err(error) =
                    self.remove_legacy_if_encrypted_copy_matches(key, source, &_guard)
                {
                    log::warn!("[keyring:encrypted_file] could not clean up legacy copy: {error}");
                }
            }
            return Ok(());
        }

        let source_path = if legacy_path.exists() {
            &legacy_path
        } else {
            &migrated_path
        };
        let metadata = std::fs::symlink_metadata(source_path).map_err(|source| {
            KeyringError::MigrationReadFailed {
                path: source_path.display().to_string(),
                source,
            }
        })?;
        if !metadata.file_type().is_file() {
            return Err(KeyringError::Backend(format!(
                "legacy {} is not a regular file; preserving it for recovery",
                source_path.display()
            )));
        }
        log::info!(
            "[keyring:encrypted_file] found legacy {} — migrating to encrypted file",
            source_path.display()
        );
        let bytes =
            std::fs::read(source_path).map_err(|source| KeyringError::MigrationReadFailed {
                path: source_path.display().to_string(),
                source,
            })?;
        let map: std::collections::BTreeMap<String, String> = if bytes.is_empty() {
            Default::default()
        } else {
            serde_json::from_slice(&bytes).map_err(|e| {
                KeyringError::Backend(format!(
                    "legacy {} is invalid JSON; preserving it for recovery: {e}",
                    source_path.display()
                ))
            })?
        };

        let json = Zeroizing::new(
            serde_json::to_vec(&map)
                .map_err(|e| KeyringError::Backend(format!("failed to serialize secrets: {e}")))?,
        );
        {
            let blob = crypto::chacha20_encrypt(key, &json)
                .map_err(|e| KeyringError::Backend(format!("encryption failed: {e}")))?;
            file_store::write_atomic(&self.path, &blob)?;
        }

        let verified = match self.remove_legacy_if_encrypted_copy_matches(key, source_path, &_guard)
        {
            Ok(verified) => verified,
            Err(error @ KeyringError::MigrationDeleteFailed { .. }) => {
                log::warn!("[keyring:encrypted_file] could not remove migrated plaintext: {error}");
                true
            }
            Err(error) => return Err(error),
        };
        if !verified {
            return Err(KeyringError::Backend(
                "encrypted migration result did not verify against its plaintext source".into(),
            ));
        }
        log::info!(
            "[keyring:encrypted_file] legacy {} migrated \
             ({} entries), verified, and durably published",
            source_path.display(),
            map.len()
        );
        Ok(())
    }

    /// Remove the plaintext source only after the published encrypted copy is
    /// durable and decrypts to a map containing every source entry unchanged.
    /// The caller holds the destination and source locks through verification
    /// and deletion, preventing a concurrent encrypted write from invalidating
    /// the snapshot between comparison and cleanup.
    fn remove_legacy_if_encrypted_copy_matches(
        &self,
        key: &[u8; KEY_LEN],
        legacy_path: &Path,
        destination_lock: &file_store::WriteLock,
    ) -> Result<bool, KeyringError> {
        self.remove_legacy_if_encrypted_copy_matches_with_sync(
            key,
            legacy_path,
            destination_lock,
            file_store::sync_parent_dir,
        )
    }

    fn remove_legacy_if_encrypted_copy_matches_with_sync(
        &self,
        key: &[u8; KEY_LEN],
        legacy_path: &Path,
        destination_lock: &file_store::WriteLock,
        sync_parent: impl FnOnce(&Path, &file_store::WriteLock) -> Result<(), KeyringError>,
    ) -> Result<bool, KeyringError> {
        let metadata = match std::fs::symlink_metadata(legacy_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => {
                return Err(KeyringError::MigrationReadFailed {
                    path: legacy_path.display().to_string(),
                    source,
                });
            }
        };
        if !metadata.file_type().is_file() {
            return Ok(false);
        }
        let bytes =
            std::fs::read(legacy_path).map_err(|source| KeyringError::MigrationReadFailed {
                path: legacy_path.display().to_string(),
                source,
            })?;
        let legacy: std::collections::BTreeMap<String, String> = if bytes.is_empty() {
            Default::default()
        } else {
            match serde_json::from_slice(&bytes) {
                Ok(legacy) => legacy,
                Err(_) => return Ok(false),
            }
        };
        let blob = match std::fs::read(&self.path) {
            Ok(blob) => blob,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(source) => {
                return Err(KeyringError::MigrationReadFailed {
                    path: self.path.display().to_string(),
                    source,
                });
            }
        };
        let plaintext = match crypto::chacha20_decrypt(key, &blob) {
            Ok(plaintext) => plaintext,
            Err(_) => return Ok(false),
        };
        let encrypted: std::collections::HashMap<String, String> =
            match serde_json::from_slice(&plaintext) {
                Ok(encrypted) => encrypted,
                Err(_) => return Ok(false),
            };
        if !legacy
            .iter()
            .all(|(name, value)| encrypted.get(name) == Some(value))
        {
            return Ok(false);
        }

        sync_parent(&self.path, destination_lock)?;
        let current_legacy = self.workspace_dir.join(LEGACY_DEV_KEYCHAIN);
        let archive_path = current_legacy.with_extension("json.migrated");
        let cleanup = if legacy_path == current_legacy && !archive_path.exists() {
            std::fs::rename(legacy_path, &archive_path)
        } else {
            std::fs::remove_file(legacy_path)
        };
        match cleanup {
            Ok(()) => {
                // Persist the source rename/removal too. If this sync fails,
                // the encrypted copy is already durable and the archive (or
                // original source) remains available for another cleanup pass.
                file_store::sync_parent_dir(&self.path, destination_lock)?;
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(KeyringError::MigrationDeleteFailed {
                path: legacy_path.display().to_string(),
                source,
            }),
        }
    }

    /// [`KeyringBackend::get`] under an explicit master key.
    pub(super) fn get_with_key(
        &self,
        key: &[u8; KEY_LEN],
        namespaced_key: &str,
    ) -> Result<Option<String>, KeyringError> {
        use tinystoragedrivers::secrets::SecretStore as _;
        let name = namespaced_key.to_string();
        let value = self.run(key, move |store| {
            let name = name.clone();
            async move { store.get(&name).await }
        })?;
        adapter::utf8(namespaced_key, value)
    }

    /// [`KeyringBackend::set`] under an explicit master key.
    pub(super) fn set_with_key(
        &self,
        key: &[u8; KEY_LEN],
        namespaced_key: &str,
        value: &str,
    ) -> Result<(), KeyringError> {
        use tinystoragedrivers::secrets::SecretStore as _;
        let name = namespaced_key.to_string();
        let value = Zeroizing::new(value.as_bytes().to_vec());
        self.run(key, move |store| {
            let (name, value) = (name.clone(), value.clone());
            async move { store.set(&name, &value).await }
        })
    }
}

impl KeyringBackend for EncryptedFileBackend {
    fn get(&self, namespaced_key: &str) -> Result<Option<String>, KeyringError> {
        let Some(key) = master_key() else {
            return Ok(None);
        };
        self.get_with_key(key, namespaced_key)
    }

    fn set(&self, namespaced_key: &str, value: &str) -> Result<(), KeyringError> {
        let Some(key) = master_key() else {
            return Err(KeyringError::Backend(
                "master key unavailable — cannot store secrets".to_string(),
            ));
        };
        self.set_with_key(key, namespaced_key, value)
    }

    fn delete(&self, namespaced_key: &str) -> Result<(), KeyringError> {
        use tinystoragedrivers::secrets::SecretStore as _;
        let Some(key) = master_key() else {
            return Ok(());
        };
        let name = namespaced_key.to_string();
        self.run(key, move |store| {
            let name = name.clone();
            async move { store.delete(&name).await.map(|_| ()) }
        })
    }

    fn name(&self) -> &'static str {
        "encrypted_file"
    }
}

#[cfg(test)]
#[path = "encrypted_file_backend_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "encrypted_file_backend_fixture_tests.rs"]
mod fixture_tests;
