//! Client seam for the TinyMemory driver when it is loaded as a `TinyBus`
//! module rather than compiled in.
//!
//! TinyMemory ships as a `cdylib` exporting one object,
//! [`OBJECT_PATH`](tinymemory_bus::names::OBJECT_PATH), with 89 members on it.
//! The host loads that binary and calls into it; it cannot `use` anything out
//! of it, so the vocabulary — the member names, the payload types, and the
//! error-name table — arrives from the `tinymemory-bus` library instead.
//!
//! This module is the piece in between: it turns a member name plus typed
//! arguments into a `TinyBus` call, and turns the reply — or the failure — back
//! into something the memory layer can act on.
//!
//! # Why the error mapping is the interesting part
//!
//! A `TinyBus` failure is a name and a prose message. The name is the contract:
//! [`tinymemory_bus::wire`] holds the table mapping it back to a
//! [`MemoryError`], and the *module uses the same table in the other
//! direction*. That is what keeps the two ends from drifting into disagreeing
//! about what a name means — the case that matters being `PathEscape`, which
//! reports a sandbox escape and must not be silently reclassified as a
//! caller's malformed argument.
//!
//! Everything that is not a `MethodFailed` never reached the driver at all: a
//! timeout, an unowned name, a transport fault. Those are mapped to the
//! [`MemoryError`] variants that say so rather than being flattened into
//! `Other`, because a host retries an [`Unreachable`](MemoryError::Unreachable)
//! and does not retry an [`Invalid`](MemoryError::Invalid).
//!
//! # Scope
//!
//! [`MemoryModule::call`] is public and reaches **every** member: pass a name
//! from [`tinymemory_bus::names::methods`] and the positional arguments as a
//! tuple. The typed wrappers below cover the driver-level members and the
//! mandatory core family, which is what a host needs to bind a driver and
//! prove it answers. The remaining families are one `call` each and are added
//! as the driver seam grows into them.

use serde::de::DeserializeOwned;
use serde::Serialize;
use tinybus::{Connection, Error as BusError, Proxy};
use tinymemory_bus::error::MemoryError;
use tinymemory_bus::names::{methods, BUS_NAME, OBJECT_PATH};
use tinymemory_bus::types::{MemoryCategory, MemoryEntry, MemoryTaint, NamespaceSummary};
use tinymemory_bus::{capabilities::Capabilities, health::MemoryHealth, wire};

/// A bound TinyMemory module object.
///
/// Addresses one store. The root object is the one at
/// [`OBJECT_PATH`](tinymemory_bus::names::OBJECT_PATH); `OpenStore` answers
/// with the path of a *sibling* store under the same workspace, exporting this
/// identical interface, which [`MemoryModule::at`] binds.
#[derive(Debug, Clone)]
pub struct MemoryModule {
    proxy: Proxy,
}

impl MemoryModule {
    /// Bind the module's root object on `connection`.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] if the well-known name or object path is
    /// rejected by `TinyBus` — unreachable in practice, since both are
    /// constants from the contract, but the constructor is fallible rather than
    /// panicking on a value it did not choose.
    pub fn new(connection: &Connection) -> Result<Self, MemoryError> {
        Self::at(connection, OBJECT_PATH)
    }

    /// Bind a specific object path on `connection` — an `OpenStore` result.
    ///
    /// # Errors
    ///
    /// [`MemoryError::Invalid`] if `object_path` is not a valid `TinyBus`
    /// object path. Unlike [`new`](Self::new) this is reachable: the path comes
    /// back over the wire.
    pub fn at(connection: &Connection, object_path: &str) -> Result<Self, MemoryError> {
        let proxy = connection
            .proxy(BUS_NAME, object_path, BUS_NAME)
            .map_err(|error| MemoryError::Invalid(error.to_string()))?;
        Ok(Self { proxy })
    }

    /// Call any member by name, with `args` as its positional arguments.
    ///
    /// Pass a tuple for several arguments, a bare value for one, and `()` for
    /// none — the encoding `#[tinybus::interface]` decodes on the far side.
    /// Names come from [`tinymemory_bus::names::methods`]; spelling one by hand
    /// is what that module exists to avoid.
    ///
    /// # Errors
    ///
    /// The driver's own [`MemoryError`], recovered through
    /// [`tinymemory_bus::wire::from_wire`], when the call reached the module and
    /// failed there. Otherwise the transport failure, mapped by
    /// [`map_bus_error`].
    pub async fn call<R: DeserializeOwned>(
        &self,
        member: &str,
        args: impl Serialize + Send,
    ) -> Result<R, MemoryError> {
        self.proxy.call(member, args).await.map_err(map_bus_error)
    }

    /// The driver id this module reports.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn driver_id(&self) -> Result<String, MemoryError> {
        self.call(methods::DRIVER_ID, ()).await
    }

    /// The capability families this driver advertises.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn capabilities(&self) -> Result<Capabilities, MemoryError> {
        self.call(methods::CAPABILITIES, ()).await
    }

    /// The driver's liveness.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn health(&self) -> Result<MemoryHealth, MemoryError> {
        self.call(methods::HEALTH, ()).await
    }

    /// Ask the module to shut its store down.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn shutdown(&self) -> Result<(), MemoryError> {
        self.call(methods::SHUTDOWN, ()).await
    }

    /// Open a sibling store under `memory_subdir`, returning its object path.
    ///
    /// Bind the result with [`at`](Self::at). Asking twice for the same subdir
    /// returns the same path rather than opening the database twice.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn open_store(&self, memory_subdir: &str) -> Result<String, MemoryError> {
        self.call(methods::OPEN_STORE, (memory_subdir,)).await
    }

    /// Upsert the entry at `(namespace, key)`.
    ///
    /// `taint` is required rather than defaulted, mirroring the contract: a
    /// caller that could omit provenance would be able to launder
    /// externally-sourced content into internal-trust content.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn store(
        &self,
        namespace: &str,
        key: &str,
        content: &str,
        category: MemoryCategory,
        session_id: Option<&str>,
        taint: MemoryTaint,
    ) -> Result<(), MemoryError> {
        self.call(
            methods::STORE,
            (namespace, key, content, category, session_id, taint),
        )
        .await
    }

    /// Fetch the entry at an exact `(namespace, key)`.
    ///
    /// A miss is `Ok(None)`, not an error — the contract's rule, preserved
    /// across the wire.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn get(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<MemoryEntry>, MemoryError> {
        self.call(methods::GET, (namespace, key)).await
    }

    /// Delete the entry at `(namespace, key)`, reporting whether it existed.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn forget(&self, namespace: &str, key: &str) -> Result<bool, MemoryError> {
        self.call(methods::FORGET, (namespace, key)).await
    }

    /// List entries, narrowing by namespace, category and session.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call) — and note that this member has no limit and no
    /// cursor, so the module refuses an oversized response with
    /// [`MemoryError::BudgetExceeded`] rather than truncating it. A short list
    /// would be indistinguishable from a complete one.
    pub async fn list(
        &self,
        namespace: Option<&str>,
        category: Option<MemoryCategory>,
        session_id: Option<&str>,
    ) -> Result<Vec<MemoryEntry>, MemoryError> {
        self.call(methods::LIST, (namespace, category, session_id))
            .await
    }

    /// Enumerate namespaces with their aggregate counts.
    ///
    /// # Errors
    ///
    /// See [`call`](Self::call).
    pub async fn namespaces(&self) -> Result<Vec<NamespaceSummary>, MemoryError> {
        self.call(methods::NAMESPACES, ()).await
    }
}

/// Turn a `TinyBus` failure into the memory error it stands for.
///
/// [`BusError::MethodFailed`] is the one that reached the driver: its name is
/// the contract, and [`wire::from_wire`] is the same table the module mapped
/// *out* through, so the variant survives the round trip. A name this build
/// does not recognise becomes [`MemoryError::Other`] and never
/// [`MemoryError::Invalid`] — a module newer than the host may name an error
/// this table has no variant for, and telling a caller its input was wrong when
/// it was not sends it into a rewrite loop over something already correct.
///
/// Every other variant never reached the driver, so it is a transport fact and
/// is reported as one.
pub(crate) fn map_bus_error(error: BusError) -> MemoryError {
    match error {
        BusError::MethodFailed { name, message } => wire::from_wire(&name, &message),
        BusError::Timeout { .. } => MemoryError::Timeout(error.to_string()),
        // The module is not running, or has not claimed its name yet. Both are
        // "try again once it is up", which is what `Unreachable` tells a caller.
        BusError::NameHasNoOwner(_) | BusError::Transport(_) | BusError::Io(_) => {
            MemoryError::Unreachable(error.to_string())
        }
        // A member or object this build believes in and the module does not.
        // That is a contract mismatch, not a caller mistake, so it is not
        // `Invalid`: the argument was fine, the peer is the wrong version.
        BusError::UnknownMethod { .. }
        | BusError::UnknownObject { .. }
        | BusError::UnknownInterface { .. }
        | BusError::IncompatibleVersion { .. } => MemoryError::Backend(error.to_string()),
        other => MemoryError::Other(anyhow::anyhow!(other.to_string())),
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
