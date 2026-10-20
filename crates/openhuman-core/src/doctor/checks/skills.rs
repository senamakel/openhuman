//! Read-only discovery diagnostics and opt-in GitHub preflight checks.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use crate::doctor::contract::{ComponentId, DoctorCheck, DoctorCtx, DoctorTier, Finding};

pub(super) fn for_context(ctx: &DoctorCtx) -> Vec<Arc<dyn DoctorCheck>> {
    let mut checks: Vec<Arc<dyn DoctorCheck>> = vec![Arc::new(SkillDiscoveryCheck)];
    #[cfg(feature = "skills")]
    if let Some(required) = required_github_preflight(ctx.config()) {
        checks.push(Arc::new(GithubPreflightCheck {
            required: required.config,
            manifest_path: required.path,
            #[cfg(test)]
            probes: None,
        }));
    }
    #[cfg(not(feature = "skills"))]
    let _ = ctx;
    checks
}

struct SkillDiscoveryCheck;

#[async_trait]
impl DoctorCheck for SkillDiscoveryCheck {
    fn id(&self) -> &str {
        "skills/discovery"
    }

    fn category(&self) -> &str {
        "skills"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Static
    }

    fn component_gate(&self) -> Option<ComponentId> {
        Some(ComponentId::Skills)
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        if !ctx.config().components.skills {
            return vec![Finding::skipped(
                self.id(),
                "Skills are disabled in the selected configuration.",
                "Enable skills in the selected configuration to inspect manifests.",
            )];
        }
        #[cfg(feature = "skills")]
        {
            let home = crate::skills::ops_discover::discovery_home_dir();
            let trusted =
                crate::skills::ops_discover::is_workspace_trusted(&ctx.config().workspace_dir);
            return crate::skills::ops_discover::discover_workflows(
                home.as_deref(),
                Some(&ctx.config().workspace_dir),
                trusted,
            )
            .into_iter()
            .filter(|skill| !skill.warnings.is_empty())
            .map(|skill| {
                let mut finding = Finding::new(
                    self.id(),
                    crate::doctor::FindingSeverity::Warn,
                    "A discovered skill manifest has validation warnings.",
                    "Review the skill manifest and rerun doctor.",
                );
                finding.config_path = skill
                    .location
                    .map(|path| path.to_string_lossy().into_owned());
                finding
            })
            .collect();
        }
        #[cfg(not(feature = "skills"))]
        {
            let _ = ctx;
            vec![Finding::skipped(
                self.id(),
                "Skill discovery is disabled in this build.",
                "Use a build with skills support to inspect skill manifests.",
            )]
        }
    }
}

#[cfg(feature = "skills")]
struct RequiredGithubPreflight {
    config: crate::skills::registry::WorkflowGithubConfig,
    path: PathBuf,
}

/// Finds a required GitHub gate using the same read-only discovery roots as
/// skills. This deliberately avoids `load_workflows`, which prunes legacy
/// bundles as part of its normal run-path preparation.
#[cfg(feature = "skills")]
fn required_github_preflight(config: &crate::config::Config) -> Option<RequiredGithubPreflight> {
    use crate::skills::registry::{IdentityMatch, WorkflowDefinition};

    let home = crate::skills::ops_discover::discovery_home_dir();
    let trusted = crate::skills::ops_discover::is_workspace_trusted(&config.workspace_dir);
    let discovered = crate::skills::ops_discover::discover_workflows(
        home.as_deref(),
        Some(&config.workspace_dir),
        trusted,
    );
    let mut required = Vec::new();
    for skill in discovered {
        let Some(document) = skill.location else {
            continue;
        };
        let Some(dir) = document.parent() else {
            continue;
        };
        let manifest = ["workflow.toml", "skill.toml"]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file());
        let Some(manifest) = manifest else {
            continue;
        };
        let Ok(text) = tinyskills::read_document(&manifest) else {
            continue;
        };
        let Ok(definition) = toml::from_str::<WorkflowDefinition>(&text) else {
            continue;
        };
        let Some(github) = definition.github.filter(|github| github.required) else {
            continue;
        };
        required.push(RequiredGithubPreflight {
            config: github,
            path: manifest,
        });
    }

    required.sort_by(|left, right| left.path.cmp(&right.path));
    // A single readiness check covers the shared local Git and GitHub
    // integration. Honor the most demanding declared identity policy.
    required
        .into_iter()
        .max_by_key(|entry| match entry.config.identity_match {
            IdentityMatch::None => 0,
            IdentityMatch::Any => 1,
            IdentityMatch::Strict => 2,
        })
}

#[cfg(feature = "skills")]
struct GithubPreflightCheck {
    required: crate::skills::registry::WorkflowGithubConfig,
    manifest_path: PathBuf,
    #[cfg(test)]
    probes: Option<Arc<dyn crate::skills::preflight::PreflightProbes>>,
}

#[cfg(feature = "skills")]
#[async_trait]
impl DoctorCheck for GithubPreflightCheck {
    fn id(&self) -> &str {
        "skills/github-preflight"
    }

    fn category(&self) -> &str {
        "skills"
    }

    fn tier(&self) -> DoctorTier {
        DoctorTier::Connect
    }

    fn component_gate(&self) -> Option<ComponentId> {
        Some(ComponentId::Skills)
    }

    async fn detect(&self, ctx: &DoctorCtx) -> Vec<Finding> {
        if !ctx.config().components.skills {
            let mut finding = Finding::skipped(
                self.id(),
                "Skills are disabled in the selected configuration.",
                "Enable skills in the selected configuration to run GitHub preflight.",
            );
            finding.config_path = Some(self.manifest_path.to_string_lossy().into_owned());
            return vec![finding];
        }
        #[cfg(test)]
        let result = if let Some(probes) = &self.probes {
            crate::skills::preflight::run_github_preflight(Some(&self.required), probes.as_ref())
                .await
        } else {
            let probes = crate::skills::preflight::LivePreflightProbes::new(ctx.config());
            crate::skills::preflight::run_github_preflight(Some(&self.required), &probes).await
        };
        #[cfg(not(test))]
        let result = {
            let probes = crate::skills::preflight::LivePreflightProbes::new(ctx.config());
            crate::skills::preflight::run_github_preflight(Some(&self.required), &probes).await
        };

        let mut finding = match result {
            Ok(()) => Finding::pass(self.id(), "The required GitHub skill preflight is ready."),
            Err(error) => Finding::error(
                self.id(),
                github_error_message(&error),
                "Review the GitHub connection and local Git setup, then rerun doctor.",
            ),
        };
        finding.config_path = Some(self.manifest_path.to_string_lossy().into_owned());
        vec![finding]
    }
}

#[cfg(feature = "skills")]
fn github_error_message(error: &crate::skills::preflight::GithubGateError) -> String {
    use crate::skills::preflight::GithubGateError;

    match error {
        GithubGateError::ComposioGithubMissing => {
            "The required GitHub integration is not connected.".to_owned()
        }
        GithubGateError::GitBinaryMissing(_) => "Git is not available.".to_owned(),
        GithubGateError::GitUserNameMissing => "Local Git user.name is not configured.".to_owned(),
        GithubGateError::GitUserEmailMissing => {
            "Local Git user.email is not configured.".to_owned()
        }
        GithubGateError::IdentityMismatch { .. } => {
            "The connected GitHub identity does not match local Git identity.".to_owned()
        }
        GithubGateError::ComposioIdentityUnresolved => {
            "The connected GitHub identity could not be verified.".to_owned()
        }
    }
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
