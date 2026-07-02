# 02.4 — One model catalog

Model metadata is currently split across several sources: `cost/catalog.rs`
(pricing + windows), `Config::model_registry` seeding/enrichment,
`model_context.rs` tier/pattern/local fallbacks, provider capability accessors,
provider `effective_context_window`, and `docs/inference-provider-catalog.md`.
Crate `registry::ModelCatalog` is the normalized shape
(`ModelCatalogEntry { provider, model_id, aliases, max_input_tokens,
max_output_tokens, pricing, capabilities }` incl. cache/reasoning rates).

Current status (2026-07-02): `cost/catalog.rs` is a 622-line static pricing and
window table plus registry-enrichment/estimate helpers. The unused
`context_window` convenience wrapper is gone, the raw pricing table is private,
and `context_window_for_model` now consults the richer `lookup` row for concrete
model ids before falling back to legacy model-context patterns.

## Steps

1. Build a `ModelCatalogSnapshot` from OpenHuman's data: seed from crate
   `ModelCatalog::seed()`, overlay `cost/catalog.rs` rates/windows and
   remaining `model_context.rs` pattern fallbacks, plus local-model profiles discovered at
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
