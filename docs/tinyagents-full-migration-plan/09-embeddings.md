# 09 — Embeddings & retrieval

Adapt OpenHuman embeddings/memory retrieval to crate interfaces; OpenHuman
stores stay authoritative.

Target SDK surface: `EmbeddingModel` trait, `VectorStore`, `Retriever`,
`ScoredDoc`, `cosine_similarity`, `MemoryScope`, `AgentEvent::
{MemoryLoaded, MemorySaved}`.

## Steps

1. Adapter: implement crate `EmbeddingModel` over
   `embeddings/provider_trait.rs::EmbeddingProvider` (voyage/openai/cohere/
   ollama/cloud/noop impls unchanged underneath). Keep OpenHuman
   `factory.rs`/rate-limit/retry as construction policy.
2. Retriever facade: implement crate `Retriever` over the memory search
   surface the harness uses (`harness/memory_context.rs` recall injection,
   `agent_memory::memory_loader`) returning `ScoredDoc`s; the harness loads
   retrieval context through the facade so retrieval becomes swappable and
   event-visible (`MemoryLoaded`).
3. Memory source identity: preserve `metadata.path_scope` dedupe semantics
   (CLAUDE.md rule) in the facade mapping.
4. Usage/cost: embedding calls emit crate `Usage` + catalog pricing (06.4).
5. Delete the vestigial `agent/memory_loader.rs` 5-line facade. The unwired
   `agent/tree_loader.rs` eager digest prefetch module has been deleted.

## Deletions

- `agent/memory_loader.rs`.

## Acceptance

- Recall-injection parity on a scripted turn (same citations via
  `collect_recall_citations`); embedding usage appears in cost records.

Keep (product): memory stores, retrieval ranking policy,
`memory_context_safety.rs` gating, archivist (orthogonal durable memory —
assessed KEEP 2026-06-30d; optional later: re-hang as `after_agent`
middleware without changing behavior).
