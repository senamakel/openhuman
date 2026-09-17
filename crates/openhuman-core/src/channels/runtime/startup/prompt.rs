//! Rendering the agent's filesystem-access boundaries into the system prompt.

use crate::security::SecurityPolicy;

/// Render the agent's current filesystem-access boundaries as a system-prompt
/// section. Advisory only: the `SecurityPolicy` enforces these regardless of
/// what the model believes, but stating them keeps the model from wasting turns
/// attempting actions the runtime will deny.
pub(super) fn format_access_context(security: &SecurityPolicy) -> String {
    use crate::security::{AutonomyLevel, TrustedAccess};

    let mode = match security.autonomy {
        AutonomyLevel::ReadOnly => "read-only (observe only; no writes or shell commands)",
        AutonomyLevel::Supervised => "supervised (acts; risky operations require approval)",
        AutonomyLevel::Full => "full (autonomous within policy bounds)",
    };
    let mut s =
        String::from("\n\n## Host access (enforced by the runtime — you cannot exceed this)\n");
    s.push_str(&format!("- Access mode: {mode}\n"));
    s.push_str(&format!(
        "- Workspace: {} ({})\n",
        security.workspace_dir.display(),
        if security.workspace_only {
            "file access confined to the workspace"
        } else {
            "workspace_only is OFF"
        }
    ));
    if security.trusted_roots.is_empty() {
        s.push_str("- Trusted roots outside the workspace: none granted\n");
    } else {
        s.push_str("- Trusted roots outside the workspace:\n");
        for root in &security.trusted_roots {
            let access = match root.access {
                TrustedAccess::Read => "read-only",
                TrustedAccess::ReadWrite => "read+write",
            };
            s.push_str(&format!("    - {} ({access})\n", root.path));
        }
    }
    s.push_str(&format!(
        "- OS package installation: {}\n",
        if security.allow_tool_install {
            "allowed via install_tool"
        } else {
            "disabled"
        }
    ));
    s.push_str(
        "Credential stores (~/.ssh, ~/.gnupg, ~/.aws) are always blocked. \
         Use detect_tools to check what's installed before assuming a tool exists.\n",
    );
    s
}
