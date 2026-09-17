//! Tool groups as a **library-level axis**, alongside `ServiceSet` and
//! `DomainSet`.
//!
//! The packs in [`super::registry`] exist for one host's problem: an
//! orchestrator whose fixed per-turn cost is dominated by tool schemas. That is
//! a compression decision, and compiling it in is right for the desktop app —
//! membership must not be editable by config or RPC, or a caller could move a
//! dangerous tool out of the reviewed surface.
//!
//! An embedder is a different question. `openhuman_core` is consumed as a
//! library through [`Harness`](crate::Harness), and there the pack table is not
//! a compression choice but a *capability* one: a host embedding the harness to
//! summarise documents has no use for the crypto belt at any disclosure level,
//! and a host driving its own routing may want every schema on the wire because
//! it does not pay the orchestrator's budget. Neither is expressible by
//! membership alone, which only ever answers "advertised or withheld".
//!
//! So the group id — the same string the model names in `use_skill` — becomes
//! the unit an embedder selects on, with three states rather than two:
//!
//! | [`GroupMode`] | Schemas on the wire | Registered and callable |
//! | --- | --- | --- |
//! | `Advertised` | yes | yes |
//! | `Withheld` | no (reached via `use_skill`) | yes |
//! | `Off` | no | **no** |
//!
//! `Off` is the state that could not be said before, and it is the one an
//! embedder reaches for most: absence beats a registered tool that fails, for
//! the reason the `flows` compile gate already documents — a tool the model can
//! see teaches it the capability exists and makes it retry.
//!
//! **The default is fail-closed, which is not the same as inert.**
//! [`ToolGroups::default`] puts every pack in `Withheld`. That was identical to
//! the compiled-in table when this type was introduced, and it stops being
//! identical every time a family moves into a pack: a host that never calls
//! [`CoreBuilder::tool_groups`](crate::core::runtime::CoreBuilder::tool_groups)
//! loses that family silently on its next bump. An embedder that packs nothing
//! on purpose says so with [`set_process_default`], which needs no
//! `CoreContext` and so is reachable from the synchronous paths that actually
//! read this — a roster build, an agent build, a host's own test fixtures.
//!
//! **This axis does not widen what a build contains.** A group whose tools are
//! compiled out (`--no-default-features`) or whose `DomainGroup` is off under
//! the ambient `DomainSet` stays absent no matter what mode is set here;
//! `Advertised` cannot conjure a tool that was never registered. The three
//! filters compose one way only — narrowing.

use super::registry::PACKS;

/// How one tool group reaches the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupMode {
    /// Schemas are on the wire on every provider call.
    Advertised,
    /// Registered and executable, but reached only through `use_skill`.
    /// The compiled-in default for every pack.
    #[default]
    Withheld,
    /// Not registered at all — the tools do not exist for this core.
    Off,
}

/// The number of compiled-in groups. `const` so [`ToolGroups`] can be a fixed
/// array and carry no allocation.
pub const GROUP_COUNT: usize = PACKS.len();

/// Per-group disclosure for one core.
///
/// Indexed positionally against [`PACKS`]; ids are resolved through
/// [`ToolGroups::index_of`] so a caller never depends on pack order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolGroups {
    modes: [GroupMode; GROUP_COUNT],
}

impl Default for ToolGroups {
    /// Every group withheld.
    ///
    /// This was "byte-identical to the behaviour before this type existed" when
    /// written, and that held only while the packs were empty of anything a
    /// host actually used. It is no longer true: every family moved into a pack
    /// since is one this default withholds from a host that never asked for
    /// packing. It is a deliberate fail-closed floor, not a no-op — a host that
    /// wants the other posture says so via
    /// [`set_process_default`](super::set_process_default) or a `CoreContext`.
    fn default() -> Self {
        Self {
            modes: [GroupMode::Withheld; GROUP_COUNT],
        }
    }
}

impl ToolGroups {
    /// Every group withheld. The desktop app's shape, and the default.
    pub fn packed() -> Self {
        Self::default()
    }

    /// Every group's schemas on the wire.
    ///
    /// For a host that does not pay the orchestrator's per-turn schema budget —
    /// a short-lived harness run, or an embedder doing its own routing — and
    /// wants native function calling rather than the `use_skill` envelope.
    pub fn advertised() -> Self {
        Self {
            modes: [GroupMode::Advertised; GROUP_COUNT],
        }
    }

    /// No group's tools registered at all: the baseline belt only.
    ///
    /// The starting point for a host that opts individual groups back in, the
    /// way `DomainSet::kernel()` is for domains.
    pub fn none() -> Self {
        Self {
            modes: [GroupMode::Off; GROUP_COUNT],
        }
    }

    /// Set one group's mode. Unknown ids are ignored — a group id is data, and
    /// a build that compiled a family out should not panic a host that still
    /// names it.
    pub fn with(mut self, id: &str, mode: GroupMode) -> Self {
        if let Some(i) = Self::index_of(id) {
            self.modes[i] = mode;
        } else {
            log::warn!(
                "[toolgroups] ignoring unknown group id `{id}` (not compiled into this build)"
            );
        }
        self
    }

    /// The mode for `id`. An unknown id reports [`GroupMode::Advertised`],
    /// because a tool that belongs to no compiled-in group is never withheld.
    pub fn mode(&self, id: &str) -> GroupMode {
        Self::index_of(id)
            .map(|i| self.modes[i])
            .unwrap_or(GroupMode::Advertised)
    }

    /// The mode owning `tool`, or [`GroupMode::Advertised`] when no group does.
    pub fn mode_for_tool(&self, tool: &str) -> GroupMode {
        match super::registry::pack_for_tool(tool) {
            Some(pack) => self.mode(pack.id),
            None => GroupMode::Advertised,
        }
    }

    /// Every compiled-in group id, in table order.
    pub fn ids() -> impl Iterator<Item = &'static str> {
        PACKS.iter().map(|p| p.id)
    }

    fn index_of(id: &str) -> Option<usize> {
        PACKS.iter().position(|p| p.id == id)
    }
}

/// The process-wide posture for a host that establishes no [`CoreContext`].
///
/// Set by [`set_process_default`]; consulted by [`current`] only when there is
/// no context to read.
///
/// [`CoreContext`]: crate::core::runtime::context::CoreContext
static PROCESS_GROUPS: std::sync::OnceLock<ToolGroups> = std::sync::OnceLock::new();

/// Declare the process-wide groups without standing up a [`CoreContext`].
///
/// For an **embedder that does its own tool routing**: a host which registers
/// its own tools and gates them itself gains nothing from pack withholding and
/// pays the capability for it. That is the audience
/// [`ToolGroups::advertised`] already names, and until this existed it was the
/// one audience that could not reach it — the only public way to set the groups
/// is [`CoreContext::init_with_config`], which is `async`, while the places
/// that read them are not: a roster build, an agent build (packs are stripped
/// in `builder_build`, long before any turn), and an embedder's own synchronous
/// test fixtures.
///
/// First call wins, matching `DEFAULT_CONTEXT`'s own rule. A scoped context
/// still takes precedence in [`current`], so multi-tenant dispatch is
/// unaffected: this changes only what a caller with **no** context resolves to.
///
/// [`CoreContext`]: crate::core::runtime::context::CoreContext
/// [`CoreContext::init_with_config`]: crate::core::runtime::context::CoreContext::init_with_config
pub fn set_process_default(groups: ToolGroups) {
    let _ = PROCESS_GROUPS.set(groups);
}

/// The ambient groups for the running core.
///
/// Resolution order: the scoped [`CoreContext`], then the process default from
/// [`set_process_default`], then [`ToolGroups::default`] — every group
/// withheld.
///
/// That last step is a **fail-closed** default, and it is load-bearing for a
/// host that packs deliberately (the desktop app). It is the wrong answer for an
/// embedder that packs nothing on purpose, which is why the middle step exists:
/// an embedder had no way to say so, so filling a pack silently removed
/// capability from it on the next bump.
///
/// [`CoreContext`]: crate::core::runtime::context::CoreContext
pub fn current() -> ToolGroups {
    resolve(
        crate::core::runtime::context::CoreContext::current().map(|c| c.tool_groups()),
        PROCESS_GROUPS.get().cloned(),
    )
}

/// The precedence itself, as a pure function of its two inputs.
///
/// Split out so the ordering is testable without touching process state:
/// `PROCESS_GROUPS` is a `OnceLock`, so a test that set it would decide the
/// posture for every other test sharing the binary — including the ones
/// asserting that packed tools *are* withheld.
fn resolve(scoped: Option<ToolGroups>, process: Option<ToolGroups>) -> ToolGroups {
    scoped.or(process).unwrap_or_default()
}

#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
