//! Durable, bounded FIFO for the agent tool's optional memory writes.
//! Only scrubbed items and trusted scope are persisted; credentials are resolved
//! at drain time. Enqueue never waits on the remote engine.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tinymemory_api::{Namespace, Reach, StoreItem, WriteOptions};

use super::error::{MemoryError, MemoryResult};
use super::tools::CallFacts;
use super::types::{ForgetParams, LearnParams};
use crate::config::Config;

const MAX_PENDING: usize = 256;
const MAX_BYTES: usize = 2 * 1024 * 1024;
const WRITE_TIMEOUT: Duration = Duration::from_secs(120);
static FENCE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static PROCESS_EPOCH: LazyLock<String> = LazyLock::new(|| uuid::Uuid::new_v4().to_string());
const QUEUED: &str = "queued; not yet saved to memory";
static LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[derive(Default)]
struct DrainState {
    barrier: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Default)]
struct OwnerState {
    admission: Mutex<()>,
    barrier: Arc<tokio::sync::RwLock<()>>,
    generation: AtomicU64,
    fences: AtomicU64,
}

static OWNERS: LazyLock<Mutex<HashMap<PathBuf, Weak<OwnerState>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

type DrainLocks = HashMap<PathBuf, Weak<DrainState>>;
static DRAINS: LazyLock<Mutex<DrainLocks>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    ticket: String,
    root: Option<String>,
    layout_v3: bool,
    scope_root: Namespace,
    #[serde(default)]
    epoch: Option<(String, u64)>,
    operation: Operation,
    // Stable, content-free last outcome; never an engine message.
    status: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum Operation {
    Learn { item: StoreItem },
    Forget { ids: Vec<String>, reach: Reach },
}

#[cfg(test)]
fn path(workspace: &Path) -> PathBuf {
    let config = Config {
        workspace_dir: workspace.to_path_buf(),
        config_path: workspace
            .parent()
            .expect("fixture root")
            .join("config.toml"),
        ..Config::default()
    };
    queue_path(&config, &Namespace::ROOT)
}

fn owner_prefix(config: &Config) -> String {
    let root = super::scope::user_root(config);
    let fallback = root
        .is_none()
        .then(|| config.config_path.to_string_lossy().to_string());
    let owner = serde_json::to_vec(&(root, fallback)).expect("owner fields serialize");
    format!("tool_writes-{:x}", Sha256::digest(owner))
}

fn queue_path(config: &Config, root: &Namespace) -> PathBuf {
    let partition = serde_json::to_vec(&(
        &config.memory.root,
        super::scope::layout_is_v3(config),
        root,
    ))
    .expect("partition fields serialize");
    config.workspace_dir.join("memory").join(format!(
        "{}-{:x}.json",
        owner_prefix(config),
        Sha256::digest(partition)
    ))
}

fn unavailable() -> MemoryError {
    MemoryError::Unavailable(
        "memory write queue unavailable; durable queueing was not confirmed".into(),
    )
}

fn read_at(path: &Path) -> MemoryResult<Vec<Entry>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(unavailable()),
    };
    let mut bytes = Vec::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable())?;
    if bytes.len() > MAX_BYTES {
        return Err(unavailable());
    }
    let entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(|_| unavailable())?;
    if entries.len() > MAX_PENDING {
        return Err(unavailable());
    }
    Ok(entries)
}

fn write_at(file: &Path, entries: &[Entry]) -> MemoryResult<()> {
    let bytes = serde_json::to_vec(entries).map_err(|_| unavailable())?;
    if bytes.len() > MAX_BYTES {
        return Err(unavailable());
    }
    let temp = file.with_extension("json.tmp");
    let result = (|| -> std::io::Result<()> {
        super::files::create_private_dir_all(file.parent().expect("memory directory"))?;
        let mut output = super::files::create_private(&temp)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        std::fs::rename(&temp, file)?;
        #[cfg(unix)]
        std::fs::File::open(file.parent().expect("memory directory"))?.sync_all()?;
        Ok(())
    })();
    result.map_err(|_| unavailable())
}

#[cfg(test)]
fn read(workspace: &Path) -> MemoryResult<Vec<Entry>> {
    read_at(&path(workspace))
}
#[cfg(test)]
fn write(workspace: &Path, entries: &[Entry]) -> MemoryResult<()> {
    write_at(&path(workspace), entries)
}

/// Accepts a validated, scrubbed write locally. The acknowledgement means only
/// that it is durable in the local outbox, not accepted by the remote engine.
pub(super) fn enqueue(
    config: &Config,
    args: &serde_json::Value,
    facts: &CallFacts,
) -> MemoryResult<serde_json::Value> {
    if config.memory.engine == super::engine::DISABLED_ENGINE {
        return Err(MemoryError::Off("memory is disabled".into()));
    }
    let confinement = super::user_scope::confinement(config)?;
    let reach = match confinement {
        Some(ref root) => super::user_scope::clamp_reach(Some(facts.reach.clone()), root),
        None => facts.reach.clone(),
    };
    let scope_root = reach.at.clone();
    let file = queue_path(config, &scope_root);
    let operation = match args.get("action").and_then(serde_json::Value::as_str) {
        Some("learn") => {
            let mut params: LearnParams = serde_json::from_value(args.clone())
                .map_err(|_| MemoryError::invalid("invalid learning arguments"))?;
            params.meta = None;
            let mut item = super::ops::learning_item(params, Some(facts.learn_meta()))?;
            if let Some(root) = confinement {
                super::user_scope::clamp_item(&mut item, &root);
            }
            if !super::user_scope::within(&item.meta().namespace, &reach.at) || reach.inherit {
                return Err(MemoryError::invalid(
                    "learning namespace is outside the trusted reach",
                ));
            }
            Operation::Learn {
                item: super::guard::scrub(item),
            }
        }
        Some("forget") => {
            let params: ForgetParams = serde_json::from_value(args.clone())
                .map_err(|_| MemoryError::invalid("invalid forgetting arguments"))?;
            let mut ids: Vec<_> = params
                .ids
                .into_iter()
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect();
            ids.sort();
            ids.dedup();
            if ids.is_empty() {
                return Err(MemoryError::invalid("forget needs at least one id"));
            }
            Operation::Forget { ids, reach }
        }
        _ => return Err(MemoryError::invalid("expected a memory write")),
    };
    let acknowledgement = match &operation {
        Operation::Learn { item } => serde_json::json!({"id":item.fingerprint(), "status":QUEUED}),
        Operation::Forget { ids, .. } => {
            serde_json::json!({"queued":ids.len(), "status":"queued; not yet removed from memory"})
        }
    };
    let root = super::scope::user_root(config);
    let layout_v3 = super::scope::layout_is_v3(config);
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !file.exists() && partitions(config)?.len() >= MAX_PENDING {
        return Err(MemoryError::Unavailable(
            "memory write queue full; nothing was queued".into(),
        ));
    }
    let mut entries = read_at(&file)?;
    let owner = owner_state(config);
    let (fencing, epoch) = {
        let _admission = owner
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (
            owner.fences.load(Ordering::SeqCst) > 0,
            Some((
                PROCESS_EPOCH.clone(),
                owner.generation.load(Ordering::SeqCst),
            )),
        )
    };
    // Adjacent retries are coalesced; never coalesce across a forget/relearn.
    if !fencing
        && entries.last().is_some_and(|last| {
            last.root == root
                && last.layout_v3 == layout_v3
                && match (&last.operation, &operation) {
                    (Operation::Learn { item: a }, Operation::Learn { item: b }) => {
                        a.fingerprint() == b.fingerprint()
                    }
                    (
                        Operation::Forget { ids: a, reach: ar },
                        Operation::Forget { ids: b, reach: br },
                    ) => a == b && ar == br,
                    _ => false,
                }
        })
    {
        return Ok(acknowledgement);
    }
    if entries.len() >= MAX_PENDING {
        return Err(MemoryError::Unavailable(
            "memory write queue full; nothing was queued".into(),
        ));
    }
    entries.push(Entry {
        ticket: uuid::Uuid::new_v4().to_string(),
        root,
        layout_v3,
        scope_root,
        epoch,
        operation,
        status: None,
    });
    write_at(&file, &entries)?;
    Ok(acknowledgement)
}

fn drain_state(workspace: &Path) -> Arc<DrainState> {
    let mut locks = DRAINS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks.retain(|_, value| value.strong_count() > 0);
    if let Some(lock) = locks.get(workspace).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(DrainState::default());
    locks.insert(workspace.to_path_buf(), Arc::downgrade(&lock));
    lock
}

fn owner_state(config: &Config) -> Arc<OwnerState> {
    let key = config
        .workspace_dir
        .join("memory")
        .join(owner_prefix(config));
    let mut owners = OWNERS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    owners.retain(|_, value| value.strong_count() > 0);
    if let Some(state) = owners.get(&key).and_then(Weak::upgrade) {
        return state;
    }
    let state = Arc::new(OwnerState::default());
    owners.insert(key, Arc::downgrade(&state));
    state
}

fn partitions(config: &Config) -> MemoryResult<Vec<PathBuf>> {
    let prefix = format!("{}-", owner_prefix(config));
    let entries = match std::fs::read_dir(config.workspace_dir.join("memory")) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(unavailable()),
    };
    let mut files = Vec::new();
    for entry in entries {
        let file = entry.map_err(|_| unavailable())?.path();
        if file
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".json"))
        {
            files.push(file);
            if files.len() > MAX_PENDING {
                return Err(unavailable());
            }
        }
    }
    Ok(files)
}

/// Owner epoch captured when automatic turn work is admitted, before it can
/// wait in the prefetch queue. Erasure retires older work and waits for active
/// mutations while subsequent work waits behind the exclusive barrier.
pub(crate) struct AutomaticMutation {
    state: Arc<OwnerState>,
    generation: u64,
}

impl AutomaticMutation {
    pub(crate) async fn run<T>(self, work: impl std::future::Future<Output = T>) -> Option<T> {
        if self.state.generation.load(Ordering::SeqCst) != self.generation {
            return None;
        }
        let _barrier = self.state.barrier.read().await;
        if self.state.generation.load(Ordering::SeqCst) != self.generation {
            return None;
        }
        Some(work.await)
    }
}

/// Captures authority without waiting on the engine or an active erasure.
pub(crate) fn automatic_mutation(config: &Config) -> AutomaticMutation {
    let state = owner_state(config);
    let generation = {
        let _admission = state
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.generation.load(Ordering::SeqCst)
    };
    AutomaticMutation { state, generation }
}

/// Holds the owner's mutation barrier through explicit engine erasure.
/// Subsequent queued writes resume only after the outer erasure finishes.
pub(crate) struct WriteFence {
    guard: Option<tokio::sync::OwnedRwLockWriteGuard<()>>,
    state: Arc<OwnerState>,
    config: Arc<Config>,
}

impl Drop for WriteFence {
    fn drop(&mut self) {
        let last = {
            let _admission = self
                .state
                .admission
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.state.fences.fetch_sub(1, Ordering::SeqCst) == 1
        };
        drop(self.guard.take());
        if last && tokio::runtime::Handle::try_current().is_ok() {
            schedule_all(self.config.clone());
        }
    }
}

/// Stops each owner partition after its current mutation, durably clears
/// earlier pending writes, and fences subsequent writes through explicit erase.
/// Other owners sharing a workspace keep independent queues and barriers.
pub(crate) async fn fence_and_clear(config: &Config) -> MemoryResult<WriteFence> {
    let state = owner_state(config);
    let fencing_generation = {
        let _admission = state
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.fences.fetch_add(1, Ordering::SeqCst);
        let next = FENCE_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1;
        state.generation.store(next, Ordering::SeqCst);
        next
    };
    let mut fence = WriteFence {
        guard: None,
        state: state.clone(),
        config: Arc::new(config.clone()),
    };
    let snapshot_config = config.clone();
    let prior = crate::core::runtime::spawn_blocking_scoped(move || {
        let _guard = LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        partitions(&snapshot_config)?
            .into_iter()
            .map(|file| {
                let tickets = read_at(&file)?
                    .into_iter()
                    .filter(|entry| {
                        entry.epoch.as_ref().is_none_or(|(process, generation)| {
                            process != &*PROCESS_EPOCH || *generation < fencing_generation
                        })
                    })
                    .map(|entry| entry.ticket)
                    .collect::<HashSet<_>>();
                Ok::<_, MemoryError>((file, tickets))
            })
            .collect::<MemoryResult<Vec<_>>>()
    })
    .await
    .map_err(|_| unavailable())??;
    fence.guard = Some(state.barrier.clone().write_owned().await);
    crate::core::runtime::spawn_blocking_scoped(move || {
        let _guard = LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (file, tickets) in prior {
            let mut entries = read_at(&file)?;
            entries.retain(|entry| !tickets.contains(&entry.ticket));
            write_at(&file, &entries)?;
        }
        Ok::<_, MemoryError>(())
    })
    .await
    .map_err(|_| unavailable())??;
    Ok(fence)
}

/// Starts a scoped background drain, retaining config only in memory.
pub(super) fn schedule(config: Arc<Config>) {
    let file = queue_path(&config, super::scope::resolve_current(&config).root());
    schedule_partition(config, file);
}

/// Retries all durable partitions belonging to this owner on auth/cron ticks.
pub(super) fn schedule_all(config: Arc<Config>) {
    if config.memory.engine == super::engine::DISABLED_ENGINE {
        return;
    }
    crate::core::runtime::spawn_scoped(async move {
        let scan_config = config.clone();
        let files =
            match crate::core::runtime::spawn_blocking_scoped(move || partitions(&scan_config))
                .await
            {
                Ok(Ok(files)) => files,
                _ => {
                    tracing::debug!(
                        code = "QUEUE_UNAVAILABLE",
                        "[memory:tool_writes] retry discovery deferred"
                    );
                    return;
                }
            };
        for file in files {
            schedule_partition(config.clone(), file);
        }
    });
}

fn schedule_partition(config: Arc<Config>, file: PathBuf) {
    let owner = owner_state(&config);
    let (generation, owner_guard) = {
        let _admission = owner
            .admission
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if owner.fences.load(Ordering::SeqCst) > 0 {
            return;
        }
        let generation = owner.generation.load(Ordering::SeqCst);
        let Ok(guard) = owner.barrier.clone().try_read_owned() else {
            return;
        };
        (generation, guard)
    };
    let state = drain_state(&file);
    let Ok(guard) = state.barrier.clone().try_lock_owned() else {
        return;
    };
    crate::core::runtime::spawn_scoped(async move {
        let _guard = guard;
        let _owner_guard = owner_guard;
        let _state = state;
        drain_locked(&config, &file, &owner, generation).await;
    });
}

/// Retries oldest first, retaining a failed write and all later writes.
/// Producers hold no queue lock during remote I/O. Also used by auth/cron
/// after restart; the persisted owner root must still match the active user.
#[cfg(test)]
pub(crate) async fn drain(config: &Config) -> usize {
    let file = queue_path(config, super::scope::resolve_current(config).root());
    let owner = owner_state(config);
    let _owner = owner.barrier.read().await;
    let state = drain_state(&file);
    let _draining = state.barrier.lock().await;
    let generation = owner.generation.load(Ordering::SeqCst);
    drain_locked(config, &file, &owner, generation).await
}

async fn drain_locked(config: &Config, file: &Path, state: &OwnerState, generation: u64) -> usize {
    if config.memory.engine == super::engine::DISABLED_ENGINE {
        return 0;
    }
    let mut settled = 0;
    // Bounded even if a producer continually adds new work.
    for _ in 0..MAX_PENDING {
        if state.generation.load(Ordering::SeqCst) != generation {
            break;
        }
        let snapshot_file = file.to_path_buf();
        let entry = crate::core::runtime::spawn_blocking_scoped(move || {
            let _guard = LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match read_at(&snapshot_file) {
                Ok(entries) => entries.into_iter().next(),
                Err(_) => None,
            }
        })
        .await
        .ok()
        .flatten();
        let Some(entry) = entry else {
            break;
        };
        if state.generation.load(Ordering::SeqCst) != generation {
            break;
        }
        if entry.root != super::scope::user_root(config)
            || entry.layout_v3 != super::scope::layout_is_v3(config)
            || queue_path(config, &entry.scope_root) != file
        {
            break;
        }
        let operation = async {
            match &entry.operation {
                Operation::Learn { item } => {
                    let bound = super::engine::resolve(config).engine()?;
                    let receipt =
                        super::ops::store_on_with(&bound, item.clone(), WriteOptions::accepted())
                            .await?;
                    let category = match item {
                        StoreItem::Learning { kind, .. } => kind.as_str(),
                        _ => "learning",
                    };
                    crate::core::bus::BUS.publish(crate::core::events::DomainEvent::MemoryStored {
                        key: receipt.id.0,
                        category: category.to_string(),
                        namespace: "learnings".to_string(),
                    });
                    Ok(())
                }
                Operation::Forget { ids, reach } => super::ops::forget(
                    config,
                    ForgetParams {
                        ids: ids.clone(),
                        reach: Some(reach.clone()),
                    },
                )
                .await
                .map(|_| ()),
            }
        };
        let result = tokio::time::timeout(WRITE_TIMEOUT, operation).await;
        let status = match result {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error.code().to_string()),
            Err(_) => Some("TIMEOUT".to_string()),
        };
        let settlement_file = file.to_path_buf();
        let ticket = entry.ticket.clone();
        let last_status = status.clone();
        let saved = crate::core::runtime::spawn_blocking_scoped(move || {
            let _guard = LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Ok(mut entries) = read_at(&settlement_file) else {
                return false;
            };
            if entries.first().is_none_or(|first| first.ticket != ticket) {
                return false;
            }
            if let Some(code) = last_status {
                entries[0].status = Some(code);
            } else {
                entries.remove(0);
            }
            write_at(&settlement_file, &entries).is_ok()
        })
        .await
        .unwrap_or(false);
        if let Some(code) = status {
            tracing::debug!(code, ticket = %entry.ticket, "[memory:tool_writes] pending write retained");
            break;
        }
        if !saved {
            break;
        }
        settled += 1;
        super::lifecycle::prefetch::invalidate(config);
    }
    settled
}

#[cfg(test)]
#[path = "tool_writes_tests.rs"]
mod tests;
