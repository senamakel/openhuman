//! The current-user cache: the positive cache (last `/auth/me` answer),
//! the negative cache (recent availability failures and their backoff),
//! and the blocking/background refresh paths that keep both in sync with
//! the backend. [`current_user_fetch`](super::current_user_fetch) does the
//! actual HTTP call; [`current_user_generation`](super::current_user_generation)
//! owns sign-out invalidation of these caches; [`staleness`](super::staleness)
//! reports how old the data being served is.

use super::auth_timeout::{auth_fetch_timeout, current_user_backoff_base};
use super::current_user_fetch::{fetch_current_user, sanitize_snapshot_user};
use super::current_user_generation::{
    clear_current_user_failure_unless_stale, current_user_generation,
    note_current_user_success_unless_stale, record_current_user_failure_locked,
    record_current_user_failure_unless_stale,
};
use super::LOG_PREFIX;
use crate::api::config::effective_backend_api_url;
use crate::config::Config;
use log::debug;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde_json::Value;
use std::time::{Duration, Instant};

/// How long a positive `/auth/me` answer is served without asking the
/// backend again.
pub(super) const CURRENT_USER_REFRESH_TTL: Duration = Duration::from_secs(5);
/// Ceiling on the negative-cache backoff. Modest on purpose: this window is
/// time during which a recovered backend still will not be noticed, so it
/// trades a bounded amount of staleness for not stalling every poll. At the
/// cap a 5s poll loop attempts roughly one live fetch per twelve polls
/// instead of one per poll.
pub(super) const CURRENT_USER_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// The positive cache: the last successful `/auth/me` answer, keyed on
/// `(api_base, token)`.
pub(super) static CURRENT_USER_CACHE: Lazy<Mutex<Option<CachedCurrentUser>>> =
    Lazy::new(|| Mutex::new(None));
/// Negative counterpart to [`CURRENT_USER_CACHE`]: the last *availability*
/// failure against `auth_get_me`, so a client whose backend is unreachable stops
/// re-paying [`auth_fetch_timeout`] on every snapshot poll.
///
/// Kept separate from the positive cache rather than folded into it because the
/// two have different lifetimes and different readers —
/// [`peek_cached_current_user_identity`] must keep serving the last known
/// identity throughout an outage, and it reads only the positive cache.
pub(super) static CURRENT_USER_FAILURE: Lazy<Mutex<Option<CurrentUserFailure>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Debug, Clone)]
pub(super) struct CachedCurrentUser {
    pub(super) api_base: String,
    pub(super) token: String,
    pub(super) fetched_at: Instant,
    pub(super) user: Value,
}

#[derive(Debug, Clone)]
pub(super) enum CurrentUserFetchError {
    Rejected(String),
    TransientResponse(String),
    FetchFailed(String),
    /// A recorded availability failure replayed from the backoff window
    /// instead of going to the network (see
    /// [`suppressed_current_user_failure`]).
    ///
    /// Carries the original error so callers that only want the message behave
    /// exactly as before, plus the two numbers that distinguish "the backend
    /// just failed" from "we did not ask the backend". Without this the
    /// snapshot caller logs a replay — which costs microseconds and makes no
    /// request — with the same `WARN … refresh failed` wording as a real 5s
    /// timeout, so a healthy backoff reads in the logs like a hammering loop.
    /// That misreading is what #5930 was filed on.
    Suppressed {
        inner: Box<CurrentUserFetchError>,
        /// Length of the failure run that opened the window.
        consecutive: u32,
        /// How long until the next live attempt is allowed.
        retry_in: Duration,
    },
}

impl CurrentUserFetchError {
    pub(super) fn message(&self) -> &str {
        match self {
            CurrentUserFetchError::Rejected(message)
            | CurrentUserFetchError::TransientResponse(message)
            | CurrentUserFetchError::FetchFailed(message) => message,
            CurrentUserFetchError::Suppressed { inner, .. } => inner.message(),
        }
    }
}

impl CurrentUserFetchError {
    /// Whether this failure says the *backend was not reachable or not healthy*,
    /// as opposed to saying something about our credentials.
    ///
    /// Only these are worth backing off. A [`Rejected`](Self::Rejected) is the
    /// backend answering, in time, that the token is no good — it drives the
    /// deferred-session cleanup at the snapshot caller, and replaying it from a
    /// cache would either delay that cleanup or, worse, hand the caller a
    /// different variant than the one the backend actually produced.
    pub(super) fn is_availability_failure(&self) -> bool {
        match self {
            CurrentUserFetchError::TransientResponse(_) | CurrentUserFetchError::FetchFailed(_) => {
                true
            }
            CurrentUserFetchError::Rejected(_) => false,
            // Defensive: a replay is not a new observation, and recording it
            // would extend the window on evidence the backend never supplied.
            // `fetch_current_user_cached` returns this variant before it can
            // reach the recorder, so this arm should never actually run.
            CurrentUserFetchError::Suppressed { .. } => false,
        }
    }
}

/// The last availability failure against `auth_get_me`, keyed the same way the
/// positive cache is so that changing environment or signing in as someone else
/// bypasses it rather than inheriting someone else's outage.
#[derive(Debug, Clone)]
pub(super) struct CurrentUserFailure {
    pub(super) api_base: String,
    pub(super) token: String,
    pub(super) failed_at: Instant,
    /// Failures in an unbroken run, counted from 1. Drives the backoff width.
    pub(super) consecutive: u32,
    /// Replayed verbatim while the window is open, so a caller that matches on
    /// the variant sees what the backend really produced.
    pub(super) error: CurrentUserFetchError,
}

/// How long a run of `consecutive` failures suppresses the next live attempt.
///
/// Doubles from [`current_user_backoff_base`] and saturates at
/// [`CURRENT_USER_BACKOFF_MAX`]. `consecutive` is 1-based; 0 is treated as 1 so
/// the function has no surprising zero-length window.
pub(super) fn current_user_backoff(consecutive: u32) -> Duration {
    let steps = consecutive.saturating_sub(1).min(16);
    current_user_backoff_base()
        .saturating_mul(2u32.saturating_pow(steps))
        .min(CURRENT_USER_BACKOFF_MAX)
}

/// The recorded failure for `(api_base, token)` if its backoff window is still
/// open, in which case the caller should return it instead of going to the
/// network.
pub(super) fn suppressed_current_user_failure(
    api_base: &str,
    token: &str,
) -> Option<(CurrentUserFetchError, u32, Duration)> {
    let failure = CURRENT_USER_FAILURE.lock();
    let entry = failure.as_ref()?;
    if entry.api_base != api_base || entry.token != token {
        return None;
    }
    let window = current_user_backoff(entry.consecutive);
    let elapsed = entry.failed_at.elapsed();
    (elapsed < window).then(|| (entry.error.clone(), entry.consecutive, window - elapsed))
}

/// Record an availability failure, extending the run when it is the same
/// `(api_base, token)` and starting a new one when it is not.
///
/// A [`CurrentUserFetchError::Rejected`] is ignored — see
/// [`CurrentUserFetchError::is_availability_failure`].
pub(super) fn record_current_user_failure(
    api_base: &str,
    token: &str,
    error: CurrentUserFetchError,
) {
    if !error.is_availability_failure() {
        return;
    }
    let mut failure = CURRENT_USER_FAILURE.lock();
    record_current_user_failure_locked(&mut failure, api_base, token, error);
}

/// Forget any recorded failure, so the next poll goes straight to the network.
///
/// Called on every success and on sign-out. Missing either one is the failure
/// mode that matters here: a stale record outliving its cause keeps the app on
/// the stored snapshot after the backend has already come back.
pub(super) fn clear_current_user_failure() {
    *CURRENT_USER_FAILURE.lock() = None;
}

/// Record the timeout path's failure.
///
/// The timeout is applied by the snapshot caller, wrapping the whole of
/// `fetch_current_user_cached`, so when it fires that future is **dropped
/// mid-flight** and nothing inside it runs — including the failure recording on
/// its error path. Without this call the backoff would never engage for the one
/// case #5624 is actually about, which is timeouts rather than returned errors.
///
/// Takes the generation read before the timeout started, for the same reason the
/// refresh does: a sign-out during those `auth_fetch_timeout()` seconds means this
/// outage belongs to an identity that no longer exists, and recording it would
/// suppress the first poll of the next session.
pub(super) fn note_current_user_timeout(generation: u64, config: &Config, token: &str) {
    record_current_user_failure_unless_stale(
        generation,
        &current_user_api_base(config),
        token,
        CurrentUserFetchError::FetchFailed(format!(
            "request timed out after {}s",
            auth_fetch_timeout().as_secs()
        )),
    );
}

/// The cache key both the positive and negative current-user caches are keyed
/// on. Factored out so the snapshot caller, which records the timeout path, and
/// the fetch itself cannot drift apart on how the base URL is normalised.
pub(super) fn current_user_api_base(config: &Config) -> String {
    effective_backend_api_url(&config.api_url)
        .trim()
        .trim_end_matches('/')
        .to_string()
}

pub(super) async fn fetch_current_user_cached(
    config: &Config,
    token: &str,
    allow_cache: bool,
    generation: u64,
) -> Result<Option<Value>, CurrentUserFetchError> {
    let api_base = current_user_api_base(config);

    if allow_cache {
        if let Some((user, age)) = cached_current_user(&api_base, token) {
            if age < CURRENT_USER_REFRESH_TTL {
                debug!(
                    "{LOG_PREFIX} using cached current user age_ms={}",
                    age.as_millis()
                );
                return Ok(Some(user));
            }
            // Stale-while-revalidate. The entry has expired, but we already
            // know who this is — serve that and re-confirm it behind the poll
            // instead of making the shell wait on a WAN round trip to be told
            // the same thing. Blocking here is what put a floor of one round
            // trip under every `app_state_snapshot` (#6180: 732 calls in 3.5h,
            // not one of them under 500ms).
            //
            // The refresh cadence is unchanged by this: `refresh_current_user_now`
            // stamps `fetched_at` when the request goes out, not when it lands,
            // so the round trip is not folded into the next TTL window and the
            // poll after this one expires on the same schedule it always did.
            // Stamping at completion would have quietly halved the cadence —
            // see the note there (#6190 review). The snapshot keeps reporting
            // the true age of the data it is serving in
            // `current_user_stale_seconds`.
            //
            // Only the expired-entry path revalidates in the background. With
            // no entry at all the shell has no identity to render, so that
            // first fetch after login still blocks, below.
            spawn_current_user_refresh(config, token, generation);
            debug!(
                "{LOG_PREFIX} serving expired current user age_ms={} while refreshing behind the poll",
                age.as_millis()
            );
            return Ok(Some(user));
        }

        // Nothing fresh to serve, so this poll would normally go to the network
        // — and if the backend is unreachable it would sit there for the full
        // `auth_fetch_timeout` before the caller gives up and uses the stored
        // snapshot anyway. Replay the recorded failure instead while its window
        // is open. The caller's behaviour is unchanged (it already falls back on
        // `Err`); it just does so in microseconds. Gated on `allow_cache` for
        // the same reason the positive cache is: a pending-backend-validation
        // pass is explicitly asking for a live answer.
        if let Some((error, consecutive, remaining)) =
            suppressed_current_user_failure(&api_base, token)
        {
            debug!(
                "{LOG_PREFIX} skipping current user refresh; backend failed {consecutive}x, \
                 retrying in {}ms",
                remaining.as_millis()
            );
            // Wrapped rather than returned bare so the caller can tell a replay
            // from a live failure. Both fall back to the stored snapshot, but
            // only one of them made a request, and logging them identically is
            // what made a working backoff read as a hammering loop (#5930).
            return Err(CurrentUserFetchError::Suppressed {
                inner: Box::new(error),
                consecutive,
                retry_in: remaining,
            });
        }
    }

    refresh_current_user_now(config, token, generation, RefreshOrigin::Blocking).await
}

/// The cached user for this identity and how old it is, if the cache holds one.
///
/// Reads under one lock and hands back an owned copy, so no caller holds
/// `CURRENT_USER_CACHE` across a decision — the freshness test and the
/// stale-while-revalidate branch below both need the same read.
pub(super) fn cached_current_user(api_base: &str, token: &str) -> Option<(Value, Duration)> {
    let cache = CURRENT_USER_CACHE.lock();
    let entry = cache.as_ref()?;
    (entry.api_base == api_base && entry.token == token)
        .then(|| (entry.user.clone(), entry.fetched_at.elapsed()))
}

/// Single-flight gate for the background refresh.
///
/// An async mutex held by the spawned task for its lifetime, taken with
/// `try_lock` so a poll that finds a refresh already running simply declines to
/// start a second one rather than queueing behind it. Same gate, same reason as
/// [`RUNTIME_SNAPSHOT_REBUILD`]: without it every overlapping poll launches its
/// own fetch. Using a guard rather than a flag means a panicking refresh
/// releases the gate instead of wedging it shut for the life of the process.
pub(super) static CURRENT_USER_REFRESH_INFLIGHT: Lazy<tokio::sync::Mutex<()>> =
    Lazy::new(|| tokio::sync::Mutex::new(()));

/// Refresh the cached current user without making the caller wait for it.
///
/// Declines to start when the backoff window from an earlier failure is still
/// open — a background refresh would re-pay exactly the timeout that window
/// exists to avoid (#5624) — and when a previous poll's refresh is still in
/// flight.
pub(super) fn spawn_current_user_refresh(config: &Config, token: &str, generation: u64) {
    if let Some((error, consecutive, remaining)) =
        suppressed_current_user_failure(&current_user_api_base(config), token)
    {
        // Logged here because the stale-while-revalidate return above means an
        // outage no longer reaches the replay branch in
        // `fetch_current_user_cached` — without this line a backend that has
        // been down for minutes leaves no trace at all in the snapshot logs.
        // `debug!`, not `warn!`: no request was made and nothing waited on it
        // (#5930).
        debug!(
            "{LOG_PREFIX} not refreshing current user behind the poll; backend failed \
             {consecutive}x, retrying in {}ms: {}",
            remaining.as_millis(),
            error.message()
        );
        return;
    }
    let gate: &'static tokio::sync::Mutex<()> = &CURRENT_USER_REFRESH_INFLIGHT;
    let Ok(guard) = gate.try_lock() else {
        return;
    };

    let config = config.clone();
    let token = token.to_string();
    tokio::spawn(async move {
        let _guard = guard;
        // Bounded by the same budget the blocking path spends, so a hung
        // backend cannot leave the gate closed for longer than one window.
        match tokio::time::timeout(
            auth_fetch_timeout(),
            refresh_current_user_now(&config, &token, generation, RefreshOrigin::Background),
        )
        .await
        {
            Ok(Ok(_)) => {}
            // The stale entry stands and the failure is recorded, so the next
            // poll takes the backoff path. Not a failure of any user-visible
            // operation — nothing was waiting on this.
            Ok(Err(error)) => debug!(
                "{LOG_PREFIX} background current user refresh failed; serving stale entry: {}",
                error.message()
            ),
            Err(_) => {
                debug!(
                    "{LOG_PREFIX} background current user refresh timed out after {}s; serving stale entry",
                    auth_fetch_timeout().as_secs()
                );
                // The generation is captured from when the refresh was launched and
                // guards the timeout record against sign-out racing the timeout.
                note_current_user_timeout(generation, &config, &token);
            }
        }
    });
}

/// Where a refresh was started from, which decides whether its answer may still
/// be committed by the time it lands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum RefreshOrigin {
    /// The caller is still awaiting this refresh and still holds the identity
    /// it asked for, so the answer is authoritative by construction.
    Blocking,
    /// Detached from the poll that started it, and therefore able to outlive
    /// the identity it was started for — a logout and re-login, or an
    /// environment switch, can land in between.
    Background,
}

/// Go to the backend, then reconcile the caches and freshness stamps with what
/// came back. The blocking half of [`fetch_current_user_cached`], split out so
/// the background refresh runs exactly the same path rather than a parallel
/// copy of it that could drift.
pub(super) async fn refresh_current_user_now(
    config: &Config,
    token: &str,
    generation: u64,
    origin: RefreshOrigin,
) -> Result<Option<Value>, CurrentUserFetchError> {
    let api_base = current_user_api_base(config);
    // The TTL clock starts when the request goes out, not when it lands.
    //
    // `fetched_at` is what `CURRENT_USER_REFRESH_TTL` is measured against, and
    // the poll loop schedules itself from the previous *response*. Stamping at
    // completion would therefore fold the round trip into the next window: a
    // refresh taking `L` leaves the following poll only `TTL - L` from expiry,
    // it reads the entry as fresh, and the refresh after that is skipped
    // entirely — halving the cadence as a side effect of not blocking (#6190
    // review). Stamping at initiation keeps the wall-clock refresh cadence
    // exactly what it was before this path became non-blocking; the only thing
    // that changed is who waits for it.
    //
    // It also errs the safe way. The data itself arrives at `started_at + L`,
    // so calling it `started_at` slightly *overstates* its age and expires the
    // entry sooner — never later. `note_current_user_success` below is the
    // stamp that answers "how old is the data we are showing", and it stays at
    // completion, because that is when the data actually arrived.
    let started_at = Instant::now();
    let fetched = match fetch_current_user(config, token).await {
        Ok(user) => sanitize_snapshot_user(user),
        Err(error) => {
            if !record_current_user_failure_unless_stale(
                generation,
                &api_base,
                token,
                error.clone(),
            ) {
                debug!("{LOG_PREFIX} discarding current user failure that raced sign-out");
            }
            return Err(error);
        }
    };
    // A detached refresh can land after the app has moved to another identity,
    // and every write below is process-global. Committing then would regress
    // the cache to the previous user — and `peek_cached_current_user_identity`
    // reads that slot WITHOUT a key check (#926), so the regressed entry would
    // be embedded in the agent's prompts as the current user. The keyed reads
    // in `fetch_current_user_cached` would merely miss; that one would be
    // wrong.
    //
    // The check and the commit share ONE lock acquisition, and nothing between
    // them can suspend or release it. Validating through a separate read would
    // leave a window in which a blocking refresh for a newer identity commits
    // after this one has already decided it is current — narrow, but the
    // runtime is multi-threaded, so "narrow" is not "impossible".
    //
    // The generation check (against sign-out) and the identity check (against
    // a subsequent login the background refresh missed) are applied under the
    // same lock. Sign-out bumps the generation before clearing the caches, so
    // a refresh that read the old token before sign-out will read the old
    // generation and fail the check — the cache it would restore was already
    // cleared.
    //
    // The failure and freshness stamps stay OUTSIDE this scope: they take their
    // own locks, and this module's rule is that `LAST_CURRENT_USER_SUCCESS` is
    // never nested inside `CURRENT_USER_CACHE`. They therefore run *after* the
    // commit rather than before it, which is also what lets a discarded refresh
    // leave the newer identity's `CURRENT_USER_FAILURE` record alone —
    // `clear_current_user_failure` is unkeyed and would otherwise wipe it. The
    // stamp is only ever read as an age in seconds, so moving it to the far
    // side of the commit cannot change an observable answer.
    //
    // When sign-out wins the generation race, the failure and success records
    // are untouched — they were already cleared by `forget_current_user_caches`,
    // and this refresh has no business touching them either.
    let committed = {
        let mut cache = CURRENT_USER_CACHE.lock();
        let generation_mismatch = current_user_generation() != generation;
        if generation_mismatch {
            debug!(
                "{LOG_PREFIX} discarding current user refresh that raced sign-out; \
                 generation bumped while in flight"
            );
            false
        } else {
            let moved_on = cache
                .as_ref()
                .is_some_and(|entry| entry.api_base != api_base || entry.token != token);
            if origin == RefreshOrigin::Background && moved_on {
                false
            } else {
                match fetched.clone() {
                    Some(user) => {
                        debug!("{LOG_PREFIX} refreshed current user from backend");
                        *cache = Some(CachedCurrentUser {
                            api_base: api_base.clone(),
                            token: token.to_string(),
                            fetched_at: started_at,
                            user,
                        });
                    }
                    None => {
                        debug!("{LOG_PREFIX} backend returned empty current user; clearing cache");
                        *cache = None;
                    }
                }
                true
            }
        }
    };

    if !committed {
        debug!(
            "{LOG_PREFIX} discarding background current user refresh; the cache moved to \
             another identity while it was in flight"
        );
        return Ok(fetched);
    }

    // Keep all post-fetch records behind the same generation checks as the
    // positive cache. A logout (or a subsequent login) can land after the
    // cache commit and must not have its failure/success records overwritten.
    clear_current_user_failure_unless_stale(generation);
    if fetched.is_some() {
        note_current_user_success_unless_stale(generation, &api_base, token);
    }

    Ok(fetched)
}

/// Synchronous, network-free peek at the cached `auth_get_me` response,
/// returning only the identifying fields the prompt layer is allowed to
/// embed (`id`, `name`, `email`). Tokens stay locked behind the JWT
/// helpers — never returned through this path. See issue #926.
///
/// Returns `None` when no `auth_get_me` call has populated the cache
/// yet (CLI-only flows, fresh installs, signed-out sessions). The
/// cache TTL is **ignored** here intentionally — for prompt rendering
/// a slightly stale identity is fine; the freshness check only
/// matters for the snapshot RPC that fronts the React shell.
pub fn peek_cached_current_user_identity() -> Option<crate::agent::prompts::UserIdentity> {
    let cache = CURRENT_USER_CACHE.lock();
    let entry = cache.as_ref()?;
    let user = entry.user.as_object()?;

    let pluck = |key: &str| -> Option<String> {
        user.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let id = pluck("id")
        .or_else(|| pluck("user_id"))
        .or_else(|| pluck("userId"));
    let name = pluck("name")
        .or_else(|| pluck("displayName"))
        .or_else(|| pluck("display_name"))
        .or_else(|| pluck("full_name"))
        .or_else(|| pluck("fullName"));
    let email = pluck("email");

    let identity = crate::agent::prompts::UserIdentity { id, name, email };
    if identity.is_empty() {
        None
    } else {
        Some(identity)
    }
}
