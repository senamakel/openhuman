//! Chunk-staging fixtures for the retrieval handler tests.
//!
//! One reason this is a directory of its own, and it is a classification rather
//! than a hiding place — the same reason
//! [`memory::tool_memory::test_support`](crate::openhuman::memory::tool_memory)
//! exists.
//!
//! `rpc.rs`'s inline `#[cfg(test)]` module needs chunk rows the **real**
//! in-process driver can read back, because one of its tests proves the source
//! gate end to end rather than against a recording double. Writing those rows
//! means the engine's chunk store: `MemoryChunks` on the contract is a read
//! family (`list_chunks` / `get_chunk` / `chunk_detail` / `storage_kinds` /
//! `chunk_embeddings`) with no write or transaction door, and none should be
//! added — `with_connection` hands out a `rusqlite` handle, which no
//! engine-neutral contract can promise.
//!
//! So the reference is real and unavoidable, and it is also **test-only**:
//! `cfg(test)` code links the `tinymemory-core` **dev-dependency**, which
//! survives #5560's shed and puts no byte of the engine in the shipped binary.
//!
//! `memory::direct_engine_refs_tests` could not see that on its own. Its
//! scanner reads line by line and skips only comments and whole files —
//! deliberately, because brace-tracking inline `#[cfg(test)]` blocks is
//! complexity its docs say it does not want — so a fixture write kept `rpc.rs`
//! on the direct-reference allowlist under a `NeedsWiderSeam` verdict that
//! described a *production* gap the file does not have. That made "deliberate"
//! and "not migrated yet" look identical in the one list that exists to tell
//! them apart.
//!
//! `test_support/` is the escape hatch both memory lints already honour by path
//! (each matches on a `test_support` path *component*, which is why this is a
//! directory and not a `test_support.rs`). Nothing about the writes changed;
//! only where the line that names the engine lives.

use tinymemory_api::chunks::Chunk;

use crate::openhuman::config::Config;

pub(crate) use tinymemory_core::store::chunks::store::upsert_chunks;

/// Write chunks the in-process driver can read back — both the row and the
/// staged content body, since a hit carries the body.
pub(crate) fn stage_test_chunks(cfg: &Config, chunks: &[Chunk]) {
    let content_root = cfg.memory_tree_content_root();
    std::fs::create_dir_all(&content_root).expect("create content_root for test");
    let staged = tinymemory_core::store::content::stage_chunks(&content_root, chunks)
        .expect("stage_chunks for test chunks");
    log::debug!(
        "[memory-tree][retrieval][test-support] staging chunks count={} content_root={}",
        chunks.len(),
        content_root.display()
    );
    tinymemory_core::store::chunks::store::with_connection(cfg, |conn| {
        let tx = conn.unchecked_transaction()?;
        tinymemory_core::store::chunks::store::upsert_staged_chunks_tx(&tx, &staged)?;
        tx.commit()?;
        Ok(())
    })
    .expect("persist staged chunk pointers");
}
