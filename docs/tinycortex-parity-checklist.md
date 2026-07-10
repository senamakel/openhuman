# TinyCortex Data-Format Parity Checklist (Phase 0.3)

**Purpose.** Existing user workspaces must open **unchanged** after every cutover flip.
This checklist enumerates every on-disk format shared between the host engine and TinyCortex,
records the audit result, and specifies the **golden-workspace parity harness** that gates W3, W5, W6.

**Hard rule (plan §0.3/§6):** any mismatch is fixed **upstream in tinycortex**, never papered over
with a host shim.

Anchors: host `7850cf363` · tinycortex `d1a8c7be` (v0.1.1).

## Ownership tiers in the shared workspace (key finding)

A user workspace's `chunks.db` (and content vault) holds **two tiers**:

1. **Crate-owned substrate** — moves to TinyCortex, schema must match byte-for-byte:
   `vectors`, `kv_global`, `kv_namespace`, `store_meta`, `legacy_marker`, `mcp_writes`,
   `mem_tree_chunks`, `mem_tree_chunk_embeddings`, `mem_tree_chunk_reembed_skipped`,
   `mem_tree_summaries`, `mem_tree_summary_embeddings`, `mem_tree_summary_reembed_skipped`,
   `mem_tree_buffers`, `mem_tree_trees`, `mem_tree_score`, `mem_tree_entity_index`,
   `mem_tree_entity_edges`, `mem_tree_entity_hotness`, `mem_tree_ingested_sources`,
   `mem_tree_jobs` **(20 tables — exact name parity confirmed)**.

2. **Host-retained `UnifiedMemory` namespace-document tier** — stays host (the "namespace
   document/graph store" plan §1 keeps host), coexisting in the **same** DB file:
   `memory_docs`, `graph_global`, `graph_namespace`, `episodic_log` (+ `episodic_fts` virtual +
   `episodic_ai/ad/au` triggers), `event_log` (+ `event_fts` + `event_embeddings` +
   `event_ai/ad/au` triggers), `conversation_segments`, `segment_embeddings`, `vector_chunks`,
   `user_profile` **(10 tables + FTS + triggers)**.

   These live in `src/openhuman/memory_store/unified/{init,fts5,events,segments,profile}.rs`.
   **W3 must keep the host creating/reading these in the same DB the crate now manages** —
   the crate's `chunks::with_connection` opens the shared handle; host `UnifiedMemory` schema
   init runs alongside the crate's. Parity requirement: crate schema init and host `unified`
   init must **compose without collision** on both fresh and existing DBs.

## Parity results by format dimension

| # | Format | Result | Evidence |
| --- | --- | --- | --- |
| P1 | **Deterministic chunk ID** | ✅ **IDENTICAL** | `chunk_id()` = SHA-256 over `source_kind.as_str()` `\0` `source_id` `\0` `seq_in_source.to_be_bytes()` `\0` `content`, first 32 hex chars. Host `memory_store/chunks/types.rs:269` vs crate `chunks/types.rs:282` — byte-for-byte identical. |
| P2 | **Vector encoding (packed f32)** | ✅ **IDENTICAL** | `vec_to_bytes`: little-endian `f32::to_le_bytes`, 4 bytes/elem, no header. Host `vectors/store.rs:467` vs crate `store/vectors/store.rs:397` — identical. |
| P3 | **`vectors` table schema** | ✅ **IDENTICAL** | `(id TEXT, namespace TEXT, text TEXT, embedding BLOB, metadata TEXT DEFAULT '{}', created_at REAL, updated_at REAL, PRIMARY KEY(namespace,id))` + `idx_vectors_ns`. Identical. |
| P4 | **`mem_tree_jobs` (queue) columns** | ✅ **MATCH** | Both persist `(id, kind, payload_json, dedupe_key, status, attempts, max_attempts, available_at_ms, locked_until_ms, last_error, created_at_ms, started_at_ms, completed_at_ms)` (identical INSERT column list). Job **payload_json** shapes must also match — verify per `JobKind` in harness (P9). |
| P5 | **`mem_tree_chunks` base columns** | ✅ **MATCH (base)** ⚠️ **new-DB divergence** | Base 15 columns identical (`id…chunk_id`). **Divergence:** crate's `CREATE` inlines 3 legacy embedding columns (`model_signature TEXT, vector BLOB, dim INTEGER`) that the host **dropped** after migrating inline embeddings to the `mem_tree_chunk_embeddings` sidecar (host `chunks/store.rs:110` comment, #1574). **Existing DBs: compatible** — both run `CREATE TABLE IF NOT EXISTS` (no-op on existing) + `migrate_legacy_embeddings_to_sidecar` (crate `chunks/migrations.rs:23`). **Fresh DBs: crate adds 3 unused columns.** Risk: positional `INSERT`/`SELECT *`. **Action: harness asserts fresh-DB schema equality; if the 3 cols matter, drop them upstream.** |
| P6 | **Content vault paths** | ✅ **IDENTICAL sig** ⚠️ verify `sanitize_filename` | `chunk_rel_path(source_kind, source_id, chunk_id)` and `summary_rel_path(tree_kind, scope_slug, level, summary_id)` — identical signatures, same per-`source_kind` branching (`email` special-case). Host sanitizes chunk-id colons → `-` (`content/paths.rs:284`); crate uses `sanitize_filename` (`paths.rs:179`). **Action: harness asserts identical relative paths for a corpus of colon/unicode/long chunk-ids** (Windows-illegal chars are the risk). |
| P7 | **YAML frontmatter** | ✅ **MATCH** (verify key order) | Summary markdown frontmatter delimited by `---\n … ---\n` (crate `content/compose/summary.rs:76,127`; `split_front_matter` on `rposition(line=="---")`). **Action: harness asserts identical frontmatter key set + order + serialization** for chunk & summary markdown (byte-compare composed files). |
| P8 | **Entity markdown** | ⏳ harness | `entities/` registry markdown. Host `memory_entities` (0 external refs) ↔ crate `entities/`. Low risk (full port, no drift). Harness byte-compares entity files. |
| P9 | **Git diff-ledger layout** | ✅ git-backed both | Host migrated to git-backed ledger 06-25 (`040e6e20d`), captured in port (crate `diff/ledger.rs`, `diff.rs:98`). Verify `.git` repo layout + snapshot markdown + read-marker storage identical. Harness opens an existing diff repo with both. |
| P10 | **`store_meta` / embedding signature** | ✅ format | `format_embedding_signature` = `"provider={name};model={model};dims={dims}"` (crate `store/vectors/embedding.rs`). Host must produce the identical signature string from `Config` (W1 `embeddings.rs` seam) or re-embed churn triggers. **Action: seam test asserts signature string equality.** |
| P11 | **Remaining `mem_tree_*` column parity** | ⏳ harness | `mem_tree_summaries`, `mem_tree_buffers`, `mem_tree_trees`, `mem_tree_score`, `mem_tree_entity_index`, `mem_tree_entity_edges`, `mem_tree_entity_hotness`, `mem_tree_ingested_sources`, `mem_tree_chunk_embeddings`, reembed-skipped tables, `kv_*`, `mcp_writes`, `legacy_marker`. Spot-checks clean; full column+index+PK diff is automated in the harness. |
| P12 | **Host-retained tier coexistence** | ⏳ W3 gate | Crate schema init + host `unified` init must both run on the shared DB without `CREATE`/index collisions. Harness opens a real workspace, runs crate init then host init (and vice-versa), asserts full `sqlite_master` superset is preserved and no data dropped. |

Legend: ✅ audited-identical · ⚠️ divergence flagged · ⏳ deferred to harness (automated per-flip).

---

## The golden-workspace parity harness (design)

The built-in parity harness is the read-side comparator that gates each risky flip. It exists at
two layers.

### Layer 1 — schema/format asserters (host-side unit tests, cheap, run every PR)

Pure-function comparators (no disk), implemented in **`src/openhuman/tinycortex/parity.rs`**
(`#[cfg(test)]`). Status ✅ = landed & green; ⏳ = pending.

- ✅ **`chunk_id_matches_historical_golden` / `chunk_id_is_sensitive_to_every_field`** — golden +
  every-field sensitivity for the deterministic `chunk_id` (covers P1). *(After W3 both resolve to the
  crate; the golden vector stays as a regression pin.)*
- ✅ **`vector_encoding_is_le_packed_f32`** — `vec_to_bytes`/`bytes_to_vec` LE-packed-f32 round-trip
  + golden bytes (P2).
- ✅ **`chunk_rel_path_host_crate_byte_parity` / `summary_rel_path_host_crate_byte_parity`** —
  adversarial id corpus (colons, all Windows-illegal chars, unicode, >255 chars, gmail participant
  slugs, malformed email; every summary-id shape × 3 tree kinds × levels) → assert host
  `chunk_rel_path`/`summary_rel_path` **byte-equal** the crate's (P6). *(Landed; verified identical.)*
- ✅ **`embedding_signature_host_crate_byte_parity`** — assert host
  `embeddings::format_embedding_signature` == crate `store::vectors::format_embedding_signature` and
  both == the golden `provider={name};model={model};dims={dims}` over a provider corpus (P10). The
  seam's own `signature()` pass-through is separately pinned in `tinycortex/embeddings.rs`.
- ⏳ **`frontmatter_parity`** — compose a fixed chunk+summary → byte-compare markdown incl.
  frontmatter key order (P7). *(Not yet landed — needs the host/crate compose types aligned.)*

### Layer 2 — golden-workspace differential harness (the flip gate)

The core mechanism from plan §0.3: **one on-disk workspace, opened by both engines, outputs compared.**

> **Status (2026-07-10).** First cut landed as **`tests/memory_golden_parity_e2e.rs`** —
> comparator **1** (schema composition) and comparator **5** (idempotent re-open) are green:
> a real workspace is stood up through the host production surface (`memory::ops`), the crate
> substrate init is forced deterministically, and **all `*.db` files under the workspace are scanned
> path-agnostically** (union of tables). It asserts the crate chunk-DB substrate (15 `chunks/schema.rs`
> tables) and the host `UnifiedMemory` tier (10 tables) **coexist without collision** (P3/P5/P11/P12),
> and that re-running the flow adds/drops no tables (comparator 5). Still TODO: `vectors`/`store_meta`/
> `kv_*` (created by the chunk/embed pipeline, need a widened ingest flow), the seeded golden fixture +
> `scripts/gen-golden-workspace.sh`, and comparators **2** (recall/retrieval snapshot), **3** (tree
> read), **4** (byte-compare vault) — these require a populated, sealed fixture and the W5 retrieval
> surface, so they land alongside the W3/W5 flips.

**Fixture.** Check in a small, deterministic `tests/fixtures/golden-workspace/` produced by the
*pre-migration* build: a real `chunks.db` + content vault + diff `.git`, seeded via a fixed script
(`scripts/gen-golden-workspace.sh`) with: a handful of chat + document + email sources across ≥2
namespaces, ingested + scored + sealed to ≥2 tree levels, some entities/edges, a few queue jobs in
mixed states, and both tiers populated (episodic/event/segment rows present). Store the **generator
script** and a **manifest** (expected chunk-ids, summary paths, recall snapshots) so the fixture is
regenerable and reviewable, not an opaque blob.

**Comparators (read-only, both engines open the SAME copied workspace):**

1. **Schema snapshot** — dump `sqlite_master` (tables, indexes, triggers, `sql` text normalized) from
   the DB after each engine's `open`/init; assert the crate-owned 20-table set is identical and the
   host-retained 10-table tier is untouched (P3–P5, P11, P12).
2. **Recall/retrieval snapshot** — run a fixed query battery through the stable public surface
   (`openhuman::memory::` recall + `read_rpc` retrieval primitives) on pre- and post-migration builds;
   assert identical ordered hit ids + scores (within f64 epsilon) + `supporting_relations` (guards G2)
   + taint (guards the security seam).
3. **Tree read snapshot** — `read_tree` / `drill_down` / `cover_window` over the sealed tree; assert
   identical node structure + summary content.
4. **Byte-compare vault** — after a read-only open, assert **no content files changed** (a flip must
   not rewrite the vault) and, for a controlled re-ingest of one source, assert composed markdown is
   byte-identical.
5. **Idempotent re-open** — open → close → open with the post-migration build; assert no migration
   churn (no re-embed storm, no schema rewrite) on an already-current DB.

**Wiring.** Runs under `pnpm test:rust` (host-side, counts toward coverage) and as a dedicated
`tests/memory_golden_parity_e2e.rs`. The existing crate-level integration tests
(`tests/memory_roundtrip_e2e.rs`, `memory_tree_sync_deep_raw_coverage_e2e.rs`) act as the
"public-surface still green" guard (plan §5.1); this harness adds the **differential** guard that a
flip preserves *existing* data, not just that the API still functions.

**Gate mapping:** Layer-1 asserters run every PR. Layer-2 golden harness is **green-before-merge** on
**W3** (store+chunks), **W5** (tree+retrieval+score), **W6** (ingest). W4 (queue) additionally asserts
job payload_json parity (P4/P9). Any red = upstream fix in tinycortex, re-bump submodule, re-run.

**W-SYNC gates (amendment 2026-07-09, plan §8):**
- **P13 sync-status parity** — `memory_sync_status_list` output (per-`source_kind` freshness rows)
  byte-equal pre/post flip on a golden workspace; asserter added to
  `src/openhuman/tinycortex/parity.rs`.
- **P14 Composio sync test pair** — the crate's mocked-HTTP gmail pipeline test
  (`vendor/tinycortex/tests/composio_sync_mock.rs`, wiremock, always-on) green in crate CI before
  the W-SYNC.3 flip; the live `#[ignore]` test (`composio_sync_live.rs`, `COMPOSIO_API_KEY`) run
  manually at least once per flip wave.

**W-EMB gate:** the existing **P10 `embedding_signature_parity`** asserter is the regression pin —
the tinyagents-backed provider stack must emit byte-identical
`provider={name};model={model};dims={dims}` signatures, or existing vector spaces split.

## Open divergences to resolve upstream before their flip

| Item | Flip gated | Resolution |
| --- | --- | --- |
| P5 `mem_tree_chunks` 3 legacy inline columns (fresh-DB) | W3 | Confirm no positional `INSERT`/`SELECT *`; if the columns are dead, drop them in a tinycortex PR so fresh DBs match. |
| P6 `sanitize_filename` vs host colon→`-` | W3 | Prove identical output on adversarial id corpus; align upstream if any diverge (Windows-illegal chars). |
| P7 frontmatter key **order** | W5/W6 | Byte-compare composed markdown; align serializer order upstream if diff. |
| G2 `supporting_relations` (graph_* host-retained vs crate derive-on-read) | W6/W7 | Recall snapshot (comparator 2) must match; else upstream relation persist. |

All other dimensions (P1–P4, P8–P12) audited compatible or covered by the automated harness.
