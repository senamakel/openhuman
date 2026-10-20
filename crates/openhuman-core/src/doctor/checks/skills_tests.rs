use super::*;
use crate::core::runtime::{CoreContext, DomainSet};
use crate::core::slots::Extensions;
use crate::doctor::contract::FindingSeverity;
use crate::skills::preflight::PreflightProbes;
use std::path::Path;

fn config_in(root: &Path) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.workspace_dir = root.join("workspace");
    config.action_dir = root.join("actions");
    config.config_path = root.join("config.toml");
    config
}

fn scoped(config: crate::config::Config) -> Arc<CoreContext> {
    CoreContext::for_test_with_extensions(DomainSet::full(), None, Extensions::new()).derive_with(
        crate::core::runtime::ContextOverlay::new(config, DomainSet::full(), Default::default())
            .without_user_skill_roots(),
    )
}

#[tokio::test]
async fn github_preflight_is_not_registered_when_no_skill_requires_it() {
    let scratch = tempfile::tempdir().expect("create isolated skill fixture");
    let config = config_in(scratch.path());
    let skill_dir = config.workspace_dir.join("skills/plain-skill");
    std::fs::create_dir_all(&skill_dir).expect("create skill directory");
    std::fs::write(skill_dir.join("SKILL.md"), "A plain skill.\n").expect("write skill document");

    let context = scoped(config.clone());
    let checks = CoreContext::scope(context, async {
        for_context(&DoctorCtx::from_config(config.clone()))
    })
    .await;

    assert!(checks.iter().any(|check| check.id() == "skills/discovery"));
    assert!(checks
        .iter()
        .all(|check| check.id() != "skills/github-preflight"));
}

#[derive(Default)]
struct MissingGithubProbe;

#[async_trait]
impl PreflightProbes for MissingGithubProbe {
    async fn composio_toolkit_active(&self, _toolkit: &str) -> bool {
        false
    }

    async fn composio_identity(&self, _toolkit: &str) -> Option<String> {
        None
    }

    async fn git_version(&self) -> Result<(), String> {
        Ok(())
    }

    async fn git_user_name(&self) -> String {
        "private local name".to_owned()
    }

    async fn git_user_email(&self) -> String {
        "private@example.test".to_owned()
    }
}

#[tokio::test]
async fn required_github_preflight_uses_fake_probes_and_redacts_error_details() {
    let scratch = tempfile::tempdir().expect("create isolated skill fixture");
    let config = config_in(scratch.path());
    let skill_dir = config.workspace_dir.join("skills/required-skill");
    std::fs::create_dir_all(&skill_dir).expect("create skill directory");
    std::fs::write(skill_dir.join("WORKFLOW.md"), "A workflow.\n")
        .expect("write workflow document");
    let manifest = skill_dir.join("workflow.toml");
    std::fs::write(
        &manifest,
        "id = \"required-skill\"\nwhen_to_use = \"Use the GitHub API\"\n\
         [github]\nrequired = true\nidentity_match = \"strict\"\n",
    )
    .expect("write GitHub gate config");

    let context = scoped(config.clone());
    let (required, checks) = CoreContext::scope(context, async {
        (
            required_github_preflight(&config),
            for_context(&DoctorCtx::from_config(config.clone())),
        )
    })
    .await;
    let required = required.expect("the workflow explicitly requires GitHub");
    assert_eq!(required.path, manifest);
    assert!(checks
        .iter()
        .any(|check| check.id() == "skills/github-preflight"));

    let check = GithubPreflightCheck {
        required: required.config,
        manifest_path: required.path,
        probes: Some(Arc::new(MissingGithubProbe)),
    };
    let finding = check
        .detect(&DoctorCtx::from_config(config))
        .await
        .remove(0);

    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding
        .message
        .contains("GitHub integration is not connected"));
    assert!(!finding.message.contains("private"));
    assert!(!finding.message.contains("example.test"));
    assert_eq!(
        finding.config_path.as_deref(),
        Some(manifest.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn malformed_skill_manifest_reports_only_a_warning_and_its_path() {
    let scratch = tempfile::tempdir().expect("create isolated skill fixture");
    let config = config_in(scratch.path());
    let skill_dir = config.workspace_dir.join("skills/broken-skill");
    std::fs::create_dir_all(&skill_dir).expect("create skill directory");
    let manifest = skill_dir.join("SKILL.md");
    std::fs::write(&manifest, "---\nname: [broken\n---\nA test skill.\n")
        .expect("write malformed skill manifest");

    let context = scoped(config.clone());
    let findings = CoreContext::scope(context, async {
        SkillDiscoveryCheck
            .detect(&DoctorCtx::from_config(config))
            .await
    })
    .await;

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].severity, FindingSeverity::Warn);
    assert_eq!(
        findings[0].config_path.as_deref(),
        Some(manifest.to_string_lossy().as_ref())
    );
    assert!(!findings[0].message.contains("broken"));
}
