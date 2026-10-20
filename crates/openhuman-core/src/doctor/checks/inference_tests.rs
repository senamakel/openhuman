use crate::config::Config;
use crate::doctor::runner;
use crate::doctor::{DoctorCtx, DoctorOptions, DoctorTier, Finding, FindingSeverity};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tinyinference_llm::model::{ChatModel, ModelRequest, ModelResponse};

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

fn isolated_config(root: &std::path::Path) -> Config {
    let mut config = Config::default();
    config.config_path = root.join("config.toml");
    config.workspace_dir = root.join("workspace");
    config
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
    use crate::config::test_env::EnvVarGuard;

    let _inference_guard = crate::inference::inference_test_guard_async().await;
    let temp = tempfile::tempdir().expect("temporary configuration directory");
    let _env_guard = EnvVarGuard::locked_async()
        .await
        .with("HOME", temp.path())
        .with("OPENHUMAN_WORKSPACE", temp.path());

    let mut config = isolated_config(temp.path());
    config.chat_provider = Some("ollama:qwen2.5".to_owned());
    config.primary_cloud = Some("provider-doctor-test".to_owned());
    config.cloud_providers = vec![CloudProviderCreds {
        id: "provider-doctor-test".to_owned(),
        slug: "doctor-test".to_owned(),
        label: "Doctor test provider".to_owned(),
        endpoint: "https://doctor-test.invalid/v1".to_owned(),
        auth_style: AuthStyle::Bearer,
        default_model: Some("test-model".to_owned()),
    }];

    let findings = run_only(config, CONSTRUCT).await;
    let result = finding(&findings, CONSTRUCT);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(result.config_path.as_deref(), Some("primary_cloud"));
    assert!(result.message.to_ascii_lowercase().contains("credential"));
    assert!(!temp.path().join("auth-profiles.json").exists());
    assert!(!temp.path().join("auth-profiles.lock").exists());
}

#[tokio::test]
async fn a_fully_configured_managed_default_constructs_cleanly() {
    use crate::config::test_env::EnvVarGuard;
    use crate::inference::registry::{register_chat_model, unregister_chat_model, ChatModelEntry};
    use crate::inference::provider::factory::PROVIDER_OPENHUMAN;
    use tinyinference_llm::error::Error as ModelError;

    struct ManagedModel {
        generations: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl ChatModel<()> for ManagedModel {
        async fn invoke(
            &self,
            _state: &(),
            _request: ModelRequest,
        ) -> tinyinference_llm::Result<ModelResponse> {
            self.generations.fetch_add(1, Ordering::SeqCst);
            Err(ModelError::Model("doctor test model must not run".to_owned()))
        }
    }

    let _inference_guard = crate::inference::inference_test_guard_async().await;
    let temp = tempfile::tempdir().expect("temporary configuration directory");
    let _env_guard = EnvVarGuard::locked_async()
        .await
        .with("HOME", temp.path())
        .with("OPENHUMAN_WORKSPACE", temp.path());
    let constructions = Arc::new(AtomicUsize::new(0));
    let generations = Arc::new(AtomicUsize::new(0));
    let build_constructions = Arc::clone(&constructions);
    let model_generations = Arc::clone(&generations);
    register_chat_model(
        PROVIDER_OPENHUMAN,
        ChatModelEntry {
            build: Arc::new(move |_| {
                build_constructions.fetch_add(1, Ordering::SeqCst);
                Ok((
                    Arc::new(ManagedModel {
                        generations: Arc::clone(&model_generations),
                    }) as Arc<dyn ChatModel<()>>,
                    "managed-test-model".to_owned(),
                ))
            }),
            probe: None,
        },
    );
    let _registered_model = RegisterManagedModel;

    let findings = run_only(isolated_config(temp.path()), CONSTRUCT).await;

    assert!(
        !findings.is_empty()
            && findings
                .iter()
                .all(|result| result.severity == FindingSeverity::Pass),
        "managed defaults should not report construction errors: {findings:#?}"
    );
    assert!(
        constructions.load(Ordering::SeqCst) > 0,
        "the managed provider factory must be consulted"
    );
    assert_eq!(generations.load(Ordering::SeqCst), 0);
    assert!(!temp.path().join("auth-profiles.json").exists());
    assert!(!temp.path().join("auth-profiles.lock").exists());
}

struct RegisterManagedModel;

impl Drop for RegisterManagedModel {
    fn drop(&mut self) {
        crate::inference::registry::unregister_chat_model(
            crate::inference::provider::factory::PROVIDER_OPENHUMAN,
        );
    }
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
