//! Seams are process-global, so each test uses names no other test shares,
//! and the ranker tests (one slot for the whole process) serialize on a lock.

use super::*;
use openhuman_core::agent::hooks::{ToolHookContext, TurnContext};
use tinytools::{RankCandidate, RankContext, RankError, RankHit, ToolRanker};

static RANKER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct NamedRanker(&'static str);

#[async_trait::async_trait]
impl ToolRanker for NamedRanker {
    fn kind(&self) -> &'static str {
        self.0
    }
    async fn rank(
        &self,
        _intent: &str,
        _context: &RankContext,
        _candidates: &[RankCandidate],
        _limit: usize,
    ) -> Result<Vec<RankHit>, RankError> {
        Ok(Vec::new())
    }
}

struct NamedPostTurn(&'static str);

#[async_trait::async_trait]
impl PostTurnHook for NamedPostTurn {
    fn name(&self) -> &str {
        self.0
    }
    async fn on_turn_complete(&self, _ctx: &TurnContext) -> anyhow::Result<()> {
        Ok(())
    }
}

struct NamedToolHook(&'static str);

#[async_trait::async_trait]
impl ToolHook for NamedToolHook {
    fn name(&self) -> &str {
        self.0
    }
    async fn before_tool(&self, _context: &ToolHookContext) -> anyhow::Result<()> {
        Ok(())
    }
    async fn after_tool(&self, _context: &ToolHookContext) -> anyhow::Result<()> {
        Ok(())
    }
}

fn ranker_kind() -> Option<&'static str> {
    installed_tool_ranker().map(|ranker| ranker.kind())
}

/// How many hooks of `name` are registered, `None` for zero.
fn post_turn_marker(name: &str) -> Option<u8> {
    let count = embedder_post_turn_hooks()
        .iter()
        .filter(|hook| hook.name() == name)
        .count();
    u8::try_from(count).ok().filter(|count| *count > 0)
}

#[test]
fn a_tool_ranker_is_installed_and_the_previous_one_restored() {
    let _lock = RANKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    install_tool_ranker(Arc::new(NamedRanker("seams-previous")));

    let seams = HostSeams {
        tool_ranker: Some(Arc::new(NamedRanker("seams-ours"))),
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    assert_eq!(ranker_kind(), Some("seams-ours"));

    drop(seams);
    assert_eq!(ranker_kind(), Some("seams-previous"));
    clear_tool_ranker();
}

#[test]
fn a_tool_ranker_with_no_predecessor_is_cleared_on_drop() {
    let _lock = RANKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tool_ranker();
    let seams = HostSeams {
        tool_ranker: Some(Arc::new(NamedRanker("seams-alone"))),
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    assert_eq!(ranker_kind(), Some("seams-alone"));
    drop(seams);
    assert_eq!(ranker_kind(), None);
}

#[test]
fn a_ranker_replaced_after_build_is_left_alone() {
    let _lock = RANKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tool_ranker();
    let seams = HostSeams {
        tool_ranker: Some(Arc::new(NamedRanker("seams-first"))),
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    install_tool_ranker(Arc::new(NamedRanker("seams-later")));
    drop(seams);
    assert_eq!(ranker_kind(), Some("seams-later"), "not ours any more");
    clear_tool_ranker();
}

#[test]
fn persisted_seams_survive_the_guard() {
    let _lock = RANKER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_tool_ranker();
    HostSeams {
        tool_ranker: Some(Arc::new(NamedRanker("seams-persisted"))),
        post_turn_hooks: vec![Arc::new(NamedPostTurn("seams-persisted-hook"))],
        ..HostSeams::default()
    }
    .install()
    .expect("install")
    .persist();
    assert_eq!(ranker_kind(), Some("seams-persisted"));
    assert_eq!(post_turn_marker("seams-persisted-hook"), Some(1));
    clear_tool_ranker();
    replace_embedder_post_turn_hook("seams-persisted-hook", None);
}

#[test]
fn post_turn_hooks_are_removed_and_a_replaced_one_restored() {
    replace_embedder_post_turn_hook(
        "seams-replaced-hook",
        Some(Arc::new(NamedPostTurn("seams-replaced-hook"))),
    );
    let seams = HostSeams {
        post_turn_hooks: vec![
            Arc::new(NamedPostTurn("seams-replaced-hook")),
            Arc::new(NamedPostTurn("seams-new-hook")),
        ],
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    // Replaced, not duplicated.
    assert_eq!(post_turn_marker("seams-replaced-hook"), Some(1));
    assert_eq!(post_turn_marker("seams-new-hook"), Some(1));

    drop(seams);
    assert_eq!(post_turn_marker("seams-new-hook"), None, "ours removed");
    assert_eq!(
        post_turn_marker("seams-replaced-hook"),
        Some(1),
        "predecessor restored"
    );
    replace_embedder_post_turn_hook("seams-replaced-hook", None);
}

#[test]
fn same_named_hooks_unwind_newest_first() {
    let original: Arc<dyn PostTurnHook> = Arc::new(NamedPostTurn("seams-dup-hook"));
    replace_embedder_post_turn_hook("seams-dup-hook", Some(Arc::clone(&original)));
    let seams = HostSeams {
        post_turn_hooks: vec![
            Arc::new(NamedPostTurn("seams-dup-hook")),
            Arc::new(NamedPostTurn("seams-dup-hook")),
        ],
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    assert_eq!(post_turn_marker("seams-dup-hook"), Some(1));

    drop(seams);
    let restored = embedder_post_turn_hooks()
        .into_iter()
        .find(|hook| hook.name() == "seams-dup-hook")
        .expect("a hook under the name");
    assert!(
        Arc::ptr_eq(&restored, &original),
        "the original is back, not the runtime's first hook"
    );
    replace_embedder_post_turn_hook("seams-dup-hook", None);
}

#[test]
fn a_hook_replaced_after_build_is_left_alone() {
    let seams = HostSeams {
        tool_hooks: vec![Arc::new(NamedToolHook("seams-late-hook"))],
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    let later: Arc<dyn ToolHook> = Arc::new(NamedToolHook("seams-late-hook"));
    replace_embedder_tool_hook("seams-late-hook", Some(Arc::clone(&later)));

    drop(seams);
    let current = embedder_tool_hooks()
        .into_iter()
        .find(|hook| hook.name() == "seams-late-hook")
        .expect("the later hook stays");
    assert!(Arc::ptr_eq(&current, &later));
    replace_embedder_tool_hook("seams-late-hook", None);
}

#[test]
fn tool_hooks_are_installed_and_removed() {
    let present = |name: &str| embedder_tool_hooks().iter().any(|hook| hook.name() == name);
    let seams = HostSeams {
        tool_hooks: vec![Arc::new(NamedToolHook("seams-tool-hook"))],
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    assert!(present("seams-tool-hook"));
    drop(seams);
    assert!(!present("seams-tool-hook"));
}

#[test]
fn a_live_policy_waits_for_the_core_to_boot() {
    let seams = HostSeams {
        live_policy: Some(Arc::new(SecurityPolicy::default())),
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    assert!(
        seams.has_pending_live_policy(),
        "installed after boot, not before"
    );
}

#[test]
fn builder_options_collect_into_the_seams() {
    let builder = RuntimeBuilder::new()
        .tool_ranker(Arc::new(NamedRanker("seams-builder")))
        .post_turn_hook(Arc::new(NamedPostTurn("seams-builder-hook")))
        .tool_hook(Arc::new(NamedToolHook("seams-builder-tool")))
        .live_policy(Arc::new(SecurityPolicy::default()));
    assert!(builder.seams.tool_ranker.is_some());
    assert_eq!(builder.seams.post_turn_hooks.len(), 1);
    assert_eq!(builder.seams.tool_hooks.len(), 1);
    assert!(builder.seams.live_policy.is_some());
    assert!(builder.seams.server_launcher.is_none());
}

static STORAGE_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn memory_backend() -> Arc<dyn openhuman_core::storage::StorageBackend> {
    openhuman_core::storage::open("memory")
        .await
        .expect("memory backend")
}

#[tokio::test]
async fn a_storage_url_is_opened_installed_and_the_previous_backend_restored() {
    let _lock = STORAGE_LOCK.lock().await;
    let previous = memory_backend().await;
    openhuman_core::storage::install(Arc::clone(&previous));

    let mut seams = HostSeams {
        storage: Some(StorageSource::from("memory")),
        ..HostSeams::default()
    };
    seams.open_storage().await.expect("open");
    let installed = seams.install().expect("install");
    let ours = openhuman_core::storage::installed().expect("ours installed");
    assert!(!Arc::ptr_eq(&ours, &previous), "the seam installed its own");

    drop(installed);
    let now = openhuman_core::storage::installed().expect("previous restored");
    assert!(Arc::ptr_eq(&now, &previous));
    openhuman_core::storage::clear();
}

#[tokio::test]
async fn a_backend_with_no_predecessor_is_cleared_and_a_replaced_one_left_alone() {
    let _lock = STORAGE_LOCK.lock().await;
    openhuman_core::storage::clear();

    let backend = memory_backend().await;
    let installed = HostSeams {
        storage: Some(StorageSource::Backend(Arc::clone(&backend))),
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    assert!(openhuman_core::storage::installed().is_some());
    drop(installed);
    assert!(openhuman_core::storage::installed().is_none());

    let installed = HostSeams {
        storage: Some(StorageSource::Backend(backend)),
        ..HostSeams::default()
    }
    .install()
    .expect("install");
    let later = memory_backend().await;
    openhuman_core::storage::install(Arc::clone(&later));
    drop(installed);
    let now = openhuman_core::storage::installed().expect("later kept");
    assert!(Arc::ptr_eq(&now, &later));
    openhuman_core::storage::clear();
}

#[tokio::test]
async fn an_unopenable_storage_url_fails_the_open() {
    let mut seams = HostSeams {
        storage: Some(StorageSource::from("nonsense://nowhere")),
        ..HostSeams::default()
    };
    let error = seams.open_storage().await.expect_err("bad url");
    assert!(error.contains("storage backend"), "{error}");
}
