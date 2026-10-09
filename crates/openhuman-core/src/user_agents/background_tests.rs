use super::*;
use crate::core::runtime::{CoreContext, DomainSet, SaasConfig};
use crate::memory::lifecycle::jobs::enqueue;
use crate::user_agents::layout::agent_config;
use crate::user_agents::UserAgentId;
use tinymemory_api::{ConsolidateRequest, ItemKind, Namespace, Reach};
use tinymemory_tools::BackgroundJob;

fn host(tmp: &tempfile::TempDir, idle_secs: u64) -> AgentHost {
    let mut saas = SaasConfig::new(tmp.path());
    saas.idle_evict_secs = idle_secs;
    AgentHost::new(saas, CoreContext::for_test(DomainSet::full(), None))
}

fn build_job() -> BackgroundJob {
    BackgroundJob::BuildBeliefs {
        request: ConsolidateRequest::new(Reach::exact(Namespace::agent("a")))
            .kinds([ItemKind::Conversation]),
    }
}

#[tokio::test]
async fn nothing_to_do_does_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 3600);
    assert_eq!(tick(&host).await, TickReport::default());
    let id = UserAgentId::for_user(&format!("idle-{}", uuid::Uuid::new_v4())).unwrap();
    host.provision(&id).unwrap();
    assert_eq!(tick(&host).await, TickReport::default());
    assert!(
        !host.is_open(&id),
        "an agent with no queued work is not opened"
    );
}

#[tokio::test]
async fn only_agents_with_queued_memory_jobs_are_run() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 3600);
    let busy = UserAgentId::for_user(&format!("busy-{}", uuid::Uuid::new_v4())).unwrap();
    let quiet = UserAgentId::for_user(&format!("quiet-{}", uuid::Uuid::new_v4())).unwrap();
    host.provision(&busy).unwrap();
    host.provision(&quiet).unwrap();

    let config = agent_config(&host.layout_of(&busy), &busy);
    crate::memory::test_fixtures::bind_reference(&config);
    enqueue(&config, &Namespace::ROOT, vec![build_job()]).await;
    assert!(jobs::has_pending(&config.workspace_dir));

    let report = tick(&host).await;
    assert_eq!(report.ran, 1, "{report:?}");
    assert!(
        host.is_open(&busy),
        "the busy agent was opened to run its jobs"
    );
    assert!(!host.is_open(&quiet), "the quiet agent was left closed");
}

#[tokio::test]
async fn a_tick_sweeps_idle_agents() {
    let tmp = tempfile::tempdir().unwrap();
    let host = host(&tmp, 0);
    let id = UserAgentId::for_user(&format!("sweep-{}", uuid::Uuid::new_v4())).unwrap();
    host.provision(&id).unwrap();
    drop(host.open(&id).unwrap());
    assert!(host.is_open(&id));
    tick(&host).await;
    assert!(!host.is_open(&id));
}
