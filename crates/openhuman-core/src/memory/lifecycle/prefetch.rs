//! Best-effort, scoped automatic memory refresh. Turns only read completed
//! packs; all engine work and date extraction run in bounded background workers.
//! Packs live in memory for one minute and are never persisted by this adapter.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use futures::future::BoxFuture;
use tinymemory_tools::prefetch::{Limits, PrefetchRuntime, TurnCache, TurnCacheRegistry, WorkerPool};

use crate::config::Config;
use crate::core::runtime::{slot_for, DispatchCtx};
use tinymemory_tools::layout::identity::ResolvedIdentity;

use super::hooks::{self, TurnPack};

static WORKERS: LazyLock<WorkerPool> = LazyLock::new(|| WorkerPool::new(8));
static CACHES: LazyLock<TurnCacheRegistry<Key>> = LazyLock::new(TurnCacheRegistry::default);

type Work = BoxFuture<'static, Option<TurnPack>>;

/// The host executor the prefetcher's workers run on: the caller's core
/// context and memory identity ride along with every spawned task.
struct HostRuntime;

impl PrefetchRuntime for HostRuntime {
    fn spawn(&self, task: BoxFuture<'static, ()>) {
        drop(DispatchCtx::ambient().spawn(task));
    }

    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    workspace: PathBuf,
    config: u64,
    identity: ResolvedIdentity,
    thread: String,
    compaction: bool,
}

impl Key {
    /// `config_fingerprint` is supplied by the host so the cache key can
    /// later consume the host's config generation directly (e.g. TurnCache).
    fn with_fingerprint(
        config: &Config,
        config_fingerprint: u64,
        identity: &ResolvedIdentity,
        thread: &str,
        compaction: bool,
    ) -> Self {
        Self {
            workspace: config.workspace_dir.clone(),
            config: config_fingerprint,
            identity: identity.clone(),
            thread: thread.to_owned(),
            compaction,
        }
    }

    #[cfg(test)]
    fn new(config: &Config, identity: &ResolvedIdentity, thread: &str, compaction: bool) -> Self {
        Self::with_fingerprint(
            config,
            memory_config_fingerprint(config),
            identity,
            thread,
            compaction,
        )
    }
}

/// Fingerprints only memory settings that affect recall and its sources. The
/// full Config is large and unrelated settings must not invalidate turn packs.
fn memory_config_fingerprint(config: &Config) -> u64 {
    let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
    config.memory.hash(&mut fingerprint);
    config.config_path.hash(&mut fingerprint);
    config.action_dir.hash(&mut fingerprint);
    config.api_url.hash(&mut fingerprint);
    config.inference_url.hash(&mut fingerprint);
    config.api_key.hash(&mut fingerprint);
    config.components.memory.hash(&mut fingerprint);
    config.default_model.hash(&mut fingerprint);
    config.chat_provider.hash(&mut fingerprint);
    config.memory_provider.hash(&mut fingerprint);
    config.model_routes.hash(&mut fingerprint);
    config.time_zone().hash(&mut fingerprint);
    fingerprint.finish()
}

/// `slot_for` constructs a per-context default, while the actual cache and
/// cross-context weak registry policy belong to TinyMemory.
struct Cache(Arc<TurnCache<Key>>);

impl Default for Cache {
    fn default() -> Self {
        Self(Arc::new(
            TurnCache::new(Arc::new(HostRuntime))
                .with_limits(Limits::default())
                .with_workers(WORKERS.clone()),
        ))
    }
}

fn cache(ctx: &DispatchCtx) -> Arc<Cache> {
    let cache = slot_for::<Cache>(ctx);
    CACHES.register(&cache.0);
    cache
}

/// Discards completed packs for this workspace, across its agent contexts.
/// Pending conversation logs are preserved; results of reads already in flight
/// cannot republish after invalidation. Call after
/// successful memory writes and erasure; no engine work runs here.
pub(crate) fn invalidate(config: &Config) {
    CACHES.invalidate_where(|key| key.workspace == config.workspace_dir);
}

/// Clears all automatic packs after engine or credential invalidation. Old
/// in-flight completions cannot recreate entries they no longer own.
pub(crate) fn invalidate_all() {
    CACHES.invalidate_all();
}

fn completed(cache: &Cache, key: &Key) -> Option<TurnPack> {
    cache.0.completed(key)
}

fn enqueue(cache: Arc<Cache>, key: Key, work: Work) {
    tracing::debug!(thread_id = %key.thread, agent_id = %key.identity.agent_id, compaction = key.compaction, "[memory:prefetch] enqueue refresh");
    cache.0.enqueue(key, work);
}

/// Returns the last completed pack immediately, then logs this user turn and
/// refreshes its context in the background. A cold cache returns `None`.
/// Omitted recall still logs, but cannot consume another identity's pack.
pub(crate) fn pre_turn(
    ctx: &DispatchCtx,
    config: Arc<Config>,
    identity: ResolvedIdentity,
    thread_id: String,
    turn_index: u32,
    user_text: String,
    in_prompt_from: u32,
    at: chrono::DateTime<chrono::Utc>,
    resumed_after_compaction: bool,
    observed_actor: Option<tinymemory_api::ObservedActor>,
    channel: Option<String>,
) -> Option<TurnPack> {
    if !config.memory.conversations.enabled && !identity.recall {
        return None;
    }
    let cache = cache(ctx);
    let key = Key::with_fingerprint(
        &config,
        memory_config_fingerprint(&config),
        &identity,
        &thread_id,
        false,
    );
    let pack = identity.recall.then(|| completed(&cache, &key)).flatten();
    tracing::debug!(thread_id = %key.thread, agent_id = %key.identity.agent_id, cache_hit = pack.is_some(), "[memory:prefetch] pre_turn cache");
    if let Some(pack) = &pack {
        hooks::deliver_citations(ctx, &config, &identity, &thread_id, pack);
    }
    let dispatch = ctx.clone();
    enqueue(
        cache,
        key,
        guard_turn_work(
            &config.clone(),
            Box::pin(dispatch.clone().enter(async move {
                hooks::pre_turn_work(
                    &dispatch,
                    &config,
                    &identity,
                    thread_id,
                    turn_index,
                    user_text,
                    in_prompt_from,
                    at,
                    resumed_after_compaction,
                    observed_actor,
                    channel,
                )
                .await
            })),
        ),
    );
    pack
}

fn guard_turn_work(config: &Config, work: Work) -> Work {
    let mutation = crate::memory::tool_writes::automatic_mutation(config);
    Box::pin(async move { mutation.run(work).await.flatten() })
}

/// Uses only completed context while queuing recall of the folded span. The
/// authoritative summarizer never waits for this refresh.
pub(crate) fn compaction(
    ctx: &DispatchCtx,
    config: Arc<Config>,
    identity: ResolvedIdentity,
    thread: String,
    dropped: Vec<tinymemory_api::Turn>,
) -> Option<TurnPack> {
    if !identity.recall || dropped.is_empty() {
        return None;
    }
    let cache = cache(ctx);
    let fingerprint = memory_config_fingerprint(&config);
    let key = Key::with_fingerprint(&config, fingerprint, &identity, &thread, true);
    let pack = completed(&cache, &key).or_else(|| {
        completed(
            &cache,
            &Key::with_fingerprint(&config, fingerprint, &identity, &thread, false),
        )
    });
    let dispatch = ctx.clone();
    enqueue(
        cache,
        key,
        Box::pin(ctx.enter(async move {
            hooks::compaction_work(&dispatch, &config, &identity, &thread, dropped).await
        })),
    );
    pack
}

#[cfg(test)]
pub(crate) async fn settle_for_test(
    ctx: &DispatchCtx,
    config: &Config,
    identity: &ResolvedIdentity,
    thread: &str,
    compaction: bool,
) {
    let cache = cache(ctx);
    let key = Key::with_fingerprint(
        config,
        memory_config_fingerprint(config),
        identity,
        thread,
        compaction,
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while !cache.0.is_idle(&key) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("automatic memory refresh completed");
}

#[cfg(test)]
#[path = "prefetch_tests.rs"]
mod tests;
