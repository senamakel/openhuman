# Agent self-learning

OpenHuman learns user preferences continuously and surfaces them as ambient defaults in every system prompt. The mechanism is a small **personalization cache** materialized from multiple deterministic + LLM-driven producers, scored by stability, rendered into a user-editable `PROFILE.md`, and injected into prompts through the existing prompt-section pipeline.

This document covers how preferences are captured, scored, persisted, and surfaced. For the originating issue, see [#566](https://github.com/tinyhumansai/openhuman/issues/566).

---

## What gets learned

Six classes, encoded in the `key` prefix and stored in the existing `user_profile_facets` table:

| Class | Example facets |
|---|---|
| `style/*` | `verbosity=terse`, `format=bullets`, `preamble=skip`, `language=english`, `emoji=skip` |
| `identity/*` | `name=Sanil`, `timezone=PST`, `role=engineer`, `employer=vezures` |
| `tooling/*` | `package_manager=pnpm`, `lang=rust`, `framework=astro`, `runtime=bun` |
| `veto/*` | `tool=jest → banned`, `format=nested-bullets → banned` |
| `goal/*` | free-form goal sentences (slugified key) |
| `channel/preference` | `primary=desktop-chat` |

Recurring topics, recurring entities, and prior threads are **not** in the cache. They live in the memory tree and are retrieved per-turn by `memory_recall` under the prompt-bias instruction described below.

---

## Architecture

Four stages: **capture → identify → score → materialize**.

```
inputs                 substrate              candidates              cache
─────                  ─────────              ──────────              ─────
chat turns       ──→   episodic_log    ──→   Buffer (push)  ──┐
                       + tree                                  │
skill syncs      ──→   tree (sources)  ──→   Buffer (push)  ──┤
                                                               ├─→  user_profile_facets
channel inbound  ──→   tree            ──→   Buffer (push)  ──┤        (state, stability,
                                                               │         user_state, evidence)
documents        ──→   tree            ──→   Buffer (push)  ──┘                   │
                                                                                  ▼
                                       stability_detector::rebuild           CacheRebuilt event
                                       (every 30 min + event-driven)              │
                                                                                  ▼
                                                                     ProfileMdRenderer
                                                                                  │
                                                                                  ▼
                                                                            PROFILE.md
                                                                            (managed blocks)
                                                                                  │
                                                                                  ▼
                                                                     UserFilesSection
                                                                       + UserProfileSection
                                                                       + MemoryAccessSection
                                                                                  │
                                                                                  ▼
                                                                       agent system prompt
```

Chat turns flow into the tree as `source_id="conversations:agent"` (with `tool_calls_json` stripped at canonicalize), so the same `tree_source::summariser` that already runs over Slack/Gmail/Notion now also produces summaries — and structured facet candidates — over chat content.

---

## Producers

Five producers write `LearningCandidate` values into `learning::candidate::global()`:

| # | Producer | Emits | LLM? |
|---|---|---|---|
| 1 | `composio::providers::profile::persist_provider_profile` (existing) | Identity from Gmail/Slack/Notion account fields | No |
| 2 | `learning::extract::signature::EmailSignatureSubscriber` | Identity from email signatures (last 8 lines) | No |
| 3 | `learning::extract::heuristics` (`LengthRatioDetector`, `EditWindowDetector`, `CorrectionRepeatDetector`) | Style + Veto from per-turn rolling state | No |
| 4 | `learning::ReflectionHook` (rerouted) | Goal + Style from heuristic cues and LLM-extracted reflections | Optional |
| 5 | `tree_source::summariser::llm` (extended schema) | All classes — long-tail extraction over rolling per-source summaries | Yes (existing call, extended output) |

Producer 5 is the long-tail backbone. Its prompt now asks the model to emit `{ summary, facets[] }`, and `learning::extract::summary_facets` validates each `ParsedFacet` (canonical class, mandatory `evidence_chunks`) before pushing to the buffer. **No new LLM calls** are introduced — the change extends an existing summarization round-trip with ~150–400 output tokens.

Explicitly **not** producing:

- Hand-curated regex catalogs for style/identity/tooling — Producer 5 covers them and generalizes to vocabulary the catalog wouldn't predict.
- Hand-curated manifest parsers for tooling — same reasoning; Astro, Bun, Deno, uv, mise, etc. all surface through the LLM summarizer.
- Free-text NER on chat — recurring entities live in the tree's graph (from structured provider metadata) and surface contextually via `memory_recall`.
- `ToolTrackerHook` candidate emission — failure-rate is not a clean preference signal; it stays as substrate for debugging.

---

## Identification → candidate buffer

Every emission carries provenance:

```rust
pub struct LearningCandidate {
    pub class: FacetClass,           // Style | Identity | Tooling | Veto | Goal | Channel
    pub key: String,                 // canonical slug, e.g. "verbosity"
    pub value: String,               // canonical value, e.g. "terse"
    pub cue_family: CueFamily,       // Explicit | Structural | Behavioral | Recurrence
    pub evidence: EvidenceRef,       // pointer back into substrate
    pub initial_confidence: f64,     // 0..=1, source-provided hint
    pub observed_at: f64,            // epoch seconds
}
```

`EvidenceRef` variants cover every substrate origin (`Episodic`, `EpisodicWindow`, `SourceSummary`, `TreeTopic`, `DocumentChunk`, `EmailMessage`, `Provider`, `ToolCall`, `TreeSourceWeight`). The buffer is a thread-safe bounded ring (default capacity 1024). The stability detector drains it every rebuild.

---

## Stability scoring

```
stability(class, key, value) = base × cue × user_state

base = Σ over evidence:
         cue_family_weight × exp(-Δt / half_life_for_class) × log(1 + evidence_count_for_family)

cue  = 2.0 if any evidence is Explicit, else 1.0
user = ∞  if Pinned, 0 if Forgotten, 1 otherwise
```

**Cue-family weights**:

| Family | Weight | Rationale |
|---|---|---|
| `Explicit` | 1.0 | direct user statement — declaration is intent |
| `Structural` | 0.9 | provider data / manifest content / signature — data doesn't lie |
| `Behavioral` | 0.7 | heuristics + summary mining — must accumulate |
| `Recurrence` | 0.6 | tree statistics — emerging |

**Class half-lives** (the time over which evidence weight halves):

| Class | Half-life |
|---|---|
| `identity` | 90 days |
| `veto` | 60 days |
| `tooling` | 30 days |
| `goal` | 30 days |
| `style` | 14 days |
| `channel` | 7 days |

**Conflict resolution per `(class, key)`**: active value = `argmax(stability)` over candidate values. Losing values are dropped from the cache; if they re-emerge they reinforce naturally through the same path.

---

## States and qualification

Two state columns govern visibility:

`state` (driven by stability):

| State | Range | Effect |
|---|---|---|
| `Active` | `stability ≥ τ_promote` (1.5) | Renders in `PROFILE.md`, injected into prompt |
| `Provisional` | `0.7 ≤ stability < 1.5` | Stored, not rendered |
| `Candidate` | `0.4 ≤ stability < 0.7` | Stays in buffer, not yet in cache |
| `Dropped` | `stability < 0.4` | Removed |

`user_state` (overrides scoring):

| State | Effect |
|---|---|
| `Auto` | Default; `state` follows scoring |
| `Pinned` | Locks `Active`; resists decay |
| `Forgotten` | Locks `Dropped`; blocks re-promotion |

**Class budgets** (top-N selection per class with a shared overflow pool):

```
style 4 · identity 4 · tooling 5 · veto 3 · goal 3 · channel 1 · overflow 5
total cap ≈ 25
```

Per-class budgets prevent a tooling-heavy user from drowning out style and identity; the overflow pool lets one class take spare capacity from another when underused.

---

## Storage

Single SQLite table — `user_profile_facets` (existing, extended by an idempotent migration in `memory::store::unified::profile::migrate_profile_schema`):

```sql
CREATE TABLE user_profile (
    facet_id            TEXT PRIMARY KEY,
    facet_type          TEXT NOT NULL,              -- legacy enum
    key                 TEXT NOT NULL,
    value               TEXT NOT NULL,
    confidence          REAL NOT NULL DEFAULT 0.5,
    evidence_count      INTEGER NOT NULL DEFAULT 1,
    source_segment_ids  TEXT,
    first_seen_at       REAL NOT NULL,
    last_seen_at        REAL NOT NULL,
    -- learning-cache columns (added by migrate_profile_schema):
    state               TEXT NOT NULL DEFAULT 'active',
    stability           REAL NOT NULL DEFAULT 0.0,
    user_state          TEXT NOT NULL DEFAULT 'auto',
    evidence_refs_json  TEXT,
    class               TEXT,
    cue_families_json   TEXT,
    UNIQUE(facet_type, key)
);
CREATE INDEX idx_profile_state ON user_profile(state);
CREATE INDEX idx_profile_class ON user_profile(class);
```

The provider-profile path (`composio::providers::profile::persist_provider_profile`) continues to write its existing rows untouched; class is auto-derived for backward-compatibility (provider keys like `skill:gmail:default:email` map to `class=tooling`/`identity`).

---

## Surface — `PROFILE.md`

`{workspace_dir}/PROFILE.md` carries multiple managed blocks. Each block is owned by automation; content outside the markers is user-authored and preserved across rebuilds.

```markdown
# User Profile

<!-- openhuman:style:start -->
## Style

- **verbosity**: terse
- **preamble**: skip *(pinned)*

<!-- openhuman:style:end -->

<!-- openhuman:identity:start -->
## Identity

- **name**: Sanil
- **timezone**: PST

<!-- openhuman:identity:end -->

<!-- openhuman:tooling:start -->
## Tooling

- **lang**: rust
- **package_manager**: pnpm

<!-- openhuman:tooling:end -->

<!-- openhuman:vetoes:start -->
## Vetoes

- **tool=jest**: banned

<!-- openhuman:vetoes:end -->

<!-- openhuman:goals:start -->
## Goals

- ship #566 before #686

<!-- openhuman:goals:end -->

<!-- openhuman:connected-accounts:start -->
## Connected Accounts

- gmail (sanil@vezures.xyz)

<!-- openhuman:connected-accounts:end -->
```

User-authored content between blocks (free-form notes, hand-edited details) is preserved verbatim across rebuilds.

`ProfileMdRenderer` subscribes to `DomainEvent::CacheRebuilt` and rewrites the cache-derived blocks (`style`, `identity`, `tooling`, `vetoes`, `goals`). The `connected-accounts` block remains owned by the provider sync path.

---

## Surface — prompt sections

Three sections in `agent/prompts/` cooperate:

- **`UserFilesSection`** — already-existing; injects `PROFILE.md` verbatim into the system prompt every turn.
- **`UserProfileSection`** — repointed in Phase 4 to read `FacetCache::list_active()` (gated by `LearningConfig::use_cache_for_user_profile_section`, default true). Renders `- **{class}/{key}**: {value}` bullets.
- **`MemoryAccessSection`** — new in Phase 4; static instruction biasing the agent to call `memory_recall` for entities, threads, prior decisions, recurring topics. Adds ~80 tokens to the system prompt.

The `MemoryAccessSection` covers the **contextual** half of personalization that the cache structurally cannot address: things that need to be retrieved when relevant (recurring people, prior threads) rather than always rendered.

---

## Surface — RPC controllers

Wired through the controller registry (`learning::schemas::all_learning_controller_schemas`):

| RPC | Purpose |
|---|---|
| `learning.list_facets { class }` | Enumerate the cache, optionally filtered by class. Returns active + provisional entries with provenance. |
| `learning.get_facet { class, key }` | Single-entry lookup. |
| `learning.update_facet { class, key, value }` | Overwrite the active value; auto-pins to prevent rebuild from clobbering. |
| `learning.pin_facet { class, key }` | `user_state = Pinned`. Locks `Active` regardless of stability score. |
| `learning.unpin_facet { class, key }` | `user_state = Auto`. |
| `learning.forget_facet { class, key }` | `user_state = Forgotten`. Locks `Dropped`, blocks re-promotion. |
| `learning.reset_cache {}` | Clears all `Auto` rows; preserves `Pinned`. Next rebuild repopulates from substrate. |
| `learning.rebuild_cache {}` | Manual trigger for the stability rebuild. |
| `learning.cache_stats {}` | `{ total, active, provisional, candidate, dropped, by_class }`. |

The agent itself acts as a conversational user-control surface: asked "what do you know about me?" it can call `list_facets` and cite `evidence_refs_json` for each entry; "forget that I prefer terse" calls `forget_facet("style", "verbosity")`.

---

## Configuration

`LearningConfig` in `src/openhuman/config/schema/learning.rs`:

```rust
pub struct LearningConfig {
    pub enabled: bool,                              // master switch
    pub reflection_enabled: bool,
    pub user_profile_enabled: bool,                 // legacy hook
    pub tool_tracking_enabled: bool,                // substrate-only
    pub reflection_source: ReflectionSource,        // Local | Cloud
    pub max_reflections_per_session: usize,
    pub min_turn_complexity: usize,
    pub chat_to_tree_enabled: bool,                 // pipe agent chat into tree
    pub stability_detector_enabled: bool,
    pub rebuild_interval_secs: u64,                 // default 1800 (30 min)
    pub use_cache_for_user_profile_section: bool,   // route prompt-section reads through cache
}
```

The summarizer-side facet emission is gated by `LlmSummariserConfig::structured_facet_extraction` (default `true`) so production deployments can disable structured extraction independently of the rest of the learning subsystem.

---

## Observability

`DomainEvent::CacheRebuilt { added, evicted, kept, total_size, rebuilt_at }` is published after every successful rebuild. Subscribers can wire personalization metrics — for example, facet-in-prompt × positive-acceptance rate — to satisfy #566's "measurable personalization improvements" criterion.

`learning.cache_stats` returns the breakdown by state and class for ad-hoc inspection.

Tracing prefixes used by new flows (filter your log stream with these):

- `[learning::candidate]`
- `[learning::extract::signature]`
- `[learning::extract::heuristics]`
- `[learning::extract::summary_facets]`
- `[learning::stability_detector]`
- `[learning::cache]`
- `[learning::profile_md]`
- `[archivist]` (chat-into-tree path)
- `[memory::tree::ingest]` (`DocumentCanonicalized` emission)

---

## Testing

Unit tests live next to their modules (`*_tests.rs` siblings; consistent with the rest of the codebase). End-to-end coverage in `tests/learning_phase4_integration_test.rs`:

- Push candidates → `stability_detector.rebuild()` → expected `CacheRebuilt` event published
- `ProfileMdRenderer` writes the expected managed blocks into `PROFILE.md`
- `learning.pin_facet` keeps an entry `Active` despite weak evidence
- `learning.forget_facet` removes an entry from `PROFILE.md` and blocks re-promotion
- `learning.list_facets` returns the expected shape and class filter

Run targeted suites:

```bash
cargo test --manifest-path Cargo.toml --lib learning::
cargo test --manifest-path Cargo.toml --lib memory::store::unified::profile
cargo test --manifest-path Cargo.toml --test learning_phase4_integration_test
```

---

## Future work

- **Validation loop** — track facet-in-prompt × positive-acceptance to calibrate cue-family weights from outcome signal rather than fixed defaults.
- **Confirmation prompts** — for high-impact, low-confidence facets (Identity, Veto), surface to the user via the agent or subconscious before promoting to `Active`.
- **Cross-corpus topic stitching** — once chat-as-tree-source has accumulated, surface unified hot topics back into the prompt as contextual hints alongside the cache.
- **Per-channel persona facets** — tooling and style differ by channel; split when signals diverge meaningfully (e.g., terser in Slack DMs, more structured in email).
- **Tool-suggestion telemetry** — distinct from the existing failure stats; needs new `ToolSuggestionAccepted`/`ToolSuggestionRejected` events before it can produce clean veto signal.
- **Soft user confirmation in PROFILE.md** — render `Provisional` rows under a quieter sub-heading so the user can promote them by editing without waiting for accumulation.
