//! `memory.provider_status` — what is bound in the memory subsystem slot
//! (`docs/specs/plan-memory.md` §5, `docs/specs/kernel.md` §6 item 6).
//!
//! This is the **memory adapter for status**: it resolves the context's
//! [`MemoryBinding`], converts it into the kernel's generic
//! [`BoundDriver`](crate::core::subsystem::BoundDriver) vocabulary, probes the
//! driver's live health, and projects the result onto the wire shape
//! [`SubsystemStatus`]. `subsystems.status` renders the same value for the
//! `memory` slot — deliberately, so the frontend can read driver capabilities
//! from the memory namespace without knowing the kernel namespace exists.
//!
//! Nothing here is a mutation and nothing here binds: a status call must never
//! be the thing that constructs a driver. It does resolve the binding, which
//! *is* lazily constructing on first use — that is
//! [`CoreContext::memory_binding`]'s existing cached behaviour and identical
//! to what any other memory RPC would trigger.

use crate::core::runtime::context::CoreContext;
use crate::core::subsystem::{DriverHealth, SubsystemStatus};
use crate::openhuman::memory::binding::{to_driver_health, MemoryBinding};
use crate::rpc::RpcOutcome;

/// The status of the memory slot for the current dispatch context.
///
/// Infallible by design: an unresolvable binding (no workspace bound, e.g. a
/// pre-login core) is *reported*, not raised. A status surface that errors
/// exactly when something is wrong is the opposite of useful.
///
/// A standalone CLI subcommand (`openhuman subsystems status`,
/// `openhuman memory status`) never builds a [`CoreContext`] — the generic
/// namespace dispatcher runs without one — so this resolves the configured
/// workspace's binding directly via [`standalone_status`] instead of reporting
/// an unresolved row. In an RPC host a context is always ambient, so that path
/// is unaffected.
pub async fn memory_subsystem_status() -> SubsystemStatus {
    match CoreContext::current().map(|ctx| ctx.memory_binding()) {
        Some(Ok(binding)) => status_from_binding(&binding).await,
        Some(Err(err)) => unresolved_status(err),
        None => standalone_status().await,
    }
}

/// Resolve status from the on-disk config when no [`CoreContext`] is ambient
/// (a bare CLI invocation). Reads the configured workspace's binding the same
/// way `cli_capability::bound_memory_driver_for` does, so `openhuman subsystems
/// status` shows the same resolved row as the table and as an RPC with a live
/// context.
///
/// Never errors: on a config load or bind failure this reports an unresolved
/// row (with a reason) rather than refusing to render — mirroring the
/// capability gate's default-OPEN posture, where a status command that refuses
/// to run because it cannot read config is worse than one that shows the
/// unresolved row.
async fn standalone_status() -> SubsystemStatus {
    let config = match crate::openhuman::config::Config::load_or_init().await {
        Ok(config) => config,
        Err(err) => {
            log::debug!(
                "[memory:provider] standalone status: config unresolved ({err}); reporting unresolved"
            );
            return unresolved_status(format!("no core context; config load failed: {err}"));
        }
    };

    match crate::openhuman::memory::binding::for_workspace(
        &config.workspace_dir,
        &config.subsystems.memory,
    ) {
        Ok(binding) => status_from_binding(&binding).await,
        Err(err) => {
            log::debug!(
                "[memory:provider] standalone status: binding unresolved ({err}); reporting unresolved"
            );
            unresolved_status(format!("no core context; binding failed: {err}"))
        }
    }
}

/// Project one resolved binding. Separate from [`memory_subsystem_status`] so
/// the bound case is testable without standing up a [`CoreContext`].
pub async fn status_from_binding(binding: &MemoryBinding) -> SubsystemStatus {
    let bound = binding.to_bound_driver();
    let health = to_driver_health(binding.unguarded_provider().health().await);
    let last_error = binding
        .fallback()
        .map(|fallback| format!("{}: {}", fallback.configured_driver, fallback.reason));

    log::debug!(
        "[memory:provider] status driver='{}' class={} health={} capabilities=[{}] fallback={}",
        bound.id,
        bound.class,
        health.as_str(),
        bound.capabilities.iter().collect::<Vec<_>>().join(","),
        bound.is_fallback()
    );

    SubsystemStatus::from_bound_with_health(&bound, health).with_last_error(last_error)
}

/// The status reported when no driver could be resolved at all. Distinct from
/// a *fallback*: nothing was bound, so there is no driver id to name and no
/// capability set to advertise.
fn unresolved_status(reason: String) -> SubsystemStatus {
    log::debug!("[memory:provider] status unresolved: {reason}");
    SubsystemStatus {
        slot: crate::core::subsystem::SubsystemSlot::Memory
            .as_str()
            .to_string(),
        driver: String::new(),
        class: crate::core::subsystem::DriverClass::Null
            .as_str()
            .to_string(),
        health: DriverHealth::down(reason.clone()).as_str().to_string(),
        health_reason: Some(reason.clone()),
        contract_version: crate::core::subsystem::format_contract_version(
            crate::openhuman::memory::api::CONTRACT_VERSION,
        ),
        capabilities: Vec::new(),
        fell_back_from: None,
        last_error: Some(reason),
    }
}

/// RPC handler body for `memory.provider_status`.
pub async fn memory_provider_status() -> RpcOutcome<SubsystemStatus> {
    RpcOutcome::new(memory_subsystem_status().await, vec![])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn status_without_a_context_reports_an_unresolved_slot() {
        let status = unresolved_status("no workspace".to_string());
        assert_eq!(status.slot, "memory");
        assert_eq!(status.class, "null");
        assert_eq!(status.health, "down");
        assert!(status.capabilities.is_empty());
        assert_eq!(status.last_error.as_deref(), Some("no workspace"));
        // Still reports the contract version this build speaks — that is a
        // build fact, independent of whether anything bound.
        assert_eq!(
            status.contract_version,
            crate::core::subsystem::format_contract_version(
                crate::openhuman::memory::api::CONTRACT_VERSION
            )
        );
    }

    #[tokio::test]
    async fn bound_driver_status_reports_id_class_contract_and_capabilities() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let cfg = crate::openhuman::config::schema::MemorySubsystemConfig::default();
        let binding = crate::openhuman::memory::binding::for_workspace(workspace.path(), &cfg)
            .expect("binding resolves");

        let status = status_from_binding(&binding).await;
        assert_eq!(status.slot, "memory");
        // The legacy default id is normalized to the compiled TinyMemory
        // module at the binding boundary.
        assert_eq!(status.driver, crate::openhuman::memory::binding::MODULE_ID);
        assert_eq!(status.class, "module");
        assert_eq!(status.health, "ready");
        assert_eq!(
            status.contract_version,
            crate::core::subsystem::format_contract_version(
                crate::openhuman::memory::api::CONTRACT_VERSION
            )
        );
        // The full eighteen families the pinned tinymemory artifact serves.
        // Previously this comment documented a narrower set (seventeen, then
        // thirteen) while the host under-claimed or over-claimed; the gap is
        // now closed: v1.4.0 serves all eighteen, `ModuleMemoryProvider`
        // implements all twenty accessors including `as_episodic`, so the
        // wire surface and the advertised set agree.
        // `modules::memory::ARTIFACT_CAPABILITIES` is the machine-checked
        // source of truth for what the pinned release serves (see its module
        // docs); this pins the same corrected boundary. Spelled out rather
        // than derived from `Capabilities::all()` on purpose: this is the
        // wire surface the frontend reads, so the strings themselves are the
        // assertion. A NEW contract family must still widen this deliberately,
        // together with the registry version bump — not silently.
        assert_eq!(
            status.capabilities,
            vec![
                // Widened to eighteen with the Episodic accessor, then twenty when
                // tinymemory v1.7.0 added SourceSync and CodingSessions —
                // landing — the archivist writes its turns and segments
                // through that family, so hiding it here would be the
                // under-claim. The list stays spelled out: a NEW contract
                // family must still widen this deliberately, with its
                // accessor and its release.
                "chunks",
                // v1.7.0's two families. Deliberately widened here rather than
                // derived: this list is the wire surface the frontend reads, so
                // a new family has to be a decision someone made, not something
                // that appeared.
                "coding_sessions",
                "core",
                "diff",
                "documents",
                "entities",
                "episodic",
                "goals",
                "graph",
                "ingest",
                "maintenance",
                "people",
                "portability",
                "profile",
                "recall",
                "retrieval",
                // Added with tinymemory v1.13.0: the MemoryScoring bus family
                // (#5560). Widened deliberately — the wire surface the frontend
                // reads must be an explicit decision, not a silent addition.
                "scoring",
                "source_sync",
                "sources",
                "tool_memory",
                "tree"
            ]
        );
        assert_eq!(status.fell_back_from, None);
        assert_eq!(status.last_error, None);
    }

    #[tokio::test]
    async fn a_refused_driver_reports_the_fallback_and_its_reason() {
        let workspace = tempfile::tempdir().expect("tempdir");
        let mut cfg = crate::openhuman::config::schema::MemorySubsystemConfig {
            driver: "supermemory".into(),
            ..Default::default()
        };
        cfg.drivers.insert(
            "supermemory".into(),
            crate::openhuman::config::schema::MemoryDriverConfig {
                class: Some("external".into()),
                transport: Some("http".into()),
                endpoint: Some("https://api.supermemory.ai".into()),
                credential_ref: Some("keychain:supermemory".into()),
                trust_state: "untrusted".into(),
            },
        );
        let binding = crate::openhuman::memory::binding::for_workspace(workspace.path(), &cfg)
            .expect("binding falls back rather than failing");

        let status = status_from_binding(&binding).await;
        assert_eq!(status.driver, "null");
        assert_eq!(status.class, "null");
        assert_eq!(status.fell_back_from.as_deref(), Some("supermemory"));
        let last_error = status.last_error.expect("a refused bind records why");
        assert!(last_error.contains("supermemory"), "{last_error}");
        assert!(last_error.contains("untrusted"), "{last_error}");
        // The refusal reason must never leak the credential reference or the
        // endpoint — same rule the binding's own tests pin.
        assert!(!last_error.contains("keychain:supermemory"), "{last_error}");
        assert!(!last_error.contains("api.supermemory.ai"), "{last_error}");
    }

    #[tokio::test]
    async fn provider_status_wraps_the_snapshot_with_no_logs() {
        let outcome = memory_provider_status().await;
        assert!(outcome.logs.is_empty());
        assert_eq!(outcome.value.slot, "memory");
    }
}
