# 02.4 — One model catalog

Three sources of model metadata exist: `cost/catalog.rs` (pricing +
windows), provider capability accessors, and
`docs/inference-provider-catalog.md`. Crate `registry::ModelCatalog` is the
normalized shape (`ModelCatalogEntry { provider, model_id, aliases,
max_input_tokens, max_output_tokens, pricing, capabilities }` incl. cache/
reasoning rates).

## Steps

1. Build a `ModelCatalogSnapshot` from OpenHuman's data: seed from crate
   `ModelCatalog::seed()`, overlay `cost/catalog.rs` rates and
   `model_context.rs` windows, plus local-model profiles discovered at
   runtime (ollama).
2. Point consumers at the one projection: `estimate_cost_usd`
   (cost bridge), token budgeting / `effective_context_window`, model picker
   RPC, capability filter (02.1 profiles via
   `ModelProfile::from_catalog_entry`).
3. Keep the catalog OpenHuman-refreshable (config/update path) — crate seed
   is a fallback, not the source.

## Deletions

- Duplicate per-MTok rate tables and window constants outside the catalog
  (`cost/catalog.rs` becomes the loader/owner of the snapshot rather than a
  second schema, or is folded into it).

## Acceptance

- Pricing/window/capability lookups have exactly one code path; cost
  estimates unchanged (existing catalog tests re-pointed).
- Local models appear in the catalog with runtime-discovered profiles.
