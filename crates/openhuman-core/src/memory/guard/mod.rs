//! [`MemoryGuard`] — the kernel-owned policy decorator over the bound memory
//! driver (`docs/specs/plan-memory.md` §3.4, `docs/specs/kernel.md` §3.4).
//!
//! ## The shape, and why it is this shape
//!
//! The guard implements [`MemoryProvider`](crate::memory::api::provider::MemoryProvider)
//! over an `Arc<dyn MemoryProvider>`. That makes it *transparent* — a caller
//! writes the same code against the guard as against the driver — and it makes
//! the guard *unskippable by construction* for anyone holding it, because there
//! is no second, unguarded shape to reach for.
//!
//! The load-bearing detail is the `as_*` accessors. Every optional capability
//! family (23 of the contract's 26; only `MemoryCore`, `MemoryRecall` and
//! `MemoryPortability` are mandatory and implemented on the guard directly in
//! `mandatory.rs`) is reachable **only** through them, so an override that
//! forwarded `self.inner.as_tree()` would hand out a raw driver handle and
//! defeat the entire design with one method call. Each family therefore gets
//! its own decorator, owned as a field on the guard (an accessor returns a
//! borrow, so it cannot build one on demand) and present exactly when the inner
//! driver provides that family. See [`families`].
//!
//! ## The seven enforcement steps
//!
//! | # | Step | Where |
//! | - | ---- | ----- |
//! | 1 | `SecurityPolicy` tier | [`GuardPolicy::enforce_read`] / [`GuardPolicy::enforce_write`] |
//! | 1b | path rules | **no-op** — no contract method carries a path; see [`policy`] |
//! | 2 | source scope as a query predicate | [`GuardPolicy::ambient_scope`], applied in `GuardedTree::query_source` |
//! | 3 | taint stamping | [`GuardPolicy::stamp_taint`] |
//! | 4 | redaction | [`GuardPolicy::redact_outbound`] — a no-op for embedded drivers |
//! | 5 | egress + trust | [`GuardPolicy::check_egress`] |
//! | 6 | char budgets | [`budget`], driven by `MemoryHooksConfig` |
//! | 7 | audit + tracing | [`audit`] |
//!
//! Three of those departed from the milestone brief because the brief's version
//! would have been wrong against this tree; each departure is argued at its own
//! call site:
//!
//! - **Step 2 is not applied to `recall`.** The bound driver *refuses* a
//!   scoped recall (`SCOPE_UNAPPLIED`), so filling the parameter from the
//!   task-local would turn every recall inside a `with_source_scope` into a
//!   hard error. The scope is filled on `MemoryTree::query_source`, which is
//!   the one method that pushes it into SQL before `LIMIT`.
//! - **Step 3 raises, it never overrides.** A plain override would rewrite a
//!   caller's `ExternalSync` down to `Internal` outside a scope, which is the
//!   laundering step the contract says the guard exists to prevent.
//! - **Step 1's path half is a no-op**, because nothing in the contract carries
//!   a filesystem path to validate.
//!
//! ## What still hands out the bare driver
//!
//! [`CoreContext::memory`](crate::core::runtime::context::CoreContext::memory)
//! is the accessor product code holds.
//! `CoreContext::memory_binding()` and `MemoryBinding::unguarded_provider()`
//! still exist and still hand out the bare driver; `memory/bypass_allowlist_tests.rs`
//! enumerates who may call them. The one production caller of
//! `unguarded_provider()` — the health probe in `memory::ops::provider` —
//! should keep bypassing the guard: a liveness probe is not product code, and
//! running it through the tier check would make an autonomy setting able to
//! break status output.
//!
//! ## Honesty clause: "the guard is the only path" is NOT yet true
//!
//! `MemoryClient::profile_conn` no longer leaves the memory family: it is
//! `pub(in crate::memory)` with one caller, `MemoryClient::profile_store`,
//! which wraps it in a typed `ProfileStore`. Every SQL statement against
//! `user_profile` is now inside the family, and the compiler enforces that.
//! (`MemoryClient` and `ProfileStore` are the extracted engine's, not named
//! here as intra-doc links since this crate no longer depends on that crate
//! — see `crates/openhuman-core/src/memory/README.md` for the split.)
//!
//! **That is confinement, not policy** — any caller that still reaches
//! `ProfileStore` directly runs beneath every one of the seven steps above:
//! no tier check, no source scope, no taint, no redaction, no budget, no
//! audit.
//!
//! This note used to say the fix "needs a fourteenth family in
//! `tinycortex_api`". **That family now exists.** The contract has
//! `MemoryProfile` (`tinymemory_api::provider::profile`, eleven methods) and
//! [`families::GuardedProfile`] implements it, so the guarded door is built.
//! What remains is migrating any remaining direct `ProfileStore` caller onto
//! it, plus the release lag on
//! the module that serves it — `MemoryProfile` is one of the five families that
//! shipped in no released artifact until v1.2.0, so a caller moved onto it
//! before the registry re-pin would get a runtime `Unsupported`. The host-side
//! half-measure the note floated (having `ProfileStore` consult
//! [`policy::GuardPolicy`] directly) is no longer the only option and should not
//! be taken: it would make a `readonly` tier start rejecting learning-cache
//! rebuilds, which is a behaviour change rather than a refactor.
//!
//! A repo-wide search for `.profile_store(` (re-run for this note; see
//! `memory/bypass_allowlist_tests.rs` for the enforced allowlist) finds no
//! production caller left outside the memory family itself — the callers this
//! note used to list under `memory/sync/composio/providers/` and
//! `agent/learning/` are gone or moved onto the guarded surface. Confirm with
//! the same grep before relying on this being still true; the allowlist test
//! is the actual enforcement, this paragraph is not.

pub mod audit;
pub mod budget;
pub mod families;
/// In-memory provider fake for tests. Not `#[cfg(test)]` — integration tests
/// link the lib without it.
#[doc(hidden)]
pub mod in_memory;
mod mandatory;
pub mod policy;
pub mod provider;

#[cfg(test)]
pub(crate) mod test_support;

pub use policy::GuardPolicy;
pub use provider::MemoryGuard;
