//! Shared test infrastructure for `memory::ops` submodule tests.
//!
//! All `ops` submodules that need a global `MemoryClient` call
//! [`ensure_shared_memory_client`] instead of creating their own
//! `OnceLock<PathBuf>`.  Sharing one leaked workspace means concurrent
//! `global::init()` calls always resolve to the same path and hit the
//! no-op fast-path inside `init_in_slot`, preventing one test thread
//! from silently rebinding the global under another thread's feet.
//!
//! # This is the one engine reference in `memory::ops` that is meant to stay
//!
//! Everything else under `ops/` was routed onto the contract for openhuman#5560
//! so `tinymemory-core` can leave `[dependencies]` and survive as a
//! **dev-dependency only**. This fixture deliberately still boots the
//! in-process engine, because that is what it is for: it hands the `ops` tests a
//! real store to write rows into and read back, and a dev-dependency reference
//! from `#[cfg(test)]` code is not linked into the shipped binary.
//!
//! It lives in a `test_support/` **directory** rather than as a
//! `test_support.rs` file for one reason: both memory ratchets skip by path, and
//! `is_test_path` matches a *path component* named `test_support`, not a file
//! stem. As a flat file this module had to carry an entry in
//! `direct_engine_refs_tests::ALLOWED` that read like an unmigrated production
//! call site. The same reasoning already put `memory::test_support` in a
//! directory — see its module docs.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Return the process-global workspace used by memory tests without starting a
/// client.
///
/// Binding tests use this narrower helper because constructing a module-backed
/// provider is intentionally synchronous and lazy. The live client starts a
/// Tokio ingestion worker, so initializing it here would make a mere bind
/// depend on whichever test happened to install a reactor first.
pub(crate) fn shared_memory_test_workspace() -> PathBuf {
    static WORKSPACE: OnceLock<PathBuf> = OnceLock::new();
    WORKSPACE
        .get_or_init(|| {
            let tmp = tempfile::TempDir::new().expect("tempdir");
            let path = tmp.path().join("workspace");
            std::fs::create_dir_all(&path).expect("workspace dir");
            std::mem::forget(tmp);
            path
        })
        .clone()
}

/// Binds the process-global memory client to a single shared temp workspace and
/// returns that workspace path.
///
/// Safe to call from multiple test threads concurrently — subsequent calls with
/// the same workspace path return the existing client without rebinding.
///
/// The returned path lets callers whose RPC path *also* resolves the workspace
/// from `OPENHUMAN_WORKSPACE` (notably `memory::ops::documents` via
/// `memory_init` → `current_workspace_dir`) pin the env var to this same path so
/// the env and the bound client agree. See `documents::tests`.
pub(crate) fn ensure_shared_memory_client() -> PathBuf {
    // Building a client reaches the embedding seam, which fails loudly when
    // unwired. Before the extraction these were direct calls and needed no
    // setup; now they need the host impls installed.
    crate::openhuman::memory::host_impls::install_for_tests();
    let workspace = shared_memory_test_workspace();
    tinymemory_core::global::init(workspace.clone()).expect("initialize shared test memory client");
    workspace
}
