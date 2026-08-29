use anyhow::Result;

use crate::openhuman::config::Config;
use crate::openhuman::memory::binding::MemoryBinding;
use crate::openhuman::memory::tree::score::store as score_store;
use crate::rpc::RpcOutcome;
use tinymemory_api::error::MemoryError;
use tinymemory_api::provider::types::{EntityOccurrence, ForgetSelector};

use super::types::{DeleteChunkResponse, EntityRef, ScoreBreakdown, ScoreSignal, MAX_LIST_LIMIT};

// ── entity index lookups ────────────────────────────────────────────────
//
// All three read the driver's entity index through `MemoryEntities`, and each
// uses the member written for the *browser's* reading of that index rather than
// the agent's: `top_entities` for what the store holds at all, `chunk_entities`
// for what one chunk is about, `entity_chunk_ids` for the content behind one
// entity. `MemoryEntities::entities` is deliberately not the member for any of
// them — it is namespace-scoped where these are store-wide, hotness-ranked
// where these rank by observation count, and its `EntityHit` carries a
// canonical name where these carry the index's `surface` sample.
//
// None of the three needs `spawn_blocking`: the driver owns whether its own
// reads block, and the module's do not run on this thread at all.

/// One occurrence row in the shape this RPC surface has always returned.
///
/// Field for field, including the one rename: the index's `mentions` is the
/// `COUNT(*)` this handler's `count` has always carried.
fn entity_ref(occurrence: EntityOccurrence) -> EntityRef {
    EntityRef {
        entity_id: occurrence.entity_id,
        kind: occurrence.kind,
        surface: occurrence.surface,
        count: occurrence.mentions,
    }
}

/// Every entity indexed against one chunk, most-observed first.
///
/// Shared by [`entity_index_for_rpc`], which reports the rows, and
/// [`delete_chunk_rpc`], which counts the ones a delete is about to take with
/// it. `op` is the caller's name, so an error still reads as that handler's.
///
/// The batch is one id long on purpose. `chunk_entities` names the chunk on
/// every row precisely because a wider batch answers as one flat list and
/// grouping becomes the caller's job — with a single id every row is this
/// chunk's by construction, so there is nothing to group and nothing that could
/// be mis-indexed by position.
///
/// A driver with no entity tier reports no rows rather than failing: both
/// callers are describing what the index holds, and it holds nothing.
async fn chunk_entity_rows(
    binding: &MemoryBinding,
    chunk_id: &str,
    op: &str,
) -> Result<Vec<EntityOccurrence>, String> {
    let Some(entities) = binding.provider().as_entities() else {
        log::debug!(
            "[memory_tree::read::entity_index] {op}: driver '{}' does not serve Entities; \
             reporting empty",
            binding.driver_id()
        );
        return Ok(Vec::new());
    };
    let ids = [chunk_id.to_string()];
    let rows = entities
        .chunk_entities(&ids, None)
        .await
        .map_err(|e| format!("{op}: {e}"))?;
    log::debug!(
        "[memory_tree::read::entity_index] {op}: driver '{}' rows={} chunk_id={chunk_id}",
        binding.driver_id(),
        rows.len()
    );
    Ok(rows.into_iter().map(|row| row.occurrence).collect())
}

pub async fn entity_index_for_rpc(
    config: &Config,
    chunk_id: String,
) -> Result<RpcOutcome<Vec<EntityRef>>, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let refs: Vec<EntityRef> = chunk_entity_rows(&binding, &chunk_id, "entity_index_for")
        .await?
        .into_iter()
        .map(entity_ref)
        .collect();

    let n = refs.len();
    Ok(RpcOutcome::single_log(
        refs,
        format!("memory_tree::read: entity_index_for chunk_id={chunk_id} n={n}"),
    ))
}

pub async fn chunks_for_entity_rpc(
    config: &Config,
    entity_id: String,
) -> Result<RpcOutcome<Vec<String>>, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let chunk_ids = match binding.provider().as_entities() {
        // A bound appears where the SQL had none, because `entity_chunk_ids`
        // requires one and the contract gives a driver no way to say
        // "unbounded". That is the right shape for a member reached over a bus:
        // the previous query would happily stream every chunk an entity was
        // ever seen in, as one RPC response, over a wire the caller cannot
        // interrupt.
        //
        // `MAX_LIST_LIMIT` is the cap this module already applies to every
        // other list it serves, so the ceiling is the surface's own rather than
        // a number invented here. It is a ceiling and not a page: this handler
        // has never taken a `limit` and its response is a bare `Vec<String>`
        // with nowhere to report truncation, so widening the wire to say
        // "there were more" is a change this migration has no mandate to make.
        // An entity observed in more than a thousand chunks is a graph query,
        // not a list.
        Some(entities) => entities
            .entity_chunk_ids(&entity_id, MAX_LIST_LIMIT as usize)
            .await
            .map_err(|e| format!("chunks_for_entity: {e}"))?,
        // Read-only, so an empty list is the honest answer: a driver with no
        // entity tier indexed this entity nowhere, which is a true statement
        // about it rather than a fault the caller can act on.
        None => {
            log::debug!(
                "[memory_tree::read::chunks_for_entity] driver '{}' does not serve Entities; \
                 reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };

    let n = chunk_ids.len();
    log::debug!(
        "[memory_tree::read::chunks_for_entity] driver '{}' entity_id={entity_id} n={n} limit={}",
        binding.driver_id(),
        MAX_LIST_LIMIT
    );
    Ok(RpcOutcome::single_log(
        chunk_ids,
        format!("memory_tree::read: chunks_for_entity entity_id={entity_id} n={n}"),
    ))
}

pub async fn top_entities_rpc(
    config: &Config,
    kind: Option<String>,
    limit: u32,
) -> Result<RpcOutcome<Vec<EntityRef>>, String> {
    let limit = limit.clamp(1, MAX_LIST_LIMIT);
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let refs: Vec<EntityRef> = match binding.provider().as_entities() {
        Some(entities) => {
            let answered = entities.top_entities(kind.as_deref(), limit as usize).await;
            match answered {
                Ok(rows) => rows.into_iter().map(entity_ref).collect(),
                // The one behaviour delta this migration was warned about, and
                // it is decided in favour of the wire this handler already has.
                // The member validates `kind` and answers `Invalid` for one it
                // does not recognise; the SQL it replaces compared the string
                // against the stored column, so an unknown kind matched no rows
                // and the handler returned an empty list. A migration that
                // turns a quiet empty result into a user-visible error is
                // changing the product, not moving a call, so the variant is
                // mapped back — narrowly, and never silently.
                //
                // Narrowly means two things. Only `Invalid` degrades, so a
                // backend failure still propagates. And only when a `kind` was
                // actually supplied: `Invalid` with no filter to be invalid
                // about is a driver fault, and swallowing that is exactly what
                // this map-back is careful not to do.
                Err(MemoryError::Invalid(reason)) if kind.is_some() => {
                    log::debug!(
                        "[memory_tree::read::top_entities] driver '{}' rejected kind={:?} \
                         ({reason}); reporting empty, which is what the SQL this replaced returned",
                        binding.driver_id(),
                        kind
                    );
                    Vec::new()
                }
                Err(e) => return Err(format!("top_entities: {e}")),
            }
        }
        // Read-only, so an empty ranking is the honest answer: a driver with no
        // entity tier has nothing indexed to rank.
        None => {
            log::debug!(
                "[memory_tree::read::top_entities] driver '{}' does not serve Entities; \
                 reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };

    let n = refs.len();
    log::debug!(
        "[memory_tree::read::top_entities] driver '{}' kind={:?} limit={limit} n={n}",
        binding.driver_id(),
        kind
    );
    Ok(RpcOutcome::single_log(
        refs,
        format!("memory_tree::read: top_entities n={n}"),
    ))
}

// ── chunk_score ─────────────────────────────────────────────────────────
//
// Still the engine's scorer, and deliberately so: `chunk_score` and
// `DEFAULT_DROP_THRESHOLD` are engine-internal ranking that the contract
// excludes by design, so there is no member to route this onto (#5560).

pub async fn chunk_score_rpc(
    config: &Config,
    chunk_id: String,
) -> Result<RpcOutcome<Option<ScoreBreakdown>>, String> {
    let cfg = config.clone();
    let id = chunk_id.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Option<ScoreBreakdown>> {
        let row = score_store::get_score(&cfg, &id)?;
        Ok(row.map(|r| {
            let llm_consulted = r.signals.llm_importance > 0.0;
            let signals = vec![
                ScoreSignal {
                    name: "token_count".into(),
                    weight: 1.0,
                    value: r.signals.token_count,
                },
                ScoreSignal {
                    name: "unique_words".into(),
                    weight: 1.0,
                    value: r.signals.unique_words,
                },
                ScoreSignal {
                    name: "metadata_weight".into(),
                    weight: 1.5,
                    value: r.signals.metadata_weight,
                },
                ScoreSignal {
                    name: "source_weight".into(),
                    weight: 1.5,
                    value: r.signals.source_weight,
                },
                ScoreSignal {
                    name: "interaction".into(),
                    weight: 3.0,
                    value: r.signals.interaction,
                },
                ScoreSignal {
                    name: "entity_density".into(),
                    weight: 1.0,
                    value: r.signals.entity_density,
                },
                ScoreSignal {
                    name: "llm_importance".into(),
                    weight: if llm_consulted { 2.0 } else { 0.0 },
                    value: r.signals.llm_importance,
                },
            ];
            ScoreBreakdown {
                signals,
                total: r.total,
                threshold: crate::openhuman::memory::tree::score::DEFAULT_DROP_THRESHOLD,
                kept: !r.dropped,
                llm_consulted,
            }
        }))
    })
    .await
    .map_err(|e| format!("chunk_score join error: {e}"))?
    .map_err(|e| format!("chunk_score: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        result,
        format!("memory_tree::read: chunk_score id={chunk_id}"),
    ))
}

// ── delete_chunk ────────────────────────────────────────────────────────

/// Whether `mem_tree_score` holds a row for this chunk, as a rowcount.
///
/// The table is keyed by chunk id, so "is there a row" *is* the number the
/// `DELETE` this replaced returned. It is read through the same
/// `tree::score::store` door [`chunk_score_rpc`] above already uses — the
/// scorer is engine-internal ranking the contract excludes by design, so there
/// is no member to reach for and no second door being opened here.
///
/// `spawn_blocking` stays because this one genuinely still blocks: it is a
/// synchronous SQLite read on the calling thread, unlike the driver calls
/// around it.
async fn score_row_count(config: &Config, chunk_id: &str) -> Result<u32, String> {
    let cfg = config.clone();
    let id = chunk_id.to_string();
    let row = tokio::task::spawn_blocking(move || score_store::get_score(&cfg, &id))
        .await
        .map_err(|e| format!("delete_chunk join error: {e}"))?
        .map_err(|e| format!("delete_chunk: {e:#}"))?;
    Ok(u32::from(row.is_some()))
}

pub async fn delete_chunk_rpc(
    config: &Config,
    chunk_id: String,
) -> Result<RpcOutcome<DeleteChunkResponse>, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;

    // Resolved on `provider()` and **refused** when the family is absent, not
    // degraded. The read handlers above answer empty for a missing family
    // because "this driver indexes nothing" is a true answer to what they were
    // asked; this is a delete, and its empty answer — `deleted: false` — is
    // byte-identical to "that chunk was already gone". The contract makes the
    // same call in `forget_matching`'s own errors: on a delete, a zero the
    // caller reads as "already gone" is worse than a refusal.
    let Some(sources) = binding.provider().as_sources() else {
        return Err(format!(
            "delete_chunk: driver '{}' does not serve Sources",
            binding.driver_id()
        ));
    };

    // Both side-row counts are observed BEFORE the delete, because
    // `ForgetOutcome` does not carry them: it reports `chunks_removed` and
    // `trees_cleaned`, and its own docs say the per-chunk side rows go
    // *together with* the chunk rather than being counted apart from it.
    // `DeleteChunkResponse` has reported these two numbers since it existed and
    // the frontend logs both, so they are read here rather than dropped to
    // zero — a zero after a successful delete reads as "there was nothing to
    // clean up", which is a different claim from "nobody counted".
    //
    // The cost, stated rather than hidden: these are a pre-delete observation,
    // not the transactional rowcounts the hand-written SQL returned. A writer
    // landing between the read and the delete would move them. Both are
    // diagnostics about one chunk a user is looking at, so a race no user can
    // provoke is the cheaper price than a number that stops being reported.
    let entity_index_rows_removed = chunk_entity_rows(&binding, &chunk_id, "delete_chunk")
        .await?
        .iter()
        .map(|occurrence| occurrence.mentions)
        .fold(0u32, u32::saturating_add);
    let score_rows_removed = score_row_count(config, &chunk_id).await?;

    let selector = ForgetSelector::Chunk {
        chunk_id: chunk_id.clone(),
    };
    let outcome = sources
        .forget_matching(&selector)
        .await
        .map_err(|e| format!("delete_chunk: {e}"))?;

    // No host-side file removal any more, and no `spawn_blocking` to host it.
    // The driver's by-id delete collects `content_path` inside the transaction
    // and unlinks after the commit, together with the chunk's embedding row,
    // its reembed-skipped row, and any raw-ref files nothing else points at —
    // the last three of which the hand-written SQL this replaces left behind.
    let resp = DeleteChunkResponse {
        deleted: outcome.chunks_removed > 0,
        score_rows_removed,
        entity_index_rows_removed,
    };
    log::debug!(
        "[memory_tree::read::delete] driver '{}' id={chunk_id} chunks_removed={} \
         trees_cleaned={} score_rows={score_rows_removed} \
         entity_rows={entity_index_rows_removed}",
        binding.driver_id(),
        outcome.chunks_removed,
        outcome.trees_cleaned
    );
    Ok(RpcOutcome::single_log(
        resp.clone(),
        format!(
            "memory_tree::read: delete_chunk id={chunk_id} deleted={} score_rows={} entity_rows={}",
            resp.deleted, resp.score_rows_removed, resp.entity_index_rows_removed
        ),
    ))
}
