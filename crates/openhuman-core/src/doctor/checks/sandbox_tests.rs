use super::*;
use crate::doctor::contract::FindingSeverity;

struct DockerAvailability(bool);

#[async_trait]
impl SandboxProbe for DockerAvailability {
    async fn docker_available(&self) -> Result<bool, ()> {
        Ok(self.0)
    }

    fn landlock_available(&self) -> bool {
        false
    }
}

fn config(backend: SandboxBackend) -> crate::config::Config {
    let mut config = crate::config::Config::default();
    config.sandbox.backend = backend;
    config
}

#[tokio::test]
async fn configured_docker_without_a_daemon_is_an_error() {
    let check = SandboxBackendCheck::with_probe(Arc::new(DockerAvailability(false)));
    let finding = check
        .detect(&DoctorCtx::from_config(config(SandboxBackend::Docker)))
        .await
        .remove(0);

    assert_eq!(finding.check_id, "sandbox/backend");
    assert_eq!(finding.severity, FindingSeverity::Error);
    assert!(finding.message.contains("Docker backend is unavailable"));
}

#[tokio::test]
async fn available_docker_backend_passes() {
    let check = SandboxBackendCheck::with_probe(Arc::new(DockerAvailability(true)));
    let finding = check
        .detect(&DoctorCtx::from_config(config(SandboxBackend::Docker)))
        .await
        .remove(0);

    assert_eq!(finding.severity, FindingSeverity::Pass);
}

#[tokio::test]
async fn no_probe_runs_when_sandboxing_is_disabled_in_config() {
    let check = SandboxBackendCheck::with_probe(Arc::new(DockerAvailability(false)));
    let mut config = config(SandboxBackend::Docker);
    config.sandbox.enabled = Some(false);
    let finding = check
        .detect(&DoctorCtx::from_config(config))
        .await
        .remove(0);

    assert_eq!(finding.severity, FindingSeverity::Skipped);
    assert!(finding.message.contains("disabled"));
}
