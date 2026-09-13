use super::*;
use std::path::PathBuf;

fn ctx(dir: &str) -> Arc<CoreContext> {
    Arc::new(CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(PathBuf::from(dir)),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    })
}

// The ambient-scope primitive is the mechanism Phase 3 multi-tenant
// isolation is built on: a dispatch scoped to context A must see A's state,
// not the process default or another tenant's. These assert the primitive
// directly (independent of the process DEFAULT_CONTEXT global, since
// `current()` inside a scope resolves the scoped value).

// ---- embedder-supplied config (the library-embedding seam) ---------------
//
// `CoreBuilder::config(..)` is only half of the story, and the half that is
// easy to get wrong. Setting the config at boot does NOT reach RPC handlers:
// they call `load_config_with_timeout()` per dispatch, which re-runs
// `Config::load_or_init()` and re-resolves the process-global workspace. The
// context has to carry it, and the loader has to prefer it, or an embedder
// configures boot and watches its turns run somewhere else entirely.

fn ctx_with_config(config: crate::config::Config) -> Arc<CoreContext> {
    Arc::new(CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(config.workspace_dir.clone()),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: Some(config),
    })
}

#[test]
fn a_context_without_an_embedder_config_reports_none() {
    // The default for every host that lets the core discover its own
    // config, which is all of them but a library embedder.
    assert!(ctx("/tmp/ws").embedder_config().is_none());
}

#[test]
fn an_embedder_config_is_readable_from_the_context() {
    let mut config = crate::config::Config::default();
    config.workspace_dir = PathBuf::from("/tmp/embedder-ws");
    config.default_model = Some("embedder-model".into());

    let ctx = ctx_with_config(config);
    let read = ctx.embedder_config().expect("supplied config is readable");
    assert_eq!(read.workspace_dir, PathBuf::from("/tmp/embedder-ws"));
    assert_eq!(read.default_model.as_deref(), Some("embedder-model"));
}

#[tokio::test]
async fn the_current_dispatch_sees_the_scoped_embedder_config() {
    // This is the read path `load_config_with_timeout` uses. If it resolved
    // to the process default instead of the scoped context, a second
    // embedder in the same process would silently serve the first's config.
    let mut config = crate::config::Config::default();
    config.workspace_dir = PathBuf::from("/tmp/scoped-ws");
    config.default_model = Some("scoped-model".into());

    let scoped = CoreContext::scope(ctx_with_config(config), async {
        CoreContext::current_embedder_config()
    })
    .await;

    let scoped = scoped.expect("a scoped embedder config is visible to the dispatch");
    assert_eq!(scoped.default_model.as_deref(), Some("scoped-model"));
    assert_eq!(scoped.workspace_dir, PathBuf::from("/tmp/scoped-ws"));
}

// ---- store-init gating (#4796 DoD item 3) --------------------------------
// `init_stores` side-effects on process globals with no init-state probe, so
// the gating is proven via the pure `StoreInitPlan` the registrar consumes.

#[test]
fn store_init_plan_full_initializes_every_store() {
    let plan = StoreInitPlan::for_domains(crate::core::runtime::DomainSet::full());
    assert_eq!(
        plan,
        StoreInitPlan {
            memory: true,
            agent_attachments: true,
            skills_prune: true,
        },
        "full() must initialize every workspace-bound store"
    );
}

#[test]
fn store_init_plan_none_initializes_nothing() {
    let plan = StoreInitPlan::for_domains(crate::core::runtime::DomainSet::none());
    assert_eq!(
        plan,
        StoreInitPlan {
            memory: false,
            agent_attachments: false,
            skills_prune: false,
        },
        "none() must leave every workspace-bound store uninitialized"
    );
}

#[test]
fn store_init_plan_harness_gates_by_owning_group() {
    let plan = StoreInitPlan::for_domains(crate::core::runtime::DomainSet::harness());
    // harness() = agent + memory + threads + config + security.
    assert!(plan.memory, "harness keeps the memory binding (Memory)");
    assert!(
        plan.agent_attachments,
        "harness keeps agent attachments sidecar (Agent)"
    );
    // Skills is NOT in harness → its store work stays off.
    assert!(
        !plan.skills_prune,
        "harness must skip skills legacy-prune (Skills)"
    );
}

#[tokio::test]
async fn scope_sets_current_context() {
    let a = ctx("/tmp/ctx-a");
    let seen = CoreContext::scope(a, async {
        CoreContext::current().map(|c| c.workspace_dir().unwrap())
    })
    .await;
    assert_eq!(seen, Some(PathBuf::from("/tmp/ctx-a")));
}

#[tokio::test]
async fn propagate_carries_scoped_context_into_spawned_task() {
    let a = ctx("/tmp/ctx-propagated");
    let seen = CoreContext::scope(a, async {
        tokio::spawn(CoreContext::propagate(async {
            CoreContext::current().unwrap().workspace_dir().unwrap()
        }))
        .await
        .unwrap()
    })
    .await;
    assert_eq!(seen, PathBuf::from("/tmp/ctx-propagated"));
}

#[tokio::test]
async fn scoped_context_exposes_its_domain_set() {
    // The ambient `current().domains()` must reflect the scoped context's
    // DomainSet — this is the seam the registry filter reads (#4796).
    let harness = crate::core::runtime::DomainSet::harness();
    let ctx = CoreContext::for_test(harness, Some(PathBuf::from("/tmp/ctx-domains")), None);
    let seen = CoreContext::scope(ctx, async { CoreContext::current().map(|c| c.domains()) }).await;
    assert_eq!(seen, Some(harness));
    assert!(seen.unwrap().allows(crate::core::all::DomainGroup::Memory));
    assert!(!seen.unwrap().allows(crate::core::all::DomainGroup::Web3));
}

#[tokio::test]
async fn nested_scope_overrides_then_restores() {
    let a = ctx("/tmp/ctx-a");
    let b = ctx("/tmp/ctx-b");
    let (inner, outer) = CoreContext::scope(a, async {
        let inner = CoreContext::scope(b, async {
            CoreContext::current().unwrap().workspace_dir().unwrap()
        })
        .await;
        let outer = CoreContext::current().unwrap().workspace_dir().unwrap();
        (inner, outer)
    })
    .await;
    // Inner dispatch sees tenant B; the outer scope is restored to A after.
    assert_eq!(inner, PathBuf::from("/tmp/ctx-b"));
    assert_eq!(outer, PathBuf::from("/tmp/ctx-a"));
}

// The Phase 3 exit criterion, at the store level: two contexts over distinct
// workspaces resolve isolated per-domain stores, and one context always
// The three people-based context tests that stood here are gone with
// `CoreContext::people()`. They proved per-context workspace isolation
// using the people store as the example, and that property is proved
// unchanged by `memory_binding_is_isolated_per_context_workspace` and
// `rebind_workspace_updates_context_memory_binding` below — which is what
// people now resolves through. The third,
// `people_rpc_uses_scoped_context_store`, asserted that a scoped
// `people_resolve` wrote workspace A and not B by reading both stores
// directly; there is no second reader to check against any more, and the
// isolation it tested is the binding's.

#[test]
fn degraded_context_rejects_workspace_bound_stores() {
    let ctx = CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: None,
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    };

    // `workspace_dir()` is the gate every workspace-bound store goes
    // through, so it is asserted directly. This used to go through
    // `CoreContext::people()`, which was simply the first such store; it
    // resolves through the memory binding now and no longer exists.
    let err = match ctx.workspace_dir() {
        Ok(_) => panic!("degraded context unexpectedly resolved a workspace"),
        Err(err) => err,
    };
    assert!(
        err.contains("workspace unavailable"),
        "unexpected error: {err}"
    );
}

// ---- memory driver binding (M2b) ----------------------------------------

fn untrusted_external_memory_cfg() -> crate::config::schema::MemorySubsystemConfig {
    use crate::config::schema::{MemoryDriverConfig, MemorySubsystemConfig};
    let mut cfg = MemorySubsystemConfig {
        driver: "supermemory".into(),
        ..Default::default()
    };
    cfg.drivers.insert(
        "supermemory".into(),
        MemoryDriverConfig {
            class: Some("external".into()),
            ..Default::default()
        },
    );
    cfg
}

/// Same proof as `people_store_is_isolated_per_context_workspace`, one layer
/// up: the memory binding is per-workspace, not per-process.
#[test]
fn memory_binding_is_isolated_per_context_workspace() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = Arc::new(CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(dir_a.path().to_path_buf()),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    });
    let b = Arc::new(CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(dir_b.path().to_path_buf()),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    });

    let bind_a = a.memory_binding().expect("bind workspace A");
    let bind_b = b.memory_binding().expect("bind workspace B");
    assert!(!Arc::ptr_eq(&bind_a, &bind_b));

    let bind_a_again = a.memory_binding().expect("re-resolve workspace A");
    assert!(Arc::ptr_eq(&bind_a, &bind_a_again));
}

/// The per-workspace rebinding requirement, proven without any explicit
/// "rebind memory" call: switching the active user re-points
/// `workspace_dir`, and the accessor keys on that.
#[test]
fn rebind_workspace_updates_context_memory_binding() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let ctx = CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(dir_a.path().to_path_buf()),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    };

    let bind_a = ctx.memory_binding().expect("bind workspace A");
    ctx.rebind_workspace(dir_b.path(), Default::default())
        .expect("rebind context workspace");

    assert_eq!(ctx.workspace_dir().unwrap(), dir_b.path());
    let bind_b = ctx.memory_binding().expect("bind workspace B");
    assert!(!Arc::ptr_eq(&bind_a, &bind_b));
}

/// The subsystem-config refresh half of the rebind requirement: a rebind
/// that passes a `[subsystems.memory] driver = "null"` config must make the
/// accessor report the null driver, not the default embedded one captured
/// before the user switch.
#[test]
fn rebind_workspace_refreshes_memory_subsystem_config() {
    let dir_a = tempfile::tempdir().unwrap();
    let ctx = CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(dir_a.path().to_path_buf()),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    };

    let bind_a = ctx.memory_binding().expect("bind workspace A");
    let expected = if cfg!(feature = "modules") {
        crate::core::subsystem::DriverClass::Module
    } else {
        crate::core::subsystem::DriverClass::Null
    };
    assert_eq!(bind_a.class(), expected);

    let null_cfg = crate::config::schema::MemorySubsystemConfig {
        driver: "null".to_string(),
        ..Default::default()
    };
    // This is the dangerous case: changing only the memory config for an
    // already-bound workspace must replace the complete snapshot, so the
    // binding cache sees the new (workspace, config) pair.
    ctx.rebind_workspace(dir_a.path(), null_cfg)
        .expect("rebind context subsystem config");

    let bind_b = ctx.memory_binding().expect("bind workspace B");
    assert_eq!(bind_b.class(), crate::core::subsystem::DriverClass::Null);
}

/// `memory::global`'s clear-on-failed-rebind property, preserved
/// structurally: a workspace whose configured driver is refused resolves to
/// the fallback, never to another workspace's driver.
#[test]
fn failed_bind_never_returns_previous_workspace_binding() {
    let dir_a = tempfile::tempdir().unwrap();
    let dir_b = tempfile::tempdir().unwrap();
    let a = CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(dir_a.path().to_path_buf()),
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    };
    let b = CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: Some(dir_b.path().to_path_buf()),
            memory_subsystem: untrusted_external_memory_cfg(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    };

    let bind_a = a.memory_binding().expect("bind workspace A");
    assert_eq!(bind_a.driver_id(), "tinymemory");
    assert!(bind_a.fallback().is_none());

    let bind_b = b.memory_binding().expect("workspace B falls back");
    assert_eq!(
        bind_b.driver_id(),
        "null",
        "a refused driver must fall back, not inherit another workspace's"
    );
    let fallback = bind_b.fallback().expect("fallback provenance recorded");
    assert_eq!(fallback.configured_driver, "supermemory");
    assert!(!Arc::ptr_eq(&bind_a, &bind_b));
}

/// The single most important default in this step: no binding ⇒ the FULL
/// capability set, mirroring `core::all::group_allowed` with no context.
#[test]
fn memory_capabilities_defaults_open_without_a_workspace() {
    let ctx = CoreContext {
        host_kind: HostKind::Cli,
        workspace_binding: RwLock::new(WorkspaceBinding {
            workspace_dir: None,
            memory_subsystem: Default::default(),
        }),
        domains: crate::core::runtime::DomainSet::full(),
        tool_groups: Default::default(),
        embedder_config: None,
    };
    assert!(ctx.memory_binding().is_err(), "no workspace ⇒ no binding");
    assert_eq!(
        ctx.memory_capabilities(),
        tinymemory_api::capabilities::Capabilities::all(),
        "a context with no binding must not deny any capability"
    );
}

/// The no-context arm of `current_memory_capabilities`. Asserted through
/// the value the fallback branch yields rather than by calling it with an
/// empty `DEFAULT_CONTEXT`: that global is process-wide and another test in
/// the same binary may have set it, which would make a bare
/// `assert_eq!(current_memory_capabilities(), all())` order-dependently
/// flaky.
#[test]
fn current_memory_capabilities_defaults_open_without_a_context() {
    assert_eq!(
        crate::memory::binding::unbound_default_capabilities(),
        tinymemory_api::capabilities::Capabilities::all()
    );
    // And when a context *is* ambient, the call resolves through it rather
    // than erroring.
    let ctx = CoreContext::for_test(crate::core::runtime::DomainSet::full(), None, None);
    assert_eq!(
        ctx.memory_capabilities(),
        tinymemory_api::capabilities::Capabilities::all()
    );
}

/// The DomainSet axis and the capability axis are independent (kernel.md
/// §3.7's three axes): a narrowed `DomainSet` must not narrow capabilities.
#[test]
fn capabilities_are_open_under_a_harness_domain_set() {
    let ctx = CoreContext::for_test(crate::core::runtime::DomainSet::harness(), None, None);
    assert_eq!(
        ctx.memory_capabilities(),
        tinymemory_api::capabilities::Capabilities::all()
    );
}
