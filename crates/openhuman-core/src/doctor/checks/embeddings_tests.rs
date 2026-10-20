use crate::config::Config;
use crate::doctor::runner;
use crate::doctor::{DoctorCtx, DoctorOptions, DoctorTier, Finding, FindingSeverity};

const EMBEDDINGS_CONFIG: &str = "embeddings/config";

async fn run_only(config: Config) -> Vec<Finding> {
    let context = DoctorCtx::from_config(config);
    runner::run(
        &context,
        DoctorOptions {
            tier: DoctorTier::Static,
            only: vec![EMBEDDINGS_CONFIG.to_owned()],
            ..DoctorOptions::default()
        },
    )
    .await
    .expect("the selected doctor check should run")
    .findings
}

fn finding(findings: &[Finding]) -> &Finding {
    findings
        .iter()
        .find(|finding| finding.check_id == EMBEDDINGS_CONFIG)
        .unwrap_or_else(|| panic!("expected finding from {EMBEDDINGS_CONFIG}: {findings:#?}"))
}

#[tokio::test]
async fn an_unknown_embedding_provider_reports_its_config_path() {
    let mut config = Config::default();
    config.memory.embedding_provider = "nope".to_owned();

    let findings = run_only(config).await;
    let result = finding(&findings);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(
        result.config_path.as_deref(),
        Some("memory.embedding_provider")
    );
}

#[tokio::test]
async fn missing_embedding_model_and_dimensions_are_reported() {
    let mut config = Config::default();
    config.memory.embedding_model.clear();
    config.memory.embedding_dimensions = 0;

    let findings = run_only(config).await;
    let paths = findings
        .iter()
        .filter(|result| result.check_id == EMBEDDINGS_CONFIG)
        .filter_map(|result| result.config_path.as_deref())
        .collect::<Vec<_>>();

    assert!(
        paths.contains(&"memory.embedding_model"),
        "missing model path in findings: {findings:#?}"
    );
    assert!(
        paths.contains(&"memory.embedding_dimensions"),
        "missing dimensions path in findings: {findings:#?}"
    );
    assert!(
        findings
            .iter()
            .filter(|result| result.check_id == EMBEDDINGS_CONFIG)
            .all(|result| result.severity == FindingSeverity::Error)
    );
}

#[tokio::test]
async fn known_model_with_unsupported_dimensions_reports_the_dimension_path() {
    let mut config = Config::default();
    config.memory.embedding_provider = "openai".to_owned();
    config.memory.embedding_model = "text-embedding-3-small".to_owned();
    config.memory.embedding_dimensions = 1024;

    let findings = run_only(config).await;
    let result = finding(&findings);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(
        result.config_path.as_deref(),
        Some("memory.embedding_dimensions")
    );
}

#[tokio::test]
async fn active_embedding_route_must_match_memory_provider_and_dimensions() {
    use crate::config::EmbeddingRouteConfig;

    let mut config = Config::default();
    config.memory.embedding_model = "hint:recall".to_owned();
    config.memory.embedding_dimensions = 1024;
    config.embedding_routes = vec![EmbeddingRouteConfig {
        hint: "recall".to_owned(),
        provider: "openai".to_owned(),
        model: "text-embedding-3-small".to_owned(),
        dimensions: Some(1024),
    }];

    let findings = run_only(config).await;
    let paths = findings
        .iter()
        .filter(|result| result.check_id == EMBEDDINGS_CONFIG)
        .filter_map(|result| result.config_path.as_deref())
        .collect::<Vec<_>>();

    assert!(
        paths.contains(&"embedding_routes[0].dimensions"),
        "missing unsupported active route dimension finding: {findings:#?}"
    );
}

#[tokio::test]
async fn embedding_workload_model_must_match_memory_model() {
    let mut config = Config::default();
    config.memory.embedding_provider = "openai".to_owned();
    config.memory.embedding_model = "text-embedding-3-large".to_owned();
    config.memory.embedding_dimensions = 3072;
    config.embeddings_provider = Some("openai:text-embedding-3-small".to_owned());

    let findings = run_only(config).await;
    let result = finding(&findings);

    assert_eq!(result.severity, FindingSeverity::Error);
    assert_eq!(result.config_path.as_deref(), Some("embeddings_provider"));
}
