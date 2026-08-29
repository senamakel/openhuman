//! `cold-phases`: sequential per-phase checkpoints of the cold bootstrap, all
//! inside one measured region. Each phase is sampled right after it completes
//! so the JSON `checkpoints` series attributes the cold-start cost per phase.
//!
//! # Why this still names the engine crate (#5560)
//!
//! Phase (e) exists to time **opening the SQLite memory store**, and
//! `MemoryClient` is the thing that opens it. `memory::binding::for_config`
//! would resolve the null driver in this binary — no module is loaded — so the
//! checkpoint would report the cost of binding a driver that opens nothing, and
//! the cold-start series would silently lose its heaviest I/O phase rather than
//! gain a migration. `MemoryClient` is also `tinymemory-core`'s own type, not a
//! re-export of a TinyCortex one, so there is no path swap available either.
//!
//! Same conclusion as the sibling `memory_ingest` scenario: this is a
//! **feature-gate** case, not a migration. See that module's note for what the
//! manifest needs (`tinymemory-core` optional, enabled by `rss-bench`).

use std::time::Duration;

use anyhow::Result;
use openhuman_core::openhuman::agent::harness::AgentDefinitionRegistry;
use openhuman_core::openhuman::agent::Agent;
use openhuman_core::openhuman::inference::provider::factory::test_provider_override;
use tinymemory_core::store::MemoryClient;

use crate::harness::{fixture, measure, ProfileResult};
use crate::mock::PlainTextMock;

/// config, event-bus, agent-registry, detectors, memory-store, agent-build,
/// first-turn, warm-turn, teardown.
const PHASE_COUNT: usize = 9;

pub async fn run() -> Result<ProfileResult> {
    measure("cold-phases", PHASE_COUNT, None, |rec| async move {
        // a. config — hermetic fixture parse (see deviation note in the report:
        //    kept as fixture parsing rather than `Config::load_or_init` to
        //    guarantee we never touch the real ~/.openhuman).
        let fixture = fixture()?;
        rec.checkpoint("config-parse")?;

        // b. event-bus (plus agent-handler registration so turns can run).
        openhuman_core::core::bus::init().await.expect("bus init");
        openhuman_core::openhuman::agent::bus::register_agent_handlers();
        rec.checkpoint("event-bus")?;

        // c. agent-registry.
        let _ = AgentDefinitionRegistry::init_global_builtins();
        rec.checkpoint("agent-registry")?;

        // d. detectors — force the lazy PII + prompt-injection statics.
        let _ = openhuman_core::openhuman::security::pii::scan("");
        let _ =
            openhuman_core::openhuman::security::prompt_injection::scan_tool_definition("x", "");
        rec.checkpoint("detectors")?;

        // e. memory-store — build and hold a unified-memory client until teardown.
        let mem = MemoryClient::from_workspace_dir(fixture.config.workspace_dir.clone())
            .map_err(anyhow::Error::msg)?;
        rec.checkpoint("memory-store")?;

        // Model mock for the two turns below (not itself a phase).
        let mock = PlainTextMock::new("Phoenix migration is healthy and on track.");
        let _provider = test_provider_override::install_model(mock.clone());

        // f. agent-build.
        let mut agent = Agent::from_config_for_agent(&fixture.config, "subconscious")?;
        rec.checkpoint("agent-build")?;

        // g. first-turn (cold).
        let first = agent
            .run_single("Give me a one-line status on the Phoenix migration.")
            .await?;
        anyhow::ensure!(!first.trim().is_empty(), "empty first-turn reply");
        rec.checkpoint("first-turn")?;

        // h. warm-turn (second, same agent).
        let warm = agent.run_single("Any change since the last check?").await?;
        anyhow::ensure!(!warm.trim().is_empty(), "empty warm-turn reply");
        rec.checkpoint("warm-turn")?;

        // i. teardown — drop the agent + memory client, settle, sample.
        drop(agent);
        drop(mem);
        tokio::time::sleep(Duration::from_millis(300)).await;
        rec.checkpoint("teardown")?;
        Ok(())
    })
    .await
}
