//! [`ProfileHost`]: the profiles a SaaS process has open.
//!
//! A profile is opened lazily on first use and kept until it has been idle
//! for [`SaasConfig::idle_evict_secs`] or the host needs its slot
//! ([`SaasConfig::max_profiles_open`]). A profile still in use — anyone holding
//! its [`Profile`], or a turn running on its context — is never evicted.
//!
//! Opening a profile takes its lease first ([`super::lease`]), so exactly one
//! process hosts it: another node's live lease makes [`ProfileHost::open`]
//! answer [`OpenError::HeldElsewhere`]. A lease taken over from a holder that
//! never released it (`previous_unclean`) means that holder's turns may have
//! died mid-flight; the profile's workspace is recovered before it opens.
//! Eviction, [`ProfileHost::release`] and deprovisioning release the lease;
//! losing it fences the profile ([`ProfileHost::fence`]).
//!
//! Each open profile carries its own [`CoreContext`], derived from the operator
//! context with the profile's forced config, its own security policy, and
//! `profile` and `session_agent` set to its id. Running work under that
//! context is what makes the session store, the config loader and the
//! per-thread tables resolve that user's state.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::layout::{self, ProfileLayout};
use super::lease::{self as profile_lease, OpenError};
use super::registry::ProfileRegistry;
use super::types::{ProfileId, ProfileMeta, ProfileSummary, LAYOUT_VERSION};
use crate::config::Config;
use crate::core::runtime::{ContextOverlay, CoreContext, DomainSet, SaasConfig};
use crate::storage::lease::{LeaseError, LeaseGrant, LeaseStore};
use crate::storage::StorageBackend;
use crate::tools::toolpacks::ToolGroups;

/// One open profile.
pub struct Profile {
    pub id: ProfileId,
    pub layout: ProfileLayout,
    pub config: Config,
    context: Arc<CoreContext>,
    fenced: AtomicBool,
}

impl std::fmt::Debug for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profile")
            .field("id", &self.id)
            .field("fenced", &self.is_fenced())
            .finish()
    }
}

impl Profile {
    /// The context every piece of this profile's work runs under.
    pub fn context(&self) -> &Arc<CoreContext> {
        &self.context
    }

    /// Whether this node lost the profile's lease: no new work may start.
    pub fn is_fenced(&self) -> bool {
        self.fenced.load(Ordering::SeqCst)
    }
}

struct Slot {
    state: Arc<Profile>,
    last_used: Instant,
    grant: LeaseGrant,
}

/// The open profiles of one SaaS process.
pub struct ProfileHost {
    saas: SaasConfig,
    operator: Arc<CoreContext>,
    node: String,
    open: Mutex<HashMap<ProfileId, Slot>>,
    /// Held across every operation that takes or gives back a lease (open,
    /// eviction, release, deprovision), so no two of them interleave on one
    /// profile. Lookups of an open profile never wait for it.
    gate: tokio::sync::Mutex<()>,
    leases: Arc<dyn LeaseStore>,
    registry: ProfileRegistry,
}

impl std::fmt::Debug for ProfileHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileHost")
            .field("root", &self.saas.root)
            .field("node", &self.node)
            .field("registry", &self.registry)
            .field("open", &self.open_count())
            .finish()
    }
}

/// The domain families a profile's context serves. Within them, only the
/// methods on [`USER_METHODS`](super::surface::USER_METHODS) are reachable.
pub fn user_domains() -> DomainSet {
    DomainSet {
        threads: true,
        channels: true,
        memory: true,
        ..DomainSet::none()
    }
}

impl ProfileHost {
    /// A host with no storage backend: `profile.toml` files and file-lock
    /// leases under `saas.root`.
    pub fn new(saas: SaasConfig, operator: Arc<CoreContext>) -> Self {
        Self::with_backend(saas, operator, None).expect("file-backed profile host")
    }

    /// A host over `backend` when given (the shared registry and cluster
    /// leases), else as [`Self::new`].
    ///
    /// # Errors
    ///
    /// When the backend refuses the cluster scope.
    pub fn with_backend(
        mut saas: SaasConfig,
        operator: Arc<CoreContext>,
        backend: Option<Arc<dyn StorageBackend>>,
    ) -> Result<Self, String> {
        let node = saas.resolve_node_id(None).to_owned();
        let leases = profile_lease::store_for(
            &saas.root,
            backend.as_ref(),
            &node,
            saas.advertise_url.clone(),
            saas.lease_ttl(),
        )?;
        let registry = match &backend {
            Some(backend) => ProfileRegistry::cluster(backend.as_ref())?,
            None => ProfileRegistry::files(&saas.root),
        };
        Ok(Self {
            saas,
            operator,
            node,
            open: Mutex::new(HashMap::new()),
            gate: tokio::sync::Mutex::new(()),
            leases,
            registry,
        })
    }

    /// The operator's settings this host runs with.
    pub fn saas(&self) -> &SaasConfig {
        &self.saas
    }

    /// This node's id in the profile leases.
    pub fn node(&self) -> &str {
        &self.node
    }

    pub(crate) fn leases(&self) -> &dyn LeaseStore {
        self.leases.as_ref()
    }

    /// Where profile `id`'s state lives, provisioned or not.
    pub fn layout_of(&self, id: &ProfileId) -> ProfileLayout {
        ProfileLayout::new(&self.saas.root, id)
    }

    /// Create profile `id`'s directories and record it. Returns whether it
    /// was new.
    pub async fn provision(&self, id: &ProfileId) -> Result<bool, String> {
        let _gate = self.gate.lock().await;
        if self.registry.get(id).await?.is_some() {
            log::debug!("[profiles] provision profile={id}: already provisioned");
            return Ok(false);
        }
        create_dirs(&self.layout_of(id))?;
        let meta = ProfileMeta {
            profile_id: id.clone(),
            created_at: unix_now(),
            layout_version: LAYOUT_VERSION,
        };
        let created = self.registry.create(&meta).await?;
        log::info!("[profiles] provisioned profile={id} created={created}");
        Ok(created)
    }

    /// Close profile `id` and archive its state under `<root>/deprovisioned/`.
    /// Nothing is deleted. Returns whether there was such a profile.
    ///
    /// A profile still in use here, or hosted by another node, is not
    /// archived from under it: deprovisioning fails and can be retried (on
    /// the other node, or after `profiles.release` there).
    pub async fn deprovision(&self, id: &ProfileId) -> Result<bool, String> {
        let _gate = self.gate.lock().await;
        let held = {
            let open = self.lock();
            match open.get(id) {
                Some(slot) if in_use(slot) => {
                    return Err(format!("profile {id} is in use; try again shortly"))
                }
                Some(slot) => Some(slot.grant.clone()),
                None => None,
            }
        };
        let layout = self.layout_of(id);
        let recorded = self.registry.get(id).await?.is_some();
        if !recorded && !layout.dir.exists() {
            return Ok(false);
        }
        let grant = match held {
            Some(grant) => grant,
            None => match self
                .leases
                .acquire(id.as_str(), profile_lease::now_ms())
                .await
            {
                Ok(grant) => grant,
                Err(LeaseError::Held(record)) => {
                    return Err(format!(
                        "profile {id} is hosted by node {}; release it there first",
                        record.owner
                    ))
                }
                Err(error) => return Err(format!("profile {id}: {error}")),
            },
        };
        self.lock().remove(id);
        // Credential secrets live in the process keyring under the profile id,
        // not only in the profile's directory, so archiving the directory alone
        // would let a re-provisioned profile pick the old credential back up.
        let config = layout::profile_config(&layout, id);
        let cleared = CoreContext::scope(self.records_context(id), async {
            super::credentials::clear(&config)
        })
        .await;
        if let Err(e) = cleared {
            log::warn!("[profiles] clearing credentials before archiving failed: {e}");
            // Keep the profile so cleanup can be retried; archiving now would
            // leave the secret for a re-provisioned profile to inherit. It is
            // closed here, so give its lease back for the retry to take.
            profile_lease::release_all(self.leases(), vec![(id.clone(), grant)]).await;
            return Err(format!("clearing credentials before archiving: {e}"));
        }
        if layout.dir.exists() {
            let archive = layout::archive_dir(&self.saas.root);
            std::fs::create_dir_all(&archive)
                .map_err(|e| format!("creating {}: {e}", archive.display()))?;
            // Unique even when one user is deprovisioned twice in a second.
            let dest = archive.join(format!(
                "{id}-{}-{}",
                unix_now(),
                uuid::Uuid::new_v4().simple()
            ));
            std::fs::rename(&layout.dir, &dest)
                .map_err(|e| format!("archiving {}: {e}", layout.dir.display()))?;
        }
        self.registry.remove(id).await?;
        profile_lease::release_all(self.leases(), vec![(id.clone(), grant)]).await;
        log::info!("[profiles] deprovisioned profile={id} (archived)");
        Ok(true)
    }

    /// A context acting for profile `id` without opening it: its forced
    /// config and its tenant key, so profile-scoped storage (credentials in a
    /// storage backend) resolves to that profile. For operator-plane work on
    /// a profile's records; it carries no policy and runs no turns.
    pub(crate) fn records_context(&self, id: &ProfileId) -> Arc<CoreContext> {
        let config = layout::profile_config(&self.layout_of(id), id);
        self.operator.derive_with(
            ContextOverlay::new(config, user_domains(), ToolGroups::none())
                .without_user_skill_roots()
                .session_agent(id.as_str())
                .profile(id.as_str()),
        )
    }

    /// The forced config of provisioned profile `id`, without opening it (and
    /// so without taking a profile slot or its lease).
    pub async fn provisioned_config(&self, id: &ProfileId) -> Result<Config, String> {
        if self.registry.get(id).await?.is_none() {
            return Err(format!("profile {id} is not provisioned"));
        }
        Ok(layout::profile_config(&self.layout_of(id), id))
    }

    /// Profile `id`, opening it — and taking its lease — if it is provisioned
    /// and not open yet.
    pub async fn open(&self, id: &ProfileId) -> Result<Arc<Profile>, OpenError> {
        if let Some(state) = self.touch(id) {
            // Every open sweeps profiles idle past `idle_evict_secs`, so they
            // close even when no new user arrives; skipped while another open
            // holds the gate rather than waiting for it.
            if let Ok(_gate) = self.gate.try_lock() {
                let closed = self.sweep_idle_locked(&mut self.lock(), Instant::now());
                profile_lease::release_all(self.leases(), closed).await;
            }
            return Ok(state);
        }
        let _gate = self.gate.lock().await;
        if let Some(state) = self.touch(id) {
            return Ok(state);
        }
        if self
            .registry
            .get(id)
            .await
            .map_err(OpenError::Storage)?
            .is_none()
        {
            return Err(OpenError::NotProvisioned(id.clone()));
        }
        let (closed, full) = {
            let mut open = self.lock();
            let closed = self.evict_locked(&mut open, Instant::now());
            (closed, open.len() >= self.saas.max_profiles_open.max(1))
        };
        profile_lease::release_all(self.leases(), closed).await;
        if full {
            return Err(OpenError::Full {
                max: self.saas.max_profiles_open,
            });
        }

        let grant = self
            .leases
            .acquire(id.as_str(), profile_lease::now_ms())
            .await
            .map_err(|error| {
                log::debug!("[profiles] lease of profile={id} refused: {error}");
                OpenError::from(error)
            })?;
        let layout = self.layout_of(id);
        if let Err(error) = create_dirs(&layout) {
            profile_lease::release_all(self.leases(), vec![(id.clone(), grant)]).await;
            return Err(OpenError::Storage(error));
        }
        let config = layout::profile_config(&layout, id);
        let context = self.operator.derive_with(
            ContextOverlay::new(config.clone(), user_domains(), ToolGroups::none())
                .without_user_skill_roots()
                .session_agent(id.as_str())
                .profile(id.as_str())
                .agent_policy(profile_policy(&config)),
        );
        if grant.previous_unclean {
            log::info!(
                "[profiles] profile={id} taken over from a holder that never released it (epoch {}); recovering",
                grant.epoch
            );
            recover_workspace(id, &layout.workspace_dir);
            recover_session_store(id, &context);
        }
        crate::platform::cost::seed_tenant_tracker(&context, &config);
        let state = Arc::new(Profile {
            id: id.clone(),
            layout,
            config,
            context,
            fenced: AtomicBool::new(false),
        });
        let mut open = self.lock();
        open.insert(
            id.clone(),
            Slot {
                state: Arc::clone(&state),
                last_used: Instant::now(),
                grant,
            },
        );
        log::debug!(
            "[profiles] opened profile={id} node={} ({} open)",
            self.node,
            open.len()
        );
        Ok(state)
    }

    /// The open state of `id`, its last use bumped.
    fn touch(&self, id: &ProfileId) -> Option<Arc<Profile>> {
        let mut open = self.lock();
        let slot = open.get_mut(id)?;
        slot.last_used = Instant::now();
        Some(Arc::clone(&slot.state))
    }

    /// Profile `id` if it is open.
    pub fn get(&self, id: &ProfileId) -> Option<Arc<Profile>> {
        self.lock().get(id).map(|slot| Arc::clone(&slot.state))
    }

    pub fn is_open(&self, id: &ProfileId) -> bool {
        self.lock().contains_key(id)
    }

    pub fn open_count(&self) -> usize {
        self.lock().len()
    }

    /// Every provisioned profile, open here or not.
    pub async fn list(&self) -> Result<Vec<ProfileSummary>, String> {
        let mut summaries = Vec::new();
        for meta in self.registry.list().await? {
            summaries.push(self.summarize(meta).await);
        }
        Ok(summaries)
    }

    /// Profile `id`, or `None` when it is not provisioned.
    pub async fn summary(&self, id: &ProfileId) -> Result<Option<ProfileSummary>, String> {
        match self.registry.get(id).await? {
            Some(meta) => Ok(Some(self.summarize(meta).await)),
            None => Ok(None),
        }
    }

    async fn summarize(&self, meta: ProfileMeta) -> ProfileSummary {
        let id = meta.profile_id;
        let config = layout::profile_config(&self.layout_of(&id), &id);
        let has_credential = CoreContext::scope(self.records_context(&id), async {
            super::credentials::has(&config)
        })
        .await;
        ProfileSummary {
            open: self.is_open(&id),
            has_credential,
            profile_id: id,
            created_at: meta.created_at,
        }
    }

    /// Close profiles idle past the configured limit that nobody holds, and
    /// release their leases.
    pub async fn evict_idle(&self) {
        let _gate = self.gate.lock().await;
        let closed = self.evict_locked(&mut self.lock(), Instant::now());
        profile_lease::release_all(self.leases(), closed).await;
    }

    /// Close profile `id` here and release its lease, so another node can
    /// host it at once. Returns whether it was open here. A profile in use
    /// is not released from under its work.
    pub async fn release(&self, id: &ProfileId) -> Result<bool, String> {
        let _gate = self.gate.lock().await;
        let grant = {
            let mut open = self.lock();
            match open.get(id) {
                None => return Ok(false),
                Some(slot) if in_use(slot) => {
                    return Err(format!("profile {id} is in use; try again shortly"))
                }
                Some(_) => open.remove(id).map(|slot| slot.grant),
            }
        };
        let grant = grant.expect("checked above");
        self.leases.release(grant).await.map_err(|error| {
            log::warn!("[profiles] releasing profile={id} failed: {error}");
            format!("releasing profile {id}: {error}")
        })?;
        log::info!("[profiles] released profile={id} node={}", self.node);
        Ok(true)
    }

    /// Release the lease of every profile nothing is using: a clean shutdown.
    /// A profile with work still running keeps its lease, so the next holder
    /// treats it as an unclean hand-over and recovers its turns.
    pub async fn release_idle_on_shutdown(&self) {
        let _gate = self.gate.lock().await;
        let closed: Vec<_> = {
            let mut open = self.lock();
            let idle: Vec<ProfileId> = open
                .iter()
                .filter(|(_, slot)| !in_use(slot))
                .map(|(id, _)| id.clone())
                .collect();
            idle.into_iter()
                .filter_map(|id| open.remove(&id).map(|slot| (id, slot.grant)))
                .collect()
        };
        log::info!(
            "[profiles] shutdown: releasing {} idle profile lease(s), {} still busy",
            closed.len(),
            self.open_count()
        );
        profile_lease::release_all(self.leases(), closed).await;
    }

    /// The current grant of every open profile.
    pub(crate) fn grants(&self) -> Vec<(ProfileId, LeaseGrant)> {
        self.lock()
            .iter()
            .map(|(id, slot)| (id.clone(), slot.grant.clone()))
            .collect()
    }

    /// Replace `id`'s grant `old` with `renewed`, unless the slot moved on
    /// (closed, or re-opened under a newer grant) meanwhile.
    pub(crate) fn store_grant(&self, id: &ProfileId, old: &LeaseGrant, renewed: LeaseGrant) {
        if let Some(slot) = self.lock().get_mut(id) {
            if slot.grant == *old {
                slot.grant = renewed;
            }
        }
    }

    /// This node lost profile `id`: mark it fenced so no new work starts,
    /// close it, and stop the turns it is running. `grant` names the lease
    /// that was lost; a slot re-opened under a newer one is left alone.
    pub async fn fence(&self, id: &ProfileId, grant: &LeaseGrant, why: &str) {
        let state = {
            let mut open = self.lock();
            match open.get(id) {
                Some(slot) if slot.grant == *grant => open.remove(id).map(|slot| slot.state),
                _ => None,
            }
        };
        let Some(state) = state else {
            log::debug!("[profiles] fence profile={id}: already closed or re-opened");
            return;
        };
        state.fenced.store(true, Ordering::SeqCst);
        log::warn!(
            "[profiles] fenced profile={id} node={} ({why}); stopping its turns",
            self.node
        );
        let stopped = CoreContext::scope(
            Arc::clone(state.context()),
            crate::web_chat::cancel_all_turns(),
        )
        .await;
        log::info!("[profiles] fenced profile={id}: stopped {stopped} thread(s)");
    }

    /// Close profiles idle past `idle_evict_secs` that nothing is using.
    /// Returns their grants for the caller to release.
    fn sweep_idle_locked(
        &self,
        open: &mut HashMap<ProfileId, Slot>,
        now: Instant,
    ) -> Vec<(ProfileId, LeaseGrant)> {
        let idle_limit = Duration::from_secs(self.saas.idle_evict_secs);
        let idle: Vec<ProfileId> = open
            .iter()
            .filter(|(_, slot)| !in_use(slot) && now.duration_since(slot.last_used) >= idle_limit)
            .map(|(id, _)| id.clone())
            .collect();
        idle.into_iter()
            .filter_map(|id| {
                log::debug!("[profiles] evicted idle profile={id}");
                open.remove(&id).map(|slot| (id, slot.grant))
            })
            .collect()
    }

    fn evict_locked(
        &self,
        open: &mut HashMap<ProfileId, Slot>,
        now: Instant,
    ) -> Vec<(ProfileId, LeaseGrant)> {
        let mut closed = self.sweep_idle_locked(open, now);
        // Still full: make room by closing the least recently used idle one.
        if open.len() >= self.saas.max_profiles_open.max(1) {
            let victim = open
                .iter()
                .filter(|(_, slot)| !in_use(slot))
                .min_by_key(|(_, slot)| slot.last_used)
                .map(|(id, _)| id.clone());
            if let Some(id) = victim {
                if let Some(slot) = open.remove(&id) {
                    log::debug!("[profiles] evicted least recently used profile={id}");
                    closed.push((id, slot.grant));
                }
            }
        }
        closed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ProfileId, Slot>> {
        self.open.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Whether anything still uses an open profile: a request holding its state,
/// or a turn still running on its context (a detached turn outlives the
/// request that started it).
fn in_use(slot: &Slot) -> bool {
    Arc::strong_count(&slot.state) > 1 || slot.state.context.tenant_in_use()
}

fn create_dirs(layout: &ProfileLayout) -> Result<(), String> {
    for dir in [&layout.workspace_dir, &layout.sandbox_dir] {
        std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    }
    Ok(())
}

/// The security policy profile work is gated by: its own forced autonomy over
/// its workspace and sandbox, never the operator's live policy.
fn profile_policy(config: &Config) -> Arc<crate::security::SecurityPolicy> {
    Arc::new(
        crate::security::SecurityPolicy::from_config(
            &config.autonomy,
            &config.workspace_dir,
            &config.action_dir,
        )
        .with_privacy_mode(config.privacy.mode),
    )
}

/// Settle what a previous holder left in profile `id`'s workspace: turns that
/// were mid-flight become interrupted and run-ledger rows left running are
/// closed. The sweep a single-user core runs at boot, run when a profile's
/// lease is taken over from a holder that never released it. Failures are
/// logged; the profile still opens.
pub(crate) fn recover_workspace(id: &ProfileId, workspace_dir: &std::path::Path) {
    let now = chrono::Utc::now().to_rfc3339();
    match tinyagents_session::turn_state::store::mark_all_interrupted(
        workspace_dir.to_path_buf(),
        &now,
    ) {
        Ok(0) => {}
        Ok(turns) => log::info!("[profiles] profile={id} recovered {turns} interrupted turn(s)"),
        Err(error) => log::warn!("[profiles] profile={id} turn recovery failed: {error}"),
    }
    match tinyagents_session::run_ledger::interrupt_orphaned_agent_runs(workspace_dir) {
        Ok(0) => {}
        Ok(runs) => log::info!("[profiles] profile={id} settled {runs} orphaned run(s)"),
        Err(error) => log::warn!("[profiles] profile={id} run recovery failed: {error:#}"),
    }
}

/// [`recover_workspace`] for turn states kept in a storage-backed session
/// store, which the workspace sweep cannot see: the profile's default
/// session key is swept through the installed provider.
fn recover_session_store(id: &ProfileId, context: &Arc<CoreContext>) {
    let Some(provider) = crate::agent::session_store::installed() else {
        return;
    };
    if provider.workspace_dir().is_some() {
        // File-backed: the workspace sweep already covered it.
        return;
    }
    let key = crate::core::runtime::session_key(&crate::core::runtime::Tenant::of(context));
    let now = chrono::Utc::now().to_rfc3339();
    match provider
        .for_agent(&key)
        .turn_states
        .mark_all_interrupted(&now)
    {
        Ok(0) => {}
        Ok(turns) => {
            log::info!("[profiles] profile={id} recovered {turns} interrupted stored turn(s)");
        }
        Err(error) => log::warn!("[profiles] profile={id} stored turn recovery failed: {error}"),
    }
}

static HOST: OnceLock<Arc<ProfileHost>> = OnceLock::new();

/// Install the process's profile host. A SaaS boot does this once; later calls
/// are ignored.
pub fn install(host: Arc<ProfileHost>) {
    if HOST.set(host).is_err() {
        log::warn!("[profiles] profile host already installed; keeping the first");
    }
}

/// The process's profile host, when this is a SaaS process.
pub fn host() -> Option<Arc<ProfileHost>> {
    HOST.get().cloned()
}

/// The profile the current work runs for, if any.
pub fn current() -> Option<Arc<Profile>> {
    let profile = crate::core::runtime::current_tenant().ok()?.profile?;
    let id = ProfileId::parse(&profile).ok()?;
    host()?.get(&id)
}

/// Refuse to start work for a profile this node no longer hosts: one whose
/// lease was lost (fenced) or that was closed under a request still running
/// for it. `Ok` outside SaaS, and for work that serves no profile.
pub fn ensure_hosted() -> Result<(), String> {
    let Some(host) = host() else {
        return Ok(());
    };
    let Some(profile) = crate::core::runtime::current_tenant()
        .ok()
        .and_then(|tenant| tenant.profile)
    else {
        return Ok(());
    };
    let hosted = ProfileId::parse(&profile)
        .ok()
        .and_then(|id| host.get(&id))
        .is_some_and(|state| !state.is_fenced());
    if hosted {
        Ok(())
    } else {
        log::warn!("[profiles] refused new work for profile={profile}: not hosted by this node");
        Err("this profile is no longer hosted by this node; retry the request".to_string())
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
