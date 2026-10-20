use crate::config::Config;
use crate::doctor::runner;
use crate::doctor::{DoctorCtx, DoctorOptions, DoctorTier, Finding, FindingSeverity};

const PROVIDER_REF: &str = "inference/provider-ref";
const CONSTRUCT: &str = "inference/construct";
const TEMPERATURE: &str = "inference/temperature";

async fn run_only(config: Config, check_id: &str) -> Vec<Finding> {
    let context = DoctorCtx::from_config(config);
    runner::run(
        &context,
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec![check_id.to_owned()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("the selected doctor check should run")
    .findings
}

fn finding<'a>(findings: &'a [Finding], check_id: &str) -> &'a Finding {
    findings
        .iter()
        .find(|finding| finding.check_id == check_id)
        .unwrap_or_else(|| panic!("expected finding from {check_id}: {findings:#?}"))
}

#[tokio::test]
async fn an_unregistered_chat_provider_reports_its_config_path() {
    let mut config = Config::default();
    config.chat_provider = Some("nope".to_owned());

    let findings = run_only(config, PROVIDER_REF).await;
    let result = finding(&findings, PROVIDER_REF);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(result.config_path.as_deref(), Some("chat_provider"));
}

#[tokio::test]
async fn a_byok_cloud_fallback_without_a_key_reports_construction_failure() {
    use crate::config::schema::cloud_providers::{AuthStyle, CloudProviderCreds};

    let mut config = Config::default();
    config.chat_provider = Some("ollama:qwen2.5".to_owned());
    config.primary_cloud = Some("provider-openai".to_owned());
    config.cloud_providers = vec![CloudProviderCreds {
        id: "provider-openai".to_owned(),
        slug: "openai".to_owned(),
        label: "OpenAI".to_owned(),
        endpoint: "https://api.openai.com/v1".to_owned(),
        auth_style: AuthStyle::Bearer,
        default_model: Some("gpt-4o".to_owned()),
    }];

    let findings = run_only(config, CONSTRUCT).await;
    let result = finding(&findings, CONSTRUCT);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(result.config_path.as_deref(), Some("primary_cloud"));
    assert!(result.message.to_ascii_lowercase().contains("credential"));
}

#[tokio::test]
async fn a_fully_configured_managed_default_constructs_cleanly() {
    let findings = run_only(Config::default(), CONSTRUCT).await;

    assert!(
        findings.iter().all(|result| result.severity == FindingSeverity::Pass),
        "managed defaults should not report construction errors: {findings:#?}"
    );
}

#[tokio::test]
async fn an_out_of_range_temperature_reports_the_role_setting() {
    let mut config = Config::default();
    config.default_temperature = 3.5;

    let findings = run_only(config, TEMPERATURE).await;
    let result = finding(&findings, TEMPERATURE);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(result.config_path.as_deref(), Some("default_temperature"));
}
