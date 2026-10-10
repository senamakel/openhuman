//! Title: Two agents with independent prompts and working folders
//! Summary: Two agents with independent prompts and working folders.
//! Run: offline; optional live via OPENHUMAN_EXAMPLE_LIVE=1 and BASE_URL/API_KEY/MODEL.
//! Feature: default

mod support;
use openhuman_embed::{AgentSpec, Runtime, Workspace};

fn main() -> anyhow::Result<()> {
    support::run(run())
}

async fn run() -> anyhow::Result<()> {
    let backend = support::stub_backend().await;
    let provider = support::provider("hello from the stub").await;
    let runtime = Runtime::builder()
        .config(support::offline_config())
        .workspace(Workspace::Ephemeral)
        .backend_url(backend.uri())
        .provider(support::example_provider(&provider)?)
        .build()
        .await?;
    // ANCHOR: two_agents
    let reviewer_dir = tempfile::tempdir()?;
    let fixer_dir = tempfile::tempdir()?;
    let reviewer = runtime.agent(
        AgentSpec::new("reviewer")
            .system_prompt("REVIEWER_PROMPT: review code")
            .action_dir(reviewer_dir.path())
            .access(openhuman_embed::Access::readonly()),
    )?;
    let fixer = runtime.agent(
        AgentSpec::new("fixer")
            .system_prompt("FIXER_PROMPT: explain fixes")
            .action_dir(fixer_dir.path())
            .access(openhuman_embed::Access::full()),
    )?;
    assert_ne!(reviewer.action_dir(), fixer.action_dir());
    assert_ne!(reviewer.home_dir(), fixer.home_dir());
    assert_ne!(reviewer.workspace_dir(), reviewer.action_dir());
    assert!(!reviewer.run("Review").await?.reply.is_empty());
    assert!(!fixer.run("Explain").await?.reply.is_empty());
    if support::offline() {
        let requests = support::chat_requests(&provider).await;
        assert_eq!(requests.len(), 2);
        assert!(String::from_utf8_lossy(&requests[0].body).contains("REVIEWER_PROMPT"));
        assert!(!String::from_utf8_lossy(&requests[0].body).contains("FIXER_PROMPT"));
        assert!(String::from_utf8_lossy(&requests[1].body).contains("FIXER_PROMPT"));
    }
    println!("two distinct prompts and action workspaces verified");
    // ANCHOR_END: two_agents
    support::passed("two_agents");
    Ok(())
}
