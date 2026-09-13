//! Post-run memory digest: [`FlowRunDigestSubscriber`] writes a compact,
//! bounded summary of every successful run into the flow's private memory
//! namespace so later runs can recall what they already did.

use crate::config::Config;
use crate::core::events::DomainEvent;
use crate::flows::store;
use crate::flows::{flow_namespace, FlowRun};
use async_trait::async_trait;
use std::sync::Arc;
use tinybus::EventHandler;
use tinymemory_api::provider::MemoryCore;
use tinymemory_api::types::{MemoryCategory, MemoryTaint};

/// Bounds a post-run memory digest to a compact, LLM-cheap size — a single
/// run's summary must never dominate a later `flow_memory_recall`.
pub(super) const DIGEST_MAX_CHARS: usize = 1000;

/// Cap on how many `run_digest:*` entries [`FlowRunDigestSubscriber`] keeps
/// per flow's memory namespace before pruning the oldest.
const DIGEST_RETENTION_CAP: usize = 50;

/// Listens for `DomainEvent::FlowRunFinished` and, on a successful terminal
/// status, writes a compact digest of the run into the flow's own private
/// memory namespace ([`flow_namespace`]) — e.g. so a later run of the same
/// scheduled digest flow can `flow_memory_recall` what it already sent
/// without re-deriving that from the target service.
///
/// Success-only: `"failed"` / `"cancelled"` / `"interrupted"` / any other
/// terminal status is ignored, since a digest of a run that didn't actually
/// complete its work would misleadingly look like a record of real output.
///
/// Best-effort throughout: every failure here is logged via `tracing::warn!`
/// and swallowed, never propagated — by the time this subscriber observes
/// `FlowRunFinished`, the run has already settled its own `flow_runs` row, so
/// a memory-layer hiccup must never retroactively affect run status.
pub struct FlowRunDigestSubscriber {
    config: Arc<Config>,
    /// Test-only memory override. In production this is `None` and the digest
    /// resolves the process-global memory client via [`active_memory_client`].
    /// The process-global client is a one-shot `OnceLock`, so a unit test
    /// cannot reliably rebind it to its own tempdir (an earlier test in the
    /// same binary may already have initialised the singleton — see
    /// `memory::global`'s own test notes). Injecting a directly-constructed
    /// [`Memory`] here lets the digest tests write and read back through the
    /// SAME instance deterministically, exactly as `flows::memory_tools`'
    /// tests do with `UnifiedMemory::new`.
    memory_override: Option<Arc<crate::memory::guard::MemoryGuard>>,
}

impl FlowRunDigestSubscriber {
    pub fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            memory_override: None,
        }
    }

    /// Test constructor: run the digest against an explicitly-provided memory
    /// instance instead of the process-global client. See [`Self::memory_override`].
    #[cfg(test)]
    pub(super) fn with_memory(
        config: Arc<Config>,
        memory: Arc<crate::memory::guard::MemoryGuard>,
    ) -> Self {
        Self {
            config,
            memory_override: Some(memory),
        }
    }

    /// Resolves the memory handle the digest writes to: the injected test
    /// override when present, else the process-global client
    /// ([`active_memory_client`]). Returns `None` (best-effort skip) when the
    /// global client is unavailable.
    async fn resolve_memory(&self) -> Option<Arc<crate::memory::guard::MemoryGuard>> {
        if let Some(memory) = &self.memory_override {
            return Some(memory.clone());
        }
        // The guarded driver, not the raw engine client. The digest writes
        // through the policy layer like every other write.
        match crate::memory::ops::guard::active_memory_guard().await {
            Ok(guard) => Some(guard),
            Err(e) => {
                tracing::warn!(target: "flows", error = %e, "[flows] digest: memory unavailable — skipping");
                None
            }
        }
    }

    async fn handle_finished(&self, flow_id: &str, run_id: &str, status: &str) {
        if status != "completed" && status != "completed_with_warnings" {
            tracing::trace!(target: "flows", %flow_id, %run_id, %status, "[flows] digest: ignoring non-success terminal status");
            return;
        }

        let flow_name = match store::get_flow(&self.config, flow_id) {
            Ok(Some(flow)) => flow.name,
            Ok(None) => {
                tracing::debug!(target: "flows", %flow_id, %run_id, "[flows] digest: flow no longer exists — skipping");
                return;
            }
            Err(e) => {
                tracing::warn!(target: "flows", %flow_id, %run_id, error = %e, "[flows] digest: failed to load flow — skipping");
                return;
            }
        };

        let run = match store::get_flow_run(&self.config, run_id) {
            Ok(Some(run)) => run,
            Ok(None) => {
                tracing::warn!(target: "flows", %flow_id, %run_id, "[flows] digest: run row not found — skipping");
                return;
            }
            Err(e) => {
                tracing::warn!(target: "flows", %flow_id, %run_id, error = %e, "[flows] digest: failed to load run — skipping");
                return;
            }
        };

        let digest = render_run_digest(&flow_name, &run);

        let Some(memory) = self.resolve_memory().await else {
            return;
        };
        let namespace = flow_namespace(flow_id);
        let digest_key = format!("run_digest:{run_id}");

        // `store` carries the taint on the contract, so the separate
        // `store_with_taint` door the engine trait needed is gone. The guard
        // still stamps the effective value — `ExternalSync` here is the
        // request, and it is the honest one: a digest is machine-generated
        // from a flow run, not user-authored.
        if let Err(e) = memory
            .store(
                &namespace,
                &digest_key,
                &digest,
                MemoryCategory::Core,
                None,
                MemoryTaint::ExternalSync,
            )
            .await
        {
            tracing::warn!(target: "flows", %flow_id, %run_id, %namespace, error = %e, "[flows] digest: failed to write run digest");
            return;
        }

        self.enforce_retention_cap(&memory, &namespace).await;
    }

    /// Best-effort prune: keeps at most [`DIGEST_RETENTION_CAP`] `run_digest:*`
    /// entries per flow namespace, evicting the oldest (by `timestamp`) first.
    async fn enforce_retention_cap(
        &self,
        memory: &Arc<crate::memory::guard::MemoryGuard>,
        namespace: &str,
    ) {
        let entries = match memory.list(Some(namespace), None, None).await {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(target: "flows", %namespace, error = %e, "[flows] digest: retention sweep failed to list namespace");
                return;
            }
        };
        let mut digests: Vec<_> = entries
            .into_iter()
            .filter(|entry| entry.key.starts_with("run_digest:"))
            .collect();
        if digests.len() <= DIGEST_RETENTION_CAP {
            return;
        }
        // Oldest first, so the excess taken below is the stalest entries.
        digests.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        let excess = digests.len() - DIGEST_RETENTION_CAP;
        for entry in digests.into_iter().take(excess) {
            if let Err(e) = memory.forget(namespace, &entry.key).await {
                tracing::warn!(target: "flows", %namespace, key = %entry.key, error = %e, "[flows] digest: retention sweep failed to forget stale entry");
            }
        }
    }
}

#[async_trait]
impl EventHandler<DomainEvent> for FlowRunDigestSubscriber {
    fn name(&self) -> &str {
        "flows::digest"
    }

    fn domains(&self) -> Option<&[&str]> {
        // `FlowRunFinished` — the only event this subscriber handles — is
        // itself tagged `"cron"` by `DomainEvent::domain()` (grouped there
        // with the other flow-run/schedule events), not `"flows"`. This is
        // matching that tag, not a typo.
        Some(&["cron"])
    }

    async fn handle(&self, event: &DomainEvent) {
        if let DomainEvent::FlowRunFinished {
            flow_id,
            run_id,
            status,
        } = event
        {
            self.handle_finished(flow_id, run_id, status).await;
        }
    }
}

/// Truncates `s` to at most `max` `char`s, appending `…` when truncated.
pub(super) fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}

/// Composes a compact, bounded summary of a finished run: flow name,
/// finished-at, status, node count, and per-node status + truncated output.
/// Bounded to [`DIGEST_MAX_CHARS`] total.
pub(super) fn render_run_digest(flow_name: &str, run: &FlowRun) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "Flow: {flow_name}");
    let _ = writeln!(out, "Status: {}", run.status);
    if let Some(finished_at) = &run.finished_at {
        let _ = writeln!(out, "Finished: {finished_at}");
    }
    let _ = writeln!(out, "Nodes: {}", run.steps.len());
    for step in &run.steps {
        if out.chars().count() >= DIGEST_MAX_CHARS {
            break;
        }
        let status = step.status.as_deref().unwrap_or("?");
        let output = truncate_chars(&step.output.to_string(), 120);
        let _ = writeln!(out, "- {} [{status}]: {output}", step.node_id);
    }
    truncate_chars(&out, DIGEST_MAX_CHARS)
}
