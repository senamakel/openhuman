//! A small base for domain stores on the document port.
//!
//! Most core stores have a synchronous API and a fixed set of collections.
//! [`Repo`] holds one scoped document handle, declares the domain's
//! collections before each call, and runs the call on the shared blocking
//! bridge ([`super::block_on`]). [`compare_and_swap`] is the read, change,
//! conditional-write loop every guarded SQL `UPDATE … WHERE` becomes, so two
//! processes on one database never both apply a change that requires a
//! particular prior state.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, LazyLock, Mutex, PoisonError, Weak};

use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tinystoragedrivers::{CollectionSpec, ErrorKind, Versioned};

use super::{
    block_on, current_scope, installed, DocumentStore, ScopedStorage, StorageBackend, StorageError,
};

/// Compare-and-swap attempts before a contended update gives up.
pub const CAS_ATTEMPTS: usize = 32;

/// Collections already declared, per process: `(backend address, scope,
/// collection)`. The `Weak` guards against an address reused by a later
/// backend, so a replaced backend is declared afresh.
type DeclaredKey = (usize, String, String);

static DECLARED: LazyLock<Mutex<HashMap<DeclaredKey, Weak<dyn StorageBackend>>>> =
    LazyLock::new(Mutex::default);

fn backend_addr(backend: &Arc<dyn StorageBackend>) -> usize {
    Arc::as_ptr(backend).cast::<()>() as usize
}

/// Whether `collection` was already declared on `backend` under `scope`.
fn is_declared(backend: &Arc<dyn StorageBackend>, scope: &str, collection: &str) -> bool {
    let key = (
        backend_addr(backend),
        scope.to_string(),
        collection.to_string(),
    );
    DECLARED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
        .and_then(Weak::upgrade)
        .is_some_and(|alive| Arc::ptr_eq(&alive, backend))
}

/// Records that `collection` is declared on `backend` under `scope`, and
/// forgets entries whose backend is gone.
fn mark_declared(backend: &Arc<dyn StorageBackend>, scope: &str, collection: &str) {
    let key = (
        backend_addr(backend),
        scope.to_string(),
        collection.to_string(),
    );
    let mut declared = DECLARED.lock().unwrap_or_else(PoisonError::into_inner);
    declared.retain(|_, weak| weak.strong_count() > 0);
    declared.insert(key, Arc::downgrade(backend));
}

/// The backend and scope a [`Repo`] was opened on, for declaring its
/// collections once per process instead of on every call.
#[derive(Clone)]
struct Origin {
    backend: Arc<dyn StorageBackend>,
    scope: String,
}

/// One domain's document store under one storage scope.
#[derive(Clone)]
pub struct Repo {
    docs: Arc<dyn DocumentStore>,
    domain: &'static str,
    collections: fn() -> Vec<CollectionSpec>,
    origin: Option<Origin>,
}

impl Repo {
    /// The repo for this call when the host configured a backend, scoped to
    /// the acting agent; `None` keeps the domain on its classic store.
    ///
    /// # Errors
    ///
    /// When the scope cannot be resolved — in SaaS mode with no acting agent
    /// — so the call fails instead of reading a shared bucket.
    pub fn current(
        domain: &'static str,
        collections: fn() -> Vec<CollectionSpec>,
    ) -> Result<Option<Self>> {
        let Some(backend) = installed() else {
            return Ok(None);
        };
        let scope =
            current_scope().with_context(|| format!("[{domain}] resolve the storage scope"))?;
        let scoped = backend
            .for_scope(&scope)
            .with_context(|| format!("[{domain}] open the storage scope"))?;
        let mut repo = Self::over(&scoped, domain, collections);
        repo.origin = Some(Origin {
            backend,
            scope: scope.as_str().to_string(),
        });
        Ok(Some(repo))
    }

    /// The repo over an already scoped handle (tests, explicit scopes).
    pub fn over(
        scoped: &ScopedStorage,
        domain: &'static str,
        collections: fn() -> Vec<CollectionSpec>,
    ) -> Self {
        Self {
            docs: Arc::clone(scoped.documents()),
            domain,
            collections,
            origin: None,
        }
    }

    /// Runs `op` against the store from synchronous code, after declaring the
    /// domain's collections (idempotent).
    ///
    /// # Errors
    ///
    /// Any storage error, prefixed with the domain name.
    pub fn run<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: FnOnce(Arc<dyn DocumentStore>) -> Fut,
        Fut: Future<Output = Result<T, StorageError>> + Send + 'static,
        T: Send + 'static,
    {
        let docs = Arc::clone(&self.docs);
        let origin = self.origin.clone();
        let specs: Vec<CollectionSpec> = (self.collections)()
            .into_iter()
            .filter(|spec| {
                origin
                    .as_ref()
                    .is_none_or(|origin| !is_declared(&origin.backend, &origin.scope, &spec.name))
            })
            .collect();
        let future = op(Arc::clone(&docs));
        let domain = self.domain;
        block_on(async move {
            for spec in &specs {
                docs.ensure_collection(spec).await?;
                if let Some(origin) = &origin {
                    mark_declared(&origin.backend, &origin.scope, &spec.name);
                }
            }
            future.await
        })
        .map_err(|error| anyhow!("[{domain}] storage: {error}"))
    }
}

/// Applies `change` to document `id` in `collection` under compare-and-swap
/// and returns what was stored, or `None` when the document is missing or
/// `change` declines (it is no longer in the state the change requires).
///
/// # Errors
///
/// A storage error, or a conflict when the document kept changing for
/// [`CAS_ATTEMPTS`] attempts.
pub async fn compare_and_swap(
    docs: &Arc<dyn DocumentStore>,
    collection: &str,
    id: &str,
    change: impl Fn(&Value) -> Option<Value>,
) -> Result<Option<Versioned<Value>>, StorageError> {
    for _ in 0..CAS_ATTEMPTS {
        let Some(stored) = docs.get(collection, id).await? else {
            return Ok(None);
        };
        let Some(next) = change(&stored.doc) else {
            return Ok(None);
        };
        match docs
            .put(collection, id, next.clone(), stored.unchanged())
            .await
        {
            Ok(version) => {
                return Ok(Some(Versioned {
                    id: id.to_string(),
                    version,
                    doc: next,
                }));
            }
            Err(error) if error.kind() == ErrorKind::Conflict => {}
            Err(error) => return Err(error),
        }
    }
    Err(StorageError::conflict(format!(
        "{collection}/{id} kept changing under {CAS_ATTEMPTS} attempts"
    )))
}

/// The string field `field` of `doc`, when present.
pub fn text<'a>(doc: &'a Value, field: &str) -> Option<&'a str> {
    doc.get(field).and_then(Value::as_str)
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
