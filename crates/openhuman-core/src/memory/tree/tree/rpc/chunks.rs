//! Chunk read handlers: `memory_tree_list_chunks` and `memory_tree_get_chunk`.

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::memory::api::provider::ChunkQuery;
use crate::rpc::RpcOutcome;
use tinymemory_api::chunks::{Chunk, SourceKind};

/// Query shape for the `list_chunks` RPC.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListChunksRequest {
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub since_ms: Option<i64>,
    #[serde(default)]
    pub until_ms: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Response shape for the `list_chunks` RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListChunksResponse {
    pub chunks: Vec<Chunk>,
}

/// `list_chunks` RPC handler. Filters and returns persisted chunks ordered by
/// timestamp DESC.
///
/// `scope` is `None`, which is not an oversight: this listing has never
/// applied the per-turn source allowlist — the engine query it replaced set
/// `source_scope: None` — and it is reached by inspection surfaces rather than
/// by an agent turn. Narrowing it here would be a policy change wearing a
/// routing change's clothes.
pub async fn list_chunks_rpc(
    config: &Config,
    req: ListChunksRequest,
) -> Result<RpcOutcome<ListChunksResponse>, String> {
    // Parsed before the driver is resolved so an unknown kind stays a caller
    // error naming the offending value, rather than a driver round trip that
    // returns nothing and looks like an empty store.
    let query = ChunkQuery {
        source_kind: match req.source_kind.as_deref() {
            None => None,
            Some(s) => Some(SourceKind::parse(s)?),
        },
        source_id: req.source_id,
        owner: req.owner,
        since_ms: req.since_ms,
        until_ms: req.until_ms,
        limit: req.limit,
        offset: None,
        exclude_dropped: false,
        // The filtered-listing predicates this request does not carry. An empty
        // predicate is unfiltered, so the defaults leave the query exactly as
        // narrow as the fields above already make it.
        ..Default::default()
    };

    // No `spawn_blocking`: the driver owns whether its own reads block, and the
    // module's do not run on this thread at all.
    let binding = crate::memory::binding::for_config(config)?;
    let rows = match binding.provider().as_chunks() {
        Some(chunks) => chunks
            .list_chunks(&query, None)
            .await
            .map_err(|e| format!("list_chunks: {e}"))?,
        // Read-only, so an empty page is the honest answer: a driver with no
        // chunk tier holds no rows to list, which is a true statement about it
        // rather than a fault the caller can act on.
        None => {
            log::debug!(
                "[memory-tree][rpc] list_chunks: driver '{}' does not serve Chunks; reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };

    let n = rows.len();
    Ok(RpcOutcome::single_log(
        ListChunksResponse { chunks: rows },
        format!("memory_tree: list_chunks n={n}"),
    ))
}

/// Request shape for the `get_chunk` RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetChunkRequest {
    pub id: String,
}

/// Response shape for the `get_chunk` RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetChunkResponse {
    pub chunk: Option<Chunk>,
}

/// `get_chunk` RPC handler. Returns the chunk identified by `id`, or `None`.
pub async fn get_chunk_rpc(
    config: &Config,
    req: GetChunkRequest,
) -> Result<RpcOutcome<GetChunkResponse>, String> {
    let binding = crate::memory::binding::for_config(config)?;
    let chunk = match binding.provider().as_chunks() {
        Some(chunks) => chunks
            .get_chunk(&req.id)
            .await
            .map_err(|e| format!("get_chunk: {e}"))?,
        // `None` is already this handler's answer for an id the store does not
        // hold, and a driver with no chunk tier holds none — so the degrade is
        // indistinguishable from the ordinary miss, which is what makes it safe
        // here and not on a write.
        None => {
            log::debug!(
                "[memory-tree][rpc] get_chunk: driver '{}' does not serve Chunks; reporting none",
                binding.driver_id()
            );
            None
        }
    };
    Ok(RpcOutcome::single_log(
        GetChunkResponse { chunk },
        format!("memory_tree: get_chunk id={}", req.id),
    ))
}
