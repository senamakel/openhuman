//! In-memory [`AgentDefinitionRegistry`] and its process-wide singleton.

use super::agent_definition::AgentDefinition;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

/// Whether two definitions serialise identically, which covers every
/// authored field (the process-local `source` and `graph` are not serialised).
fn same_definition(a: &AgentDefinition, b: &AgentDefinition) -> bool {
    match (serde_json::to_value(a), serde_json::to_value(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// In-memory registry of all known [`AgentDefinition`]s.
///
/// One singleton instance is initialised at startup via
/// [`AgentDefinitionRegistry::init_global`]. Built-ins are registered
/// unconditionally; custom TOML definitions (if a workspace is provided)
/// are loaded next and override built-ins on `id` collision.
#[derive(Debug, Default)]
pub struct AgentDefinitionRegistry {
    by_id: HashMap<String, AgentDefinition>,
    /// Insertion-stable order for predictable `list()` output.
    order: Vec<String>,
}

static GLOBAL: OnceLock<Arc<AgentDefinitionRegistry>> = OnceLock::new();

impl AgentDefinitionRegistry {
    /// Build a registry containing only the built-in definitions
    /// (no TOML loading). Useful for tests.
    pub fn builtins_only() -> Self {
        let mut reg = Self::default();
        for def in super::super::builtin_definitions::all() {
            reg.insert(def);
        }
        reg
    }

    /// Build a registry containing built-ins plus any custom TOML
    /// definitions found under `<workspace>/agents/*.toml` (and the
    /// `~/.openhuman/agents/*.toml` fallback). Custom definitions
    /// override built-ins on `id` collision. Files that fail to parse
    /// are logged and skipped rather than aborting startup.
    pub fn load(workspace: &Path) -> Result<Self> {
        let mut reg = Self::builtins_only();
        let custom = super::super::definition_loader::load_from_workspace(workspace)?;
        for def in custom {
            tracing::info!(
                id = %def.id,
                source = ?def.source,
                "[agent_defs] loaded custom definition (overrides any built-in with the same id)"
            );
            reg.insert(def);
        }

        // Re-validate the tier hierarchy after custom overrides are
        // merged in — a workspace TOML can legally replace a built-in
        // (same id) and is held to the same spawn-hierarchy contract
        // as the bundled set. See
        // [`crate::agent::registry::agents::loader::validate_tier_hierarchy`].
        let snapshot: Vec<AgentDefinition> = reg.list().into_iter().cloned().collect();
        crate::agent::registry::agents::validate_tier_hierarchy(&snapshot).map_err(|e| {
            anyhow::anyhow!(
                "agent registry rejected after merging workspace overrides from {}: {}",
                workspace.display(),
                e
            )
        })?;

        Ok(reg)
    }

    /// Convenience: resolve the default workspace via
    /// [`crate::config::Config::load_or_init`] and load from
    /// it. Built for sync CLI call sites (`openhuman agent list`,
    /// future inspection tools) so they don't re-implement the Config
    /// → workspace resolution dance. Must NOT be called from an
    /// existing tokio runtime — construct a runtime and `block_on`.
    pub async fn load_for_default_workspace() -> Result<Self> {
        let config = crate::config::Config::load_or_init().await?;
        Self::load(&config.workspace_dir)
    }

    /// Insert (or replace) a definition by id.
    pub fn insert(&mut self, def: AgentDefinition) {
        let id = def.id.clone();
        if self.by_id.insert(id.clone(), def).is_none() {
            self.order.push(id);
        }
    }

    /// Look up a definition by id.
    pub fn get(&self, id: &str) -> Option<&AgentDefinition> {
        self.by_id.get(id)
    }

    /// All definitions, in insertion order.
    pub fn list(&self) -> Vec<&AgentDefinition> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .collect()
    }

    /// Number of registered definitions.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// True when the registry has no definitions.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Whether this registry is exactly the built-in set: the same ids
    /// and contents as [`Self::builtins_only`], and no definition loaded from
    /// a file or synthesized from a user entry. SaaS boot refuses anything else.
    pub fn holds_builtins_only(&self) -> bool {
        let builtins = Self::builtins_only();
        self.len() == builtins.len()
            && self.list().iter().all(|def| {
                def.source == super::DefinitionSource::Builtin
                    && builtins
                        .get(&def.id)
                        .is_some_and(|builtin| same_definition(def, builtin))
            })
    }

    // ── singleton API ──────────────────────────────────────────────────

    /// Initialise the global registry. Subsequent calls are no-ops (the
    /// `OnceLock` only fires once); use [`Self::reload_global`] to refresh
    /// custom definitions during development.
    pub fn init_global(workspace: &Path) -> Result<()> {
        let registry = Self::load(workspace)?;
        match GLOBAL.set(Arc::new(registry)) {
            Ok(()) => {
                tracing::info!(
                    "[agent_defs] global registry initialised with {} definitions",
                    GLOBAL.get().map(|r| r.len()).unwrap_or(0)
                );
                Ok(())
            }
            Err(_) => {
                tracing::debug!("[agent_defs] global registry already initialised; ignoring");
                Ok(())
            }
        }
    }

    /// Initialise the global registry with builtins only (no workspace
    /// scan). Used by tests and by callers that don't have a workspace.
    pub fn init_global_builtins() -> Result<()> {
        let registry = Self::builtins_only();
        let _ = GLOBAL.set(Arc::new(registry));
        Ok(())
    }

    /// Borrow the global registry, if initialised.
    pub fn global() -> Option<&'static Self> {
        GLOBAL.get().map(Arc::as_ref)
    }

    /// Clones the durable process registry for a host capability bundle.
    ///
    /// Hosted invocations need owned authority because their capability bundle
    /// outlives the factory stack frame. This is the owned counterpart of
    /// [`Self::global`], not a rebuilt or global lookup performed mid-turn.
    pub fn global_arc() -> Option<Arc<Self>> {
        GLOBAL.get().cloned()
    }

    /// The catalogue sub-agent lookups resolve through: the ambient agent
    /// context's own registry when it carries one, else the process registry.
    pub fn current() -> Option<Arc<Self>> {
        crate::core::runtime::CoreContext::current()
            .and_then(|ctx| ctx.definitions())
            .or_else(Self::global_arc)
    }

    /// A copy of this registry with `definitions` added, replacing any
    /// definition with the same id.
    pub fn with_definitions(&self, definitions: impl IntoIterator<Item = AgentDefinition>) -> Self {
        let mut registry = Self::default();
        for definition in self.list() {
            registry.insert(definition.clone());
        }
        for definition in definitions {
            registry.insert(definition);
        }
        registry
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
