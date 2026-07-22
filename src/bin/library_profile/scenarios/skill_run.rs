//! `skill-run`: the true, process-*tree* cost of a skill step that executes on
//! a real language runtime — the interpreter child process included.
//!
//! ## What it actually runs
//!
//! A skill run's orchestrator (`spawn_workflow_run_background` → the
//! `orchestrator` agent) deliberately owns **no** `node_exec` tool: the
//! chat-tier orchestrator never executes code itself, it delegates every code
//! step to the `code_executor` specialist (the only builtin whose allow-list
//! carries `node_exec` / `npm_exec`). So the agent that genuinely spawns the
//! language runtime *is* `code_executor`. This scenario drives that specialist
//! directly — one turn, one scripted `node_exec` call — which is the real
//! node-executing path, not a bare `std::process` spawn. Measuring the
//! orchestrator→specialist delegation on top would add in-process agent cost
//! without changing the runtime-child cost this scenario exists to capture;
//! the compromise is documented here on purpose.
//!
//! The mock ([`SkillRunMock`]) emits a `node_exec` call whose inline JavaScript
//! does real work, allocates, and busy-waits ~1.2 s so the child stays resident
//! long enough for the harness tree sampler (15 ms poll) to attribute it, then
//! prints JSON carrying [`NODE_MARKER`]. When that output rides back into the
//! turn the mock returns a plain final answer and the turn completes.
//!
//! ## No interpreter download
//!
//! `node.prefer_system = true` (the default) means a host `node` whose **major**
//! matches the configured target is reused rather than downloaded. This
//! scenario **requires** a system `node` and bails with a clear stderr error +
//! nonzero exit if none is on `PATH` — it must never pull a runtime.
//!
//! The measured cost lands in `result.tree` (`tree_rss_kib`, `child_count`,
//! per-child RSS) — captured at the workload peak, since the `node` child has
//! already exited by settle time.

use std::sync::Arc;

use anyhow::Result;
use openhuman_core::core::event_bus::init_global;
use openhuman_core::openhuman::agent::harness::AgentDefinitionRegistry;
use openhuman_core::openhuman::agent::Agent;
use openhuman_core::openhuman::inference::provider::factory::test_provider_override;
use openhuman_core::openhuman::inference::provider::Provider;
use openhuman_core::openhuman::security::AutonomyLevel;

use crate::harness::{fixture, measure_with_tree, EnvGuard, ProfileResult};
use crate::mock::SkillRunMock;

/// The specialist agent that owns `node_exec` and spawns the runtime child.
const CODE_AGENT: &str = "code_executor";

/// Probe for a usable system `node`. Returns its version on success; on failure
/// prints a clear `[library-profile]` stderr error and returns `Err` (which
/// propagates to a nonzero process exit). We must NOT download an interpreter.
fn require_system_node() -> Result<String> {
    match std::process::Command::new("node").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            eprintln!("[library-profile] skill-run: using system node {version}");
            Ok(version)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!(
                "[library-profile] skill-run: `node --version` failed (status {:?}): {stderr}",
                output.status.code()
            );
            anyhow::bail!("skill-run requires a working system `node`, but `node --version` failed")
        }
        Err(err) => {
            eprintln!(
                "[library-profile] skill-run: `node` not found on PATH: {err}. Install Node.js — \
                 the profiler will NOT download an interpreter."
            );
            anyhow::bail!("skill-run requires a system `node` on PATH; none found")
        }
    }
}

pub async fn run() -> Result<ProfileResult> {
    // Hard requirement: a system node must be present (no download).
    require_system_node()?;

    let mut fixture = fixture()?;
    // `node_exec` is a Write-class acting tool. Full autonomy keeps the gate
    // from parking the turn on approval; the gate is also opted out explicitly.
    fixture.config.autonomy.level = AutonomyLevel::Full;
    let _approval_env = EnvGuard::set("OPENHUMAN_APPROVAL_GATE", "0");

    let _ = init_global(256);
    openhuman_core::openhuman::agent::bus::register_agent_handlers();
    let _ = AgentDefinitionRegistry::init_global_builtins();

    let mock = SkillRunMock::new();
    let provider: Arc<dyn Provider> = mock.clone();
    let _provider = test_provider_override::install(provider);
    eprintln!(
        "[library-profile] skill-run: registries ready, node_exec mock installed \
         (agent={CODE_AGENT})"
    );

    let mock_for_workload = mock.clone();
    let config = fixture.config.clone();
    let mut result = measure_with_tree("skill-run", 1, None, move |rec| async move {
        rec.checkpoint("turn-start")?;
        let mut agent = Agent::from_config_for_agent(&config, CODE_AGENT)?;
        let reply = agent
            .run_single(
                "Run a short JavaScript computation with node_exec and report the JSON it prints.",
            )
            .await?;
        rec.checkpoint("turn-done")?;
        anyhow::ensure!(!reply.trim().is_empty(), "empty code_executor reply");
        anyhow::ensure!(
            mock_for_workload.node_call_emitted(),
            "the node_exec tool call was never emitted"
        );
        anyhow::ensure!(
            mock_for_workload.node_output_seen(),
            "the node child's output never flowed back — the interpreter child did not run/print"
        );
        Ok(())
    })
    .await?;

    match result.tree.as_ref() {
        Some(tree) if tree.child_count >= 1 => {
            eprintln!(
                "[library-profile] skill-run: captured tree_rss_kib={} child_count={} \
                 children={:?}",
                tree.tree_rss_kib, tree.child_count, tree.children
            );
        }
        Some(tree) => {
            eprintln!(
                "[library-profile] skill-run: WARNING tree captured but no child was resident at \
                 peak (tree_rss_kib={}). The node child may have been too short-lived; \
                 the busy-wait should have kept it alive.",
                tree.tree_rss_kib
            );
        }
        None => {
            eprintln!("[library-profile] skill-run: WARNING no process-tree sample captured");
        }
    }

    // Fold in scenario-visible fields (schema stays additive).
    result.workload_units = 1;
    Ok(result)
}
