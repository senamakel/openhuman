//! Owner identity discovery agent.
//!
//! > **Status: scaffolded, Apify integration stubbed.** The trait +
//! > runner + event-bus wiring are all in place so Phase 3 can be
//! > reactivated later, but the Apify backend-proxy client has been
//! > removed for now. Today, owner identity is populated only by
//! > **Phase 1** (skills calling `memory.updateOwner` from JavaScript).
//! > The subscriber is registered but disabled by default via
//! > `DiscoveryConfig::enabled = false`, so no background LLM calls
//! > fire on skill connect.
//!
//! # What still runs
//!
//! - `register_discovery_subscriber()` is called from startup, so the
//!   handler is live on the event bus.
//! - When the feature is disabled (the default), the subscriber logs
//!   and no-ops. When re-enabled, it invokes [`run_discovery`] with a
//!   stub Apify client that always returns empty dataset items — the
//!   LLM can still write facts via `owner_write` but the research
//!   tools are no-ops until the real wiring returns.
//!
//! # What the full flow will look like
//!
//! ```text
//!     SkillOAuthCompleted (skill_id, state_snapshot)
//!              │
//!              ▼
//!       DiscoverySubscriber
//!              │
//!              │  (debounce via KV)
//!              ▼
//!         run_discovery(job)
//!              │
//!              ├──► LlmClient: bounded chat loop
//!              │       │
//!              │       ├──► owner_write(facts, document?)
//!              │       │      → MemoryClient::profile_upsert_owner
//!              │       │      → MemoryClient::store_owner_doc
//!              │       │
//!              │       ├──► apify_search_person(name, email?)  [stub]
//!              │       └──► apify_fetch_url(url)                [stub]
//!              │
//!              ▼
//!         DiscoveryReport
//! ```
//!
//! # Testing
//!
//! Both external dependencies are behind traits:
//! - [`ApifyClient`] — real wiring will return here in a follow-up
//! - [`LlmClient`] — wrapper over the backend's chat-completions endpoint
//!
//! Integration tests in `tests/owner_discovery_test.rs` inject stubs
//! for both so the test never touches the network, and they verify
//! the runner logic end-to-end independently of whether the runner is
//! currently invoked from the event bus in production.
//!
//! # Data flow
//!
//! ```text
//!     SkillOAuthCompleted (skill_id, state_snapshot)
//!              │
//!              ▼
//!       DiscoverySubscriber
//!              │
//!              │  (debounce via KV)
//!              ▼
//!         run_discovery(job)
//!              │
//!              ├──► LlmClient: bounded chat loop
//!              │       │
//!              │       ├──► owner_write(facts, document?)
//!              │       │      → MemoryClient::profile_upsert_owner
//!              │       │      → MemoryClient::store_owner_doc
//!              │       │
//!              │       ├──► apify_search_person(name, email?)
//!              │       └──► apify_fetch_url(url)
//!              │
//!              ▼
//!         DiscoveryReport
//! ```

pub mod apify;
pub mod bus;
pub mod llm;
pub mod runner;
pub mod tools;
pub mod types;

pub use apify::{ApifyClient, ApifyError, StubApifyClient};
pub use bus::{register_discovery_subscriber, DiscoverySubscriber};
pub use llm::{LlmClient, LlmError, LlmMessage, LlmResponse, LlmToolCall};
pub use runner::{run_discovery, DiscoveryDeps};
pub use types::{DiscoveryError, DiscoveryJob, DiscoveryReport, DiscoveryTrigger, SeedFact};
