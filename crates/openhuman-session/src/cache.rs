//! The current-user cache: the last `/auth/me` answer, served fresh within a
//! short TTL, stale-while-revalidate after it, and a negative cache with a
//! bounded backoff so an unreachable backend is not re-paid the fetch timeout
//! on every poll (#5624, #5930, #6180).
//!
//! Ported from the core's `desktop::app_state::ops::{current_user,
//! current_user_generation, staleness, auth_timeout}`. Keyed on
//! `(base_url, secret)` throughout so one identity's freshness or outage is
//! never reported as another's.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::client::{FetchMeError, SessionClient};
use crate::credential::Credential;

const LOG_PREFIX: &str = "[session][cache]";

/// How long a positive `/auth/me` answer is served without asking again.
pub const REFRESH_TTL: Duration = Duration::from_secs(5);
/// Ceiling on the negative-cache backoff.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Floor under the first backoff step, independent of the fetch timeout.
const BACKOFF_BASE_FLOOR: Duration = Duration::from_secs(10);

/// Wall-clock budget for one refresh when nothing overrides it.
pub const DEFAULT_FETCH_TIMEOUT_SECS: u64 = 5;
pub const MIN_FETCH_TIMEOUT_SECS: u64 = 2;
pub const MAX_FETCH_TIMEOUT_SECS: u64 = 12;
/// Operator override for the refresh budget, in seconds (clamped 2..=12).
pub const FETCH_TIMEOUT_ENV: &str = "OPENHUMAN_AUTH_FETCH_TIMEOUT_SECS";

/// Parse a raw override into a bounded timeout in seconds.
pub fn parse_fetch_timeout_secs(raw: Option<&str>) -> u64 {
    raw.map(str::trim)
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|n| (MIN_FETCH_TIMEOUT_SECS..=MAX_FETCH_TIMEOUT_SECS).contains(n))
        .unwrap_or(DEFAULT_FETCH_TIMEOUT_SECS)
}

/// The effective refresh budget.
pub fn fetch_timeout() -> Duration {
    Duration::from_secs(parse_fetch_timeout_secs(
        std::env::var(FETCH_TIMEOUT_ENV).ok().as_deref(),
    ))
}

/// First backoff step: at least the floor, and at least twice the fetch
/// timeout, so widening the timeout cannot re-open the poll treadmill.
pub fn backoff_base_for(fetch_timeout: Duration) -> Duration {
    BACKOFF_BASE_FLOOR.max(fetch_timeout.saturating_mul(2))
}

/// How long a run of `consecutive` failures suppresses the next live attempt.
pub fn backoff_for(consecutive: u32, fetch_timeout: Duration) -> Duration {
    let steps = consecutive.saturating_sub(1).min(16);
    backoff_base_for(fetch_timeout)
        .saturating_mul(2u32.saturating_pow(steps))
        .min(BACKOFF_MAX)
}

/// What the cache hands back: the user (if the backend returned one) and
/// how stale the data being served is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedUser {
    pub user: Option<Value>,
    /// The backend could not refresh this identity recently.
    pub stale: bool,
    /// Seconds since the backend last returned a user for this identity;
    /// `None` when it never has in this process.
    pub stale_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Key {
    base: String,
    secret: String,
}

#[derive(Debug, Clone)]
struct Positive {
    key: Key,
    fetched_at: Instant,
    user: Value,
}

#[derive(Debug, Clone)]
struct Failure {
    key: Key,
    failed_at: Instant,
    consecutive: u32,
    error: FetchMeError,
}

#[derive(Debug, Default)]
struct State {
    positive: Option<Positive>,
    failure: Option<Failure>,
    last_success: Option<(Key, Instant)>,
    /// Bumped by [`CurrentUserCache::forget`]; a refresh that started under an
    /// older generation must not commit.
    generation: u64,
}

struct Inner {
    state: Mutex<State>,
    /// Single-flight gate for the background refresh.
    inflight: tokio::sync::Mutex<()>,
}

/// Cheap to clone: every clone shares the same cache.
#[derive(Clone)]
pub struct CurrentUserCache {
    inner: Arc<Inner>,
}

impl Default for CurrentUserCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CurrentUserCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(State::default()),
                inflight: tokio::sync::Mutex::new(()),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn key(client: &SessionClient, credential: &Credential) -> Key {
        Key {
            base: client.base_url().to_string(),
            secret: credential.secret.clone(),
        }
    }

    /// Drop everything and bump the generation, so a refresh already in flight
    /// for the previous identity cannot commit. Call on logout.
    pub fn forget(&self) {
        let mut state = self.lock();
        state.generation = state.generation.wrapping_add(1);
        state.positive = None;
        state.failure = None;
        state.last_success = None;
    }

    /// Seed the positive entry with a user just fetched elsewhere (store-time
    /// validation), so the next poll is served from cache instead of paying a
    /// second round trip for the same answer.
    pub fn seed(&self, client: &SessionClient, credential: &Credential, user: Value) {
        let key = Self::key(client, credential);
        let mut state = self.lock();
        state.generation = state.generation.wrapping_add(1);
        state.positive = Some(Positive {
            key: key.clone(),
            fetched_at: Instant::now(),
            user,
        });
        state.failure = None;
        state.last_success = Some((key, Instant::now()));
    }

    /// The cached user regardless of TTL or identity. For prompt/identity
    /// peeks where a slightly stale answer is fine.
    pub fn peek(&self) -> Option<Value> {
        self.lock().positive.as_ref().map(|p| p.user.clone())
    }

    fn cached(&self, key: &Key) -> Option<(Value, Duration)> {
        let state = self.lock();
        let entry = state.positive.as_ref()?;
        (entry.key == *key).then(|| (entry.user.clone(), entry.fetched_at.elapsed()))
    }

    fn suppressed(&self, key: &Key) -> Option<(FetchMeError, u32, Duration)> {
        let state = self.lock();
        let entry = state.failure.as_ref()?;
        if entry.key != *key {
            return None;
        }
        let window = backoff_for(entry.consecutive, fetch_timeout());
        let elapsed = entry.failed_at.elapsed();
        (elapsed < window).then(|| (entry.error.clone(), entry.consecutive, window - elapsed))
    }

    /// [`Self::suppressed`], mapped to the error a caller should see: a
    /// rejection is authoritative and returned as-is (a stale-while-revalidate
    /// caller cannot observe a background result directly, so it must be
    /// retained until the next read hands it to the owner); anything else
    /// suppressed under the backoff window comes back as
    /// [`FetchMeError::Suppressed`].
    fn suppressed_error(&self, key: &Key) -> Option<FetchMeError> {
        let (error, consecutive, retry_in) = self.suppressed(key)?;
        if matches!(error, FetchMeError::Rejected(_)) {
            return Some(error);
        }
        Some(FetchMeError::Suppressed {
            message: error.message().to_string(),
            consecutive,
            retry_in,
        })
    }

    fn staleness(&self, key: &Key) -> (bool, Option<u64>) {
        let state = self.lock();
        let stale = state.failure.as_ref().is_some_and(|f| f.key == *key);
        let age = state
            .last_success
            .as_ref()
            .filter(|(k, _)| k == key)
            .map(|(_, at)| at.elapsed().as_secs());
        (stale, age)
    }

    fn record_failure(&self, generation: u64, key: &Key, error: FetchMeError) {
        let mut state = self.lock();
        if state.generation != generation {
            log::debug!("{LOG_PREFIX} discarding failure that raced sign-out");
            return;
        }
        let consecutive = match state.failure.as_ref() {
            Some(f) if f.key == *key => f.consecutive.saturating_add(1),
            _ => 1,
        };
        state.failure = Some(Failure {
            key: key.clone(),
            failed_at: Instant::now(),
            consecutive,
            error,
        });
    }

    fn with_result(&self, key: &Key, user: Option<Value>) -> CachedUser {
        let (stale, stale_seconds) = self.staleness(key);
        CachedUser {
            user,
            stale,
            stale_seconds,
        }
    }

    /// The user for `credential`, from cache when fresh, else from the backend.
    ///
    /// * fresh positive entry → served as-is;
    /// * expired positive entry → served stale while a background refresh runs;
    /// * open backoff window → [`FetchMeError::Suppressed`] without a request;
    /// * otherwise a blocking refresh bounded by [`fetch_timeout`].
    ///
    /// `force` skips every cache and goes to the network.
    pub async fn get_or_refresh(
        &self,
        client: &SessionClient,
        credential: &Credential,
        force: bool,
    ) -> Result<CachedUser, FetchMeError> {
        let key = Self::key(client, credential);
        let generation = self.lock().generation;

        if !force {
            // A rejection is authoritative regardless of what else is cached:
            // a stale-while-revalidate caller cannot observe a background
            // rejection directly, so it must be surfaced now rather than
            // served through on a positive entry that predates it.
            if let Some((error, _, _)) = self.suppressed(&key) {
                if matches!(error, FetchMeError::Rejected(_)) {
                    return Err(error);
                }
            }
            if let Some((user, age)) = self.cached(&key) {
                if age < REFRESH_TTL {
                    return Ok(self.with_result(&key, Some(user)));
                }
                self.spawn_refresh(client, credential, generation);
                log::debug!(
                    "{LOG_PREFIX} serving expired current user age_ms={} while refreshing",
                    age.as_millis()
                );
                return Ok(self.with_result(&key, Some(user)));
            }
            // No positive entry to fall back on: only now does an open
            // availability-backoff window turn into an error, instead of
            // pre-empting perfectly good cached data for the whole window
            // (#6318 review follow-up).
            if let Some(error) = self.suppressed_error(&key) {
                return Err(error);
            }
        }

        // The first caller owns the network refresh. Concurrent cache misses
        // (including forced refreshes) wait for it, then reuse its result.
        // This avoids multiplying `/auth/me` requests during startup/polling.
        let waited_for_refresh = self.inner.inflight.try_lock().is_err();
        let _refresh_guard = self.inner.inflight.lock().await;
        if waited_for_refresh {
            if let Some((user, _)) = self.cached(&key) {
                return Ok(self.with_result(&key, Some(user)));
            }
            // The caller we waited behind may have failed rather than
            // succeeded — its failure is recorded under the same lock this
            // caller just acquired. Recheck suppression before starting a
            // second sequential request: without this, every waiter behind a
            // failed first refresh redoes the same doomed call instead of
            // observing the backoff window that call just opened, turning
            // one outage into N serialized timeouts (#6318).
            if let Some(error) = self.suppressed_error(&key) {
                return Err(error);
            }
        }
        let timeout = fetch_timeout();
        match tokio::time::timeout(
            timeout,
            self.refresh_now(client, credential, generation, false),
        )
        .await
        {
            Ok(Ok(user)) => Ok(self.with_result(&key, user)),
            Ok(Err(error)) => Err(error),
            Err(_) => {
                let error = FetchMeError::Transport(format!(
                    "request timed out after {}s",
                    timeout.as_secs()
                ));
                self.record_failure(generation, &key, error.clone());
                Err(error)
            }
        }
    }

    fn spawn_refresh(&self, client: &SessionClient, credential: &Credential, generation: u64) {
        let key = Self::key(client, credential);
        if let Some((error, consecutive, remaining)) = self.suppressed(&key) {
            log::debug!(
                "{LOG_PREFIX} not refreshing behind the poll; backend failed {consecutive}x, retrying in {}ms: {}",
                remaining.as_millis(),
                error.message()
            );
            return;
        }
        let Ok(guard) = self.inner.inflight.try_lock() else {
            return;
        };
        // Move the guard into the task by re-taking it there; `try_lock` guards
        // borrow `self.inner`, so hand over an owned lock instead.
        drop(guard);
        let cache = self.clone();
        let client = client.clone();
        let credential = credential.clone();
        tokio::spawn(async move {
            let Ok(_guard) = cache.inner.inflight.try_lock() else {
                return;
            };
            let timeout = fetch_timeout();
            match tokio::time::timeout(
                timeout,
                cache.refresh_now(&client, &credential, generation, true),
            )
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => log::debug!(
                    "{LOG_PREFIX} background refresh failed; serving stale entry: {}",
                    error.message()
                ),
                Err(_) => {
                    let key = Self::key(&client, &credential);
                    cache.record_failure(
                        generation,
                        &key,
                        FetchMeError::Transport(format!(
                            "request timed out after {}s",
                            timeout.as_secs()
                        )),
                    );
                }
            }
        });
    }

    /// Go to the backend and reconcile the caches with what came back.
    async fn refresh_now(
        &self,
        client: &SessionClient,
        credential: &Credential,
        generation: u64,
        background: bool,
    ) -> Result<Option<Value>, FetchMeError> {
        let key = Self::key(client, credential);
        // The TTL clock starts when the request goes out, not when it lands,
        // so the refresh cadence is not stretched by the round trip.
        let started_at = Instant::now();
        let fetched = match client.fetch_me(credential).await {
            Ok(user) => sanitize_user(Some(user)),
            Err(error) => {
                self.record_failure(generation, &key, error.clone());
                return Err(error);
            }
        };

        let committed = {
            let mut state = self.lock();
            if state.generation != generation {
                false
            } else {
                let moved_on = state.positive.as_ref().is_some_and(|p| p.key != key);
                if background && moved_on {
                    false
                } else {
                    state.positive = fetched.clone().map(|user| Positive {
                        key: key.clone(),
                        fetched_at: started_at,
                        user,
                    });
                    state.failure = None;
                    if fetched.is_some() {
                        state.last_success = Some((key.clone(), Instant::now()));
                    }
                    true
                }
            }
        };
        if !committed {
            log::debug!(
                "{LOG_PREFIX} discarding refresh that raced sign-out or an identity switch"
            );
        }
        if committed {
            Ok(fetched)
        } else {
            Err(FetchMeError::Superseded)
        }
    }
}

fn sanitize_user(user: Option<Value>) -> Option<Value> {
    match user {
        Some(Value::Object(map)) if map.is_empty() => None,
        Some(Value::Null) => None,
        other => other,
    }
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
