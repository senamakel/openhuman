//! Best-effort, scoped automatic memory refresh. Turns only read completed
//! packs; all engine work and date extraction run in bounded background workers.
//! Packs live in memory for one minute and are never persisted by this adapter.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use futures::future::BoxFuture;
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::core::runtime::{current_slot, spawn_scoped};
use crate::memory::scope::ResolvedIdentity;

use super::hooks::{self, PreTurnInput, TurnPack};

const TTL: Duration = Duration::from_secs(60);
const WORK_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ENTRIES: usize = 64;
const MAX_PENDING: usize = 16;
const MAX_PACK_BYTES: usize = 64 * 1024;
static WORKERS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(8)));
static CACHES: LazyLock<Mutex<Vec<Weak<Cache>>>> = LazyLock::new(Mutex::default);

type Work = BoxFuture<'static, Option<TurnPack>>;

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    workspace: PathBuf,
    config: u64,
    identity: ResolvedIdentity,
    thread: String,
    compaction: bool,
}

impl Key {
    fn new(config: &Config, identity: &ResolvedIdentity, thread: &str, compaction: bool) -> Self {
        let mut fingerprint = std::collections::hash_map::DefaultHasher::new();
        // Keep only the digest, never serialized settings or credentials.
        serde_json::to_value(config)
            .ok()
            .and_then(|value| serde_json::to_vec(&value).ok())
            .unwrap_or_default()
            .hash(&mut fingerprint);
        config.config_path.hash(&mut fingerprint);
        config.action_dir.hash(&mut fingerprint);
        Self {
            workspace: config.workspace_dir.clone(),
            config: fingerprint.finish(),
            identity: identity.clone(),
            thread: thread.to_owned(),
            compaction,
        }
    }
}

struct Entry {
    ticket: u64,
    revision: u64,
    pack: Option<(Instant, TurnPack)>,
    pending: VecDeque<Work>,
    active: bool,
    touched: Instant,
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Entry>,
    next_ticket: u64,
}

#[derive(Default)]
struct Cache(Mutex<State>);

fn cache() -> Arc<Cache> {
    let cache = current_slot::<Cache>();
    let mut caches = CACHES.lock().unwrap_or_else(PoisonError::into_inner);
    caches.retain(|known| known.strong_count() > 0);
    if !caches
        .iter()
        .any(|known| known.ptr_eq(&Arc::downgrade(&cache)))
    {
        caches.push(Arc::downgrade(&cache));
    }
    cache
}

/// Discards completed packs for this workspace, across its agent contexts.
/// Pending conversation logs are preserved; results of reads already in flight
/// cannot republish after invalidation. Call after
/// successful memory writes and erasure; no engine work runs here.
pub(crate) fn invalidate(config: &Config) {
    let mut caches = CACHES.lock().unwrap_or_else(PoisonError::into_inner);
    caches.retain(|known| {
        let Some(cache) = known.upgrade() else {
            return false;
        };
        cache
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .iter_mut()
            .filter(|(key, _)| key.workspace == config.workspace_dir)
            .for_each(|(_, entry)| {
                entry.pack = None;
                entry.revision = entry.revision.wrapping_add(1);
            });
        true
    });
}

/// Clears all automatic packs after engine or credential invalidation. Old
/// in-flight completions cannot recreate entries they no longer own.
pub(crate) fn invalidate_all() {
    let mut caches = CACHES.lock().unwrap_or_else(PoisonError::into_inner);
    caches.retain(|known| {
        let Some(cache) = known.upgrade() else {
            return false;
        };
        cache
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entries
            .clear();
        true
    });
}

fn completed(cache: &Cache, key: &Key) -> Option<TurnPack> {
    let mut state = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    let entry = state.entries.get_mut(key)?;
    entry.touched = Instant::now();
    if entry
        .pack
        .as_ref()
        .is_some_and(|(at, _)| at.elapsed() >= TTL)
    {
        entry.pack = None;
    }
    entry.pack.as_ref().map(|(_, pack)| pack.clone())
}

/// Returns the last completed pack immediately, then logs this user turn and
/// refreshes its context in the background. A cold cache returns `None`.
/// Omitted recall still logs, but cannot consume another identity's pack.
pub(crate) fn pre_turn(
    config: Arc<Config>,
    identity: ResolvedIdentity,
    input: PreTurnInput,
    channel: Option<String>,
) -> Option<TurnPack> {
    if !config.memory.conversations.enabled && !identity.recall {
        return None;
    }
    let cache = cache();
    let key = Key::new(&config, &identity, &input.thread_id, false);
    let pack = identity.recall.then(|| completed(&cache, &key)).flatten();
    tracing::debug!(thread_id = %key.thread, agent_id = %key.identity.agent_id, cache_hit = pack.is_some(), "[memory:prefetch] pre_turn cache");
    if let Some(pack) = &pack {
        crate::memory::tools::record_pack_citations(&input.thread_id, pack.citations.clone());
    }
    enqueue(
        cache,
        key,
        guard_turn_work(
            &config.clone(),
            Box::pin(crate::core::runtime::spawn::scoped(async move {
                if config.memory.conversations.enabled {
                    if let Some(channel) = channel {
                        let workspace = config.workspace_dir.clone();
                        let thread = input.thread_id.clone();
                        let _ = crate::core::runtime::spawn_blocking_scoped(move || {
                            crate::memory::channels::record(&workspace, &channel, &thread)
                        })
                        .await;
                    }
                }
                hooks::pre_turn_work(&config, &identity, input).await
            })),
        ),
    );
    pack.map(cached_notice)
}

fn guard_turn_work(config: &Config, work: Work) -> Work {
    let mutation = crate::memory::tool_writes::automatic_mutation(config);
    Box::pin(async move { mutation.run(work).await.flatten() })
}

fn cached_notice(mut pack: TurnPack) -> TurnPack {
    let notice = "This is cached memory from a completed background lookup for an earlier turn. It may be incomplete or unrelated to the current question; use explicit memory recall or fetch when you need a current answer.\n\n";
    pack.markdown.insert_str(0, notice);
    pack.tokens += tinymemory_tools::recall::estimate_tokens(notice);
    pack
}

/// Uses only completed context while queuing recall of the folded span. The
/// authoritative summarizer never waits for this refresh.
pub(crate) fn compaction(
    config: Arc<Config>,
    identity: ResolvedIdentity,
    thread: String,
    dropped: Vec<tinymemory_api::Turn>,
) -> Option<TurnPack> {
    if !identity.recall || dropped.is_empty() {
        return None;
    }
    let cache = cache();
    let key = Key::new(&config, &identity, &thread, true);
    let pack = completed(&cache, &key)
        .or_else(|| completed(&cache, &Key::new(&config, &identity, &thread, false)));
    enqueue(
        cache,
        key,
        Box::pin(crate::core::runtime::spawn::scoped(async move {
            hooks::compaction_work(&config, &identity, &thread, dropped).await
        })),
    );
    pack.map(cached_notice)
}

fn enqueue(cache: Arc<Cache>, key: Key, work: Work) {
    let mut state = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
    if !state.entries.contains_key(&key) {
        if state.entries.len() >= MAX_ENTRIES {
            let oldest = state
                .entries
                .iter()
                .filter(|(_, entry)| !entry.active)
                .min_by_key(|(_, entry)| entry.touched)
                .map(|(key, _)| key.clone());
            let Some(oldest) = oldest else {
                tracing::warn!("[memory:prefetch] cache busy; automatic refresh skipped");
                return;
            };
            state.entries.remove(&oldest);
        }
        state.next_ticket = state.next_ticket.wrapping_add(1);
        let ticket = state.next_ticket;
        state.entries.insert(
            key.clone(),
            Entry {
                ticket,
                revision: 0,
                pack: None,
                pending: VecDeque::new(),
                active: false,
                touched: Instant::now(),
            },
        );
    }
    let entry = state.entries.get_mut(&key).expect("inserted");
    if entry.pending.len() >= MAX_PENDING {
        tracing::warn!("[memory:prefetch] queue full; automatic refresh skipped");
        return;
    }
    // No waiting task is created when capacity is exhausted.
    let permit = if entry.active {
        None
    } else {
        let Ok(permit) = WORKERS.clone().try_acquire_owned() else {
            tracing::warn!("[memory:prefetch] workers busy; automatic refresh skipped");
            return;
        };
        Some(permit)
    };
    entry.pending.push_back(work);
    entry.touched = Instant::now();
    let ticket = entry.ticket;
    tracing::debug!(thread_id = %key.thread, agent_id = %key.identity.agent_id, compaction = key.compaction, ticket, pending = entry.pending.len(), "[memory:prefetch] refresh queued");
    if entry.active {
        return;
    }
    entry.active = true;
    drop(state);
    let cleanup = Worker {
        cache: cache.clone(),
        key: key.clone(),
        ticket,
        armed: true,
    };
    spawn_scoped(async move {
        let _permit = permit;
        let mut cleanup = cleanup;
        loop {
            let (work, revision) = {
                let mut state = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(entry) = state
                    .entries
                    .get_mut(&key)
                    .filter(|entry| entry.ticket == ticket)
                else {
                    return;
                };
                let Some(work) = entry.pending.pop_front() else {
                    entry.active = false;
                    cleanup.armed = false;
                    return;
                };
                (work, entry.revision)
            };
            let started = Instant::now();
            let pack = match tokio::time::timeout(WORK_TIMEOUT, work).await {
                Ok(pack) => pack.filter(|pack| {
                    serde_json::to_vec(pack)
                        .ok()
                        .zip(serde_json::to_vec(&pack.citations).ok())
                        .is_some_and(|(pack, citations)| {
                            pack.len().saturating_add(citations.len()) <= MAX_PACK_BYTES
                        })
                }),
                Err(_) => {
                    tracing::warn!(thread_id = %key.thread, agent_id = %key.identity.agent_id, compaction = key.compaction, ticket, "[memory:prefetch] automatic refresh timed out");
                    None
                }
            };
            tracing::debug!(thread_id = %key.thread, agent_id = %key.identity.agent_id, compaction = key.compaction, ticket, completed = pack.is_some(), elapsed_ms = started.elapsed().as_millis() as u64, "[memory:prefetch] refresh completed");
            let mut state = cache.0.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(entry) = state
                .entries
                .get_mut(&key)
                .filter(|entry| entry.ticket == ticket)
            {
                if entry.revision == revision {
                    entry.pack = pack.map(|pack| (Instant::now(), pack));
                }
            }
        }
    });
}

// Cancellation/runtime shutdown and panics must release the active flag as
// well as the semaphore permit; a subsequent turn can retry normally.
struct Worker {
    cache: Arc<Cache>,
    key: Key,
    ticket: u64,
    armed: bool,
}
impl Drop for Worker {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut state = self.cache.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(entry) = state
            .entries
            .get_mut(&self.key)
            .filter(|entry| entry.ticket == self.ticket)
        {
            entry.active = false;
            entry.pending.clear();
        }
    }
}

#[cfg(test)]
pub(crate) async fn settle_for_test(
    config: &Config,
    identity: &ResolvedIdentity,
    thread: &str,
    compaction: bool,
) {
    let cache = cache();
    let key = Key::new(config, identity, thread, compaction);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if cache
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .entries
                .get(&key)
                .is_some_and(|entry| !entry.active)
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("automatic memory refresh completed");
}

#[cfg(test)]
#[path = "prefetch_tests.rs"]
mod tests;
