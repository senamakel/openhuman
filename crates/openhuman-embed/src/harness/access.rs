//! What the agent is allowed to do.
//!
//! # The trap this type exists to close
//!
//! Access is governed by **two independent mechanisms**, and setting only the
//! obvious one produces an agent that looks configured and silently refuses to
//! act:
//!
//! 1. **The autonomy tier** (`config.autonomy.level`) drives `SecurityPolicy` —
//!    which command classes are allowed, prompted, or blocked.
//! 2. **The turn origin** (a task-local, [`AgentTurnOrigin`]) is the caller's
//!    statement of authority, and the approval gate is *fail-closed* on it. An
//!    unlabelled call site is hard-denied for external-effect tools regardless
//!    of tier.
//!
//! An embedder that sets `level = "full"` and nothing else gets a turn whose
//! `shell`, `edit`, `apply_patch` and `*_exec` calls all refuse — with a
//! plausible-looking transcript, because the model narrates around the refusals.
//! It reads as a weak model rather than a missing scope, which is what makes it
//! expensive to diagnose. [`Access`] therefore always yields **both** halves
//! together, and they cannot be set independently.

use openhuman_core::agent::turn_origin::{AgentTurnOrigin, TrustedAutomationSource};
use openhuman_core::security::{AutonomyLevel, TrustedAccess, TrustedRoot};

/// The `ExternalChannel` channel name [`Access::public`] turns carry.
const PUBLIC_CHANNEL: &str = "public";
/// The event channel an embedded agent's sessions run on.
const SESSION_CHANNEL: &str = "internal";

/// The authority a harness's turns run with.
#[derive(Debug, Clone)]
pub struct Access {
    level: AutonomyLevel,
    /// `None` means "let the core apply its own default for a direct chat
    /// dispatch", which is the trusted-operator `Cli` allowance.
    origin: Option<AgentTurnOrigin>,
    trusted_roots: Vec<TrustedRoot>,
    allow_tool_install: bool,
    approval_gate: bool,
    /// Set by [`Access::public`]: the session's tool permission ceiling is
    /// read-only, enforced at the tool boundary rather than by the tier alone.
    public: bool,
}

impl Default for Access {
    /// [`Access::supervised`] — the same default the core itself uses.
    fn default() -> Self {
        Self::supervised()
    }
}

impl Access {
    /// Observe but never act: no writes, no shell, no network side effects.
    ///
    /// The safe default for running an untrusted prompt.
    pub fn readonly() -> Self {
        Self {
            level: AutonomyLevel::ReadOnly,
            // A read-only agent has nothing to approve, and labelling the turn
            // as automation would grant an allowance the tier already refuses.
            origin: None,
            trusted_roots: Vec::new(),
            allow_tool_install: false,
            approval_gate: true,
            public: false,
        }
    }

    /// Act, but park risky operations for a human decision.
    ///
    /// The approval gate stays on, which means an *unattended* harness will
    /// stall here: parked turns hold for a 10-minute TTL and then deny. Use
    /// [`Access::full`] for automation, or answer the approvals.
    pub fn supervised() -> Self {
        Self {
            level: AutonomyLevel::Supervised,
            origin: None,
            trusted_roots: Vec::new(),
            allow_tool_install: false,
            approval_gate: true,
            public: false,
        }
    }

    /// Act autonomously within policy bounds, without pausing for approval.
    ///
    /// Sets the tier *and* labels turns as trusted automation, because either
    /// alone is not enough — see the module docs. Hard blocks are unaffected:
    /// credential stores and system directories stay forbidden, and the agent
    /// still cannot write into the workspace's internal state.
    ///
    /// This grants an agent real ability to run commands and edit files under
    /// its `action_dir`. Give it a directory you are willing to have changed.
    pub fn full() -> Self {
        Self {
            level: AutonomyLevel::Full,
            origin: Some(AgentTurnOrigin::TrustedAutomation {
                job_id: "embedded-harness".to_string(),
                source: TrustedAutomationSource::Workflow {
                    require_approval: false,
                },
            }),
            trusted_roots: Vec::new(),
            allow_tool_install: false,
            approval_gate: false,
            public: false,
        }
    }

    /// For untrusted public input: anyone can put text in front of this agent.
    ///
    /// Turns run as an [`AgentTurnOrigin::ExternalChannel`] (`channel:
    /// "public"`), the autonomy policy is enabled at
    /// [`AutonomyLevel::ReadOnly`], and the session's tool permission ceiling
    /// is read-only. Every tool that writes, executes or has an external effect
    /// is then refused **immediately**: the model gets a tool error naming the
    /// reason, and nothing parks waiting for an approval no one on a public
    /// channel could give. Read-only tools — including a host's own — work.
    ///
    /// Pair it with [`AgentSpec::lockdown`](crate::AgentSpec::lockdown), which
    /// bounds *which* tools exist; this bounds what the ones that exist may do.
    pub fn public() -> Self {
        Self {
            level: AutonomyLevel::ReadOnly,
            origin: Some(AgentTurnOrigin::ExternalChannel {
                channel: PUBLIC_CHANNEL.to_string(),
                sender: None,
                reply_target: String::new(),
                message_id: String::new(),
            }),
            trusted_roots: Vec::new(),
            allow_tool_install: false,
            // Nothing is parked: refusals happen at the tool boundary.
            approval_gate: false,
            public: true,
        }
    }

    /// Grant access to a directory outside the workspace.
    ///
    /// Takes precedence over `workspace_only`, except for credential stores
    /// (`~/.ssh`, `~/.gnupg`, `~/.aws`), which stay blocked whatever is granted.
    pub fn trust(mut self, path: impl Into<String>, access: TrustedAccess) -> Self {
        self.trusted_roots.push(TrustedRoot {
            path: path.into(),
            access,
        });
        self
    }

    /// Permit the agent to install OS packages via the `install_tool` tool.
    ///
    /// Off in every preset, including [`full`](Self::full): installing software
    /// on the host reaches outside the action directory that otherwise bounds
    /// the blast radius, so it is opted into by name rather than implied by a
    /// tier.
    pub fn allow_tool_install(mut self, allow: bool) -> Self {
        self.allow_tool_install = allow;
        self
    }

    /// Override the turn origin.
    ///
    /// The presets pick one for you. Reach for this when the harness is driving
    /// on behalf of something with a narrower authority than "trusted
    /// automation" — an inbound external message, say — so the approval gate
    /// applies the grant that actually matches.
    pub fn origin(mut self, origin: AgentTurnOrigin) -> Self {
        self.origin = Some(origin);
        self
    }

    /// The origin turns run under, if this access level states one.
    pub fn turn_origin(&self) -> Option<&AgentTurnOrigin> {
        self.origin.as_ref()
    }

    /// Whether the interactive approval gate should park turns.
    pub fn approval_gate_enabled(&self) -> bool {
        self.approval_gate
    }

    /// Write this access level into `config`, **replacing** whatever
    /// `trusted_roots` it already carried.
    ///
    /// An `Access` is the complete authority statement for whoever applies
    /// it, not an addition to it: `Runtime::agent` starts each agent's config
    /// from the runtime's own base config (which already has the runtime
    /// default `Access` applied), then applies the agent's own `Access` on
    /// top. Extending rather than replacing would let a narrower per-agent
    /// grant — e.g. `Access::readonly()`, which trusts nothing — keep every
    /// root the runtime default trusted, silently widening a supposedly
    /// scoped-down agent.
    pub(crate) fn apply(&self, config: &mut openhuman_core::config::Config) {
        config.autonomy.level = self.level;
        config.autonomy.allow_tool_install = self.allow_tool_install;
        config.autonomy.trusted_roots = self.trusted_roots.clone();
        if self.public {
            // Tiers are inert while the policy is off, so turn it on. A
            // non-empty `channel_permissions` map caps every channel it does
            // not list at read-only; the session channel is named anyway so
            // the intent reads from the config.
            config.autonomy.enabled = true;
            config.agent.channel_permissions =
                std::iter::once((SESSION_CHANNEL.to_string(), "readonly".to_string())).collect();
        }
        // `auto_approve_all` is deliberately NOT set for `full()`. The origin
        // is the correct instrument — it says *who is calling*, which the gate
        // can reason about — whereas `auto_approve_all` is a blanket bypass
        // that would also cover call sites this harness never intended to
        // vouch for.
    }
}

#[cfg(test)]
#[path = "access_tests.rs"]
mod tests;
