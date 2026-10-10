//! Title: Copy a skill bundle into its agent workspace
//! Summary: Copy a skill bundle into its agent workspace.
//! Run: offline; optional live via OPENHUMAN_EXAMPLE_LIVE=1 and BASE_URL/API_KEY/MODEL.
//! Feature: skills

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
    // ANCHOR: skills
    let skills = tempfile::tempdir()?;
    std::fs::create_dir(skills.path().join("review"))?;
    std::fs::write(skills.path().join("review/SKILL.md"),
        "---\nname: review\ndescription: Review Rust functions carefully.\n---\nCheck error paths.\n")?;
    let agent = runtime.agent(AgentSpec::new("skilled").skills_dir(skills.path()))?;
    let copied = agent
        .workspace_dir()
        .join("agents/skilled/skills/review/SKILL.md");
    assert_eq!(
        std::fs::read_to_string(&copied)?,
        std::fs::read_to_string(skills.path().join("review/SKILL.md"))?
    );
    assert!(!std::fs::symlink_metadata(copied)?.file_type().is_symlink());
    assert!(!agent.run("Hello").await?.reply.is_empty());
    println!("skill bundle copied without symlinks");
    // ANCHOR_END: skills
    support::passed("skills");
    Ok(())
}
