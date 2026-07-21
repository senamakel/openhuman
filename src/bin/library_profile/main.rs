//! Hermetic, Rust-only library profiling workloads.
//!
//! This binary never enters shipped builds (it requires the default-off
//! `rss-bench` feature). It measures production code paths in fresh processes
//! with network inference replaced by a deterministic provider.
//!
//! Scenarios (`library-profile <scenario>`):
//! - `memory-ingest` — ingest 100 chat messages, drain the memory queue.
//! - `subagents`     — one orchestrator turn spawning two real researchers.
//! - `agent-turn`    — a single cold agent turn (minimal library unit).
//! - `long-agent`    — N warmed sequential turns with a per-turn checkpoint series.
//! - `workflow`      — a real flows trigger->transform->agent graph, end to end.
//! - `subconscious`  — one promoted subconscious turn WITHOUT delegation.
//! - `cold-phases`   — per-phase checkpoints of the cold bootstrap in one region.
//!
//! stdout is ALWAYS a single pretty JSON object (the pinned schema in
//! `harness::ProfileResult`); every diagnostic goes to stderr with the stable
//! `[library-profile]` prefix.
//!
//! With the `rss-bench-dhat` feature, dhat's global allocator + profiler are
//! active: RSS/time numbers are perturbed, the result carries `"dhat": true`,
//! and a `dhat-<scenario>.json` heap profile is written under
//! `target/profile/rust-library/` (override via `OPENHUMAN_PROFILE_DHAT_OUT`).

mod harness;
mod mock;
mod scenarios;

use std::time::Duration;

use anyhow::{Context, Result};

use harness::ProfileResult;

#[cfg(feature = "rss-bench-dhat")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Builds the dhat profiler (feature-gated), writing to the requested path.
/// Kept alive by the caller until after the JSON result is printed.
#[cfg(feature = "rss-bench-dhat")]
fn start_dhat(scenario: &str) -> Result<dhat::Profiler> {
    let out = std::env::var("OPENHUMAN_PROFILE_DHAT_OUT")
        .unwrap_or_else(|_| format!("target/profile/rust-library/dhat-{scenario}.json"));
    if let Some(parent) = std::path::Path::new(&out).parent() {
        std::fs::create_dir_all(parent).context("create dhat output directory")?;
    }
    eprintln!("[library-profile] dhat active — heap profile -> {out}");
    Ok(dhat::Profiler::builder().file_name(&out).build())
}

async fn dispatch(scenario: &str) -> Result<ProfileResult> {
    match scenario {
        "memory-ingest" => scenarios::memory_ingest::run().await,
        "subagents" => scenarios::subagents::run().await,
        "agent-turn" => scenarios::agent_turn::run().await,
        "long-agent" => scenarios::long_agent::run().await,
        "workflow" => scenarios::workflow::run().await,
        "subconscious" => scenarios::subconscious::run().await,
        "cold-phases" => scenarios::cold_phases::run().await,
        other => anyhow::bail!("unknown scenario: {other}"),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let scenario = std::env::args().nth(1).context(
        "usage: library-profile \
         <memory-ingest|subagents|agent-turn|long-agent|workflow|subconscious|cold-phases>",
    )?;

    // Profiler must outlive the whole run + the JSON print so its Drop writes
    // the complete heap profile last.
    #[cfg(feature = "rss-bench-dhat")]
    let _dhat = start_dhat(&scenario)?;

    eprintln!(
        "[library-profile] pid={} scenario={scenario} start",
        std::process::id()
    );
    #[cfg_attr(not(feature = "rss-bench-dhat"), allow(unused_mut))]
    let mut result = dispatch(&scenario).await?;

    #[cfg(feature = "rss-bench-dhat")]
    {
        result.dhat = Some(true);
    }

    println!("{}", serde_json::to_string_pretty(&result)?);

    if let Some(seconds) = std::env::var("OPENHUMAN_PROFILE_HOLD_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
    {
        eprintln!(
            "[library-profile] pid={} holding for {seconds}s",
            std::process::id()
        );
        tokio::time::sleep(Duration::from_secs(seconds)).await;
    }
    Ok(())
}
