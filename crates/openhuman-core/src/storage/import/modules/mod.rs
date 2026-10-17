//! Host-owned one-time imports from module legacy files into storage.
//!
//! Each import reads the old module-owned files before the module starts using
//! its storage contract. Importers stay here because migration and the marker
//! policy belong to the host; after every supported upgrade has crossed the
//! storage-contract release, the corresponding importer and tests can be
//! removed. The current removal floors are tinymcp/tinyconnectors 0.18,
//! tinywallet 0.22, tinybox 0.1.29, and tinyjuice 0.10.

#[cfg(feature = "modules")]
mod support;
#[cfg(feature = "modules")]
mod tinymcp;
#[cfg(feature = "modules")]
mod tinyconnectors;
#[cfg(feature = "modules")]
mod tinybox;
#[cfg(feature = "modules")]
mod tinyjuice;
#[cfg(all(feature = "modules", feature = "web3"))]
mod tinywallet;
#[cfg(all(feature = "channels", feature = "modules"))]
mod delivery;

/// Imports tinymcp's legacy SQLite files under `data_dir` into the module's
/// storage, once per directory. Never fails the caller: an error is logged and
/// the import is retried at the next open.
pub async fn import_legacy_mcp(
    ctx: &crate::core::runtime::DispatchCtx,
    data_dir: &std::path::Path,
) {
    #[cfg(feature = "modules")]
    import_legacy_mcp_with(
        ctx,
        data_dir,
        tinymcp::import_legacy(ctx, data_dir),
    )
    .await;
    #[cfg(not(feature = "modules"))]
    let _ = (ctx, data_dir);
}

#[cfg(feature = "modules")]
async fn import_legacy_mcp_with<F, T>(
    ctx: &crate::core::runtime::DispatchCtx,
    data_dir: &std::path::Path,
    run: F,
) where
    F: std::future::Future<Output = anyhow::Result<T>>,
    T: std::fmt::Debug + Settled,
{
    import_once(ctx, "mcp", "tinymcp", data_dir, run).await;
}

/// What an importer's outcome says about whether it may be skipped next time.
#[cfg(feature = "modules")]
trait Settled {
    /// True once the import finished or its marker was found; false while the
    /// outcome (no backend, no source, unsupported backend) can still change.
    fn settled(&self) -> bool;
}

#[cfg(feature = "modules")]
macro_rules! settled_outcome {
    ($($module:ident),*) => {$(
        impl Settled for $module::Outcome {
            fn settled(&self) -> bool {
                matches!(self, Self::AlreadyDone | Self::Imported { .. })
            }
        }
    )*};
}

#[cfg(feature = "modules")]
settled_outcome!(
    tinymcp,
    tinyconnectors,
    tinybox,
    tinyjuice
);
#[cfg(all(feature = "modules", feature = "web3"))]
settled_outcome!(tinywallet);

/// The (importer, module scope, directory) triples this process already
/// imported, so a hot call path pays for the marker lookup once rather than
/// on every call. The async lock is held across an import, so a concurrent
/// first caller waits for the migration instead of racing it.
#[cfg(feature = "modules")]
type ImportedSet =
    tokio::sync::Mutex<std::collections::HashSet<(&'static str, String, std::path::PathBuf)>>;

#[cfg(feature = "modules")]
fn imported_once() -> &'static ImportedSet {
    static DONE: std::sync::OnceLock<ImportedSet> = std::sync::OnceLock::new();
    DONE.get_or_init(Default::default)
}

/// Runs one legacy import at most once per process, module scope and
/// directory, and again after a failure or an unsettled outcome (which is
/// logged, never returned). Returns false only when the import failed. The key is recorded only after the import
/// finishes, so a cancelled caller leaves nothing behind.
#[cfg(feature = "modules")]
async fn import_once<F, T>(
    ctx: &crate::core::runtime::DispatchCtx,
    kind: &'static str,
    module_id: &str,
    dir: &std::path::Path,
    run: F,
) -> bool
where
    F: std::future::Future<Output = anyhow::Result<T>>,
    T: std::fmt::Debug + Settled,
{
    let scope = match storage_host::current_module_scope(ctx, module_id) {
        Ok(scope) => format!("{scope:?}"),
        Err(error) => {
            log::warn!("[{kind}] legacy storage import skipped: {:?}", error.kind());
            return true;
        }
    };
    let key = (kind, scope, dir.to_path_buf());
    let mut done = imported_once().lock().await;
    if done.contains(&key) {
        return true;
    }
    match run.await {
        Ok(outcome) => {
            log::debug!("[{kind}] legacy storage import: {outcome:?}");
            if outcome.settled() {
                done.insert(key);
            }
            true
        }
        Err(error) => {
            log::warn!("[{kind}] legacy storage import failed, will retry: {error:#}");
            false
        }
    }
}

/// Imports the connector module's legacy preference and trigger-archive files
/// under `state_dir` into its storage, once per directory. Never fails the
/// caller: an error is logged and the import is retried at the next call.
pub async fn import_legacy_connectors(
    ctx: &crate::core::runtime::DispatchCtx,
    state_dir: &std::path::Path,
) {
    #[cfg(feature = "modules")]
    import_once(
        ctx,
        "connectors",
        "tinyconnectors",
        state_dir,
        tinyconnectors::import_legacy(ctx, state_dir),
    )
    .await;
    #[cfg(not(feature = "modules"))]
    let _ = (ctx, state_dir);
}

/// Imports the x402 ledger's legacy `payments.jsonl` under `workspace_dir`
/// into the wallet module's storage, once per workspace. Never fails the
/// caller: an error is logged and the import is retried at the next call. Returns
/// false when the import failed, so the caller can refuse to open a ledger that
/// would ignore the legacy payments.
pub async fn import_legacy_x402(
    ctx: &crate::core::runtime::DispatchCtx,
    workspace_dir: &std::path::Path,
) -> bool {
    #[cfg(all(feature = "modules", feature = "web3"))]
    return import_once(
        ctx,
        "x402",
        "tinywallet",
        workspace_dir,
        tinywallet::import_legacy(ctx, workspace_dir),
    )
    .await;
    #[cfg(not(all(feature = "modules", feature = "web3")))]
    {
        let _ = (ctx, workspace_dir);
        true
    }
}

/// Imports the legacy jail registry index under `<workspace_dir>/jails` into
/// the tinybox module's storage, once. Never fails the caller: an error is
/// logged and the import is retried at the next call.
pub async fn import_legacy_tinybox(
    ctx: &crate::core::runtime::DispatchCtx,
    workspace_dir: &std::path::Path,
) {
    #[cfg(feature = "modules")]
    import_once(
        ctx,
        "tinybox",
        "tinybox",
        workspace_dir,
        tinybox::import_legacy(ctx, workspace_dir),
    )
    .await;
    #[cfg(not(feature = "modules"))]
    let _ = (ctx, workspace_dir);
}

/// Scopes, under the current tenant, that hold expiring blobs written by
/// modules (today TinyJuice's CCR originals), for the blob sweeper. A module
/// whose scope cannot be resolved here is skipped. Without the `modules`
/// feature no module writes blobs, so there is nothing to sweep.
#[cfg(feature = "modules")]
pub(crate) fn expiring_blob_module_scopes(
    ctx: &crate::core::runtime::DispatchCtx,
) -> Vec<crate::storage::Scope> {
    ["tinyjuice"]
        .into_iter()
        .filter_map(|id| storage_host::current_module_scope(ctx, id).ok())
        .collect()
}

/// [`expiring_blob_module_scopes`] for SaaS profile `profile`, whose acting
/// scope is `base`, from outside that profile's task scope.
#[cfg(feature = "modules")]
pub(crate) fn expiring_blob_module_scopes_for(
    profile: &str,
    base: &crate::storage::Scope,
) -> Vec<crate::storage::Scope> {
    ["tinyjuice"]
        .into_iter()
        .filter_map(|id| storage_host::module_scope(Some(profile), base, id).ok())
        .collect()
}

/// See the `modules` build: with the feature off no module writes blobs.
#[cfg(not(feature = "modules"))]
pub(crate) fn expiring_blob_module_scopes_for(
    _profile: &str,
    _base: &crate::storage::Scope,
) -> Vec<crate::storage::Scope> {
    Vec::new()
}

/// See the `modules` build: with the feature off no module writes blobs.
#[cfg(not(feature = "modules"))]
pub(crate) fn expiring_blob_module_scopes(
    _ctx: &crate::core::runtime::DispatchCtx,
) -> Vec<crate::storage::Scope> {
    Vec::new()
}

/// Imports the legacy on-disk CCR tier under `root` into the tinyjuice
/// module's blob storage, once per directory. `ttl` is the configured original
/// lifetime. Never fails the caller: an error is logged and the import is
/// retried at the next install.
pub async fn import_legacy_tinyjuice(
    ctx: &crate::core::runtime::DispatchCtx,
    root: &std::path::Path,
    ttl: Option<std::time::Duration>,
) {
    #[cfg(feature = "modules")]
    import_once(
        ctx,
        "tinyjuice",
        "tinyjuice",
        root,
        tinyjuice::import_legacy(ctx, root, ttl),
    )
    .await;
    #[cfg(not(feature = "modules"))]
    let _ = (ctx, root, ttl);
}

#[cfg(all(test, feature = "modules"))]
#[path = "import_once_tests.rs"]
mod import_once_tests;
