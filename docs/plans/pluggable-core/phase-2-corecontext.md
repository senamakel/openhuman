# Phase 2 — `CoreContext` ownership, handler signature, registry collapse, store traits

**Status:** planned.
**Goal:** make state reachable _through_ a context instead of _only_ through
process globals, without a big-bang DI rewrite. Three landable sub-series:
(a) signature-only sweep, (b) registry collapse, (c) per-domain migration
with store-trait extraction — tracked in a drift ledger
(`pluggable-core-drift-ledger.md`, created with the first migrated domain,
per the tinyagents/tinycortex convention).

## 2.a Handler signature (Stage B — mechanical)

```rust
// src/core/all.rs:21 today:
pub type ControllerHandler =
    fn(Map<String, Value>) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;

// after:
pub type ControllerHandler =
    fn(Arc<CoreContext>, Map<String, Value>) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;
```

- Stays a **plain `fn` pointer** — handlers need no captured state, so the
  sweep is mechanical: every `handle_*` in every domain `schemas.rs` gains a
  leading `_ctx: Arc<CoreContext>` and the PR compiles **without touching
  handler bodies**. `invoke_method` (`src/core/jsonrpc.rs:230`) threads the
  context; `default_state()` becomes "the context of the default runtime"
  (an `OnceLock<Arc<CoreContext>>` set by the first `CoreBuilder::build`,
  so old global paths and new context paths read the same state during the
  migration window).
- Land as one signature-only PR. Coverage gate: the diff is signatures, so
  existing tests carry it.

## 2.b Registry collapse

Replace the three hand-maintained parallel lists in `src/core/all.rs`
(handlers `:105-344`, schemas `:375-509`, namespace descriptions `:530-690`)
with one per-domain struct:

```rust
pub struct DomainRegistration {
    pub namespace: &'static str,
    pub description: &'static str,
    pub controllers: Vec<RegisteredController>,   // schema + handler already paired
    pub cli: Option<CliHandler>,                  // absorbs CLI_ADAPTERS (all.rs:88)
}
fn all_domains() -> Vec<DomainRegistration> { vec![about_app::domain(), /* … one line per domain … */] }
```

- Schemas derive from `controllers`, so `validate_registry`'s panic-on-drift
  check (`all.rs:886`) becomes true by construction and is deleted.
- The hard-coded `namespace_description` match dies as a side effect.
- **Explicitly rejected:** `inventory`/linkme link-section auto-registration.
  One explicit `all_domains()` list matches this repo's ledger culture and
  keeps controller exposure auditable ("controller-only exposure" rule).
- Done in the same sweep window as 2.a since both touch every registration
  site — one churn pass, two PRs.

## 2.c Per-domain migration + store traits

Domains migrate `_ctx` → real context usage opportunistically; **required**
(because multi-context isolation in phase 3 needs them) for: `memory`
(tinycortex seam handle), `people`, `attachments`, `config`. Everything else
migrates when touched.

Storage abstraction lands here, at the handle boundary:

```rust
impl CoreContext {
    pub fn people(&self) -> Arc<dyn PeopleStore>;
    pub fn attachments(&self) -> Arc<dyn AttachmentStore>;
    // … per-domain accessors added as domains migrate
}

pub enum StorageBackend { WorkspaceFs }   // CoreBuilder::storage(..); only impl for now
```

- Traits are carved **per domain as it migrates** (drift-ledger column
  "store trait extracted?"), never as one up-front storage rewrite.
- `WorkspaceFs` wraps the existing SQLite/JSON-on-disk stores byte-for-byte;
  the global facades (`*::global()`) delegate to the default context's
  handles so both paths hit the same instance.
- Host-owned stores only (people, attachments, cost/x402 ledgers, config
  state, run ledgers). `tinyagents` (`StoreRegistry`, checkpointers) and
  `tinycortex` already own their internal store interfaces — the context
  holds _handles to_ those seams, it does not re-abstract them.
- Adding a remote backend (e.g. Postgres) must become "new `StorageBackend`
  impl", but shipping one is out of scope.

## Risks & mitigations

| Risk                                     | Mitigation                                                                             |
| ---------------------------------------- | -------------------------------------------------------------------------------------- |
| Sweep-size diff unreviewable             | three PR series: signature-only → registry collapse → per-domain                       |
| Global facade and context handle diverge | facade delegates to default context (single instance); parity asserts in debug builds  |
| Store trait too narrow, churns later     | extract traits from _observed_ handler usage per domain, not speculatively             |
| Coverage gate on mechanical diffs        | signature/registry PRs carry existing tests; per-domain PRs add store-trait unit tests |

## Verification

- `pnpm test:rust` + `json_rpc_e2e` green after each sub-series.
- Drift ledger lists every domain with columns: signature migrated / context
  usage / store trait / notes.
- `rg 'fn handle_.*Map<String, Value>' src/openhuman` shows no old-signature
  stragglers after 2.a.
