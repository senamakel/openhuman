//! Two independently configured agents on one runtime.
//!
//! The two-step library API: initialise a [`Runtime`] once, then instantiate
//! agents on it. Here a read-only *reviewer* and a full-autonomy *fixer*
//! share a runtime but nothing else — each has its own access tier, working
//! directory, model and (optionally) skills.
//!
//! ```bash
//! # BYOK: both agents run on the endpoint you name.
//! OPENHUMAN_EXAMPLE_BASE_URL=https://api.openai.com/v1 \
//! OPENHUMAN_EXAMPLE_API_KEY=sk-… \
//! OPENHUMAN_EXAMPLE_MODEL=gpt-5 \
//!   cargo run -p openhuman-embed --example two_agents -- "Describe this directory."
//!
//! # Managed: the runtime holds a TinyHumans API key and agents that name no
//! # provider run on the managed backend.
//! OPENHUMAN_EXAMPLE_TINYHUMANS_API_KEY=th_… \
//!   cargo run -p openhuman-embed --example two_agents -- "Describe this directory."
//! ```
//!
//! Optional: `OPENHUMAN_EXAMPLE_BACKEND_URL` for non-inference backend calls,
//! `OPENHUMAN_EXAMPLE_SKILLS_DIR` for skill bundles given to the reviewer only.

use std::path::PathBuf;

use openhuman_core::core::runtime::{AGENT_WORKER_STACK_BYTES, MAX_BLOCKING_THREADS};
use openhuman_embed::{Access, AgentSpec, Provider, Runtime, Workspace};

fn main() -> anyhow::Result<()> {
    let _ = env_logger::builder().is_test(false).try_init();

    // See `run_turn.rs` for why the runtime is built by hand: a turn's async
    // state machine overflows tokio's default worker stack.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(AGENT_WORKER_STACK_BYTES)
        .max_blocking_threads(MAX_BLOCKING_THREADS)
        .build()?;

    runtime.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "Describe this directory in one paragraph.".to_string());

    // ── step 1: the runtime ──────────────────────────────────────────────
    let mut builder = Runtime::builder().workspace(Workspace::Ephemeral);
    if let Ok(key) = std::env::var("OPENHUMAN_EXAMPLE_TINYHUMANS_API_KEY") {
        builder = builder.api_key(key);
    }
    if let Ok(url) = std::env::var("OPENHUMAN_EXAMPLE_BACKEND_URL") {
        builder = builder.backend_url(url);
    }
    let runtime = builder.build().await?;
    println!("runtime root: {}", runtime.root_dir().display());

    // A BYOK route when the caller named one; otherwise the agents inherit
    // the runtime's default (managed inference on the API key).
    let provider = match (
        std::env::var("OPENHUMAN_EXAMPLE_BASE_URL"),
        std::env::var("OPENHUMAN_EXAMPLE_API_KEY"),
    ) {
        (Ok(base_url), Ok(api_key)) => {
            let model = std::env::var("OPENHUMAN_EXAMPLE_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string());
            Provider::openai_compatible(base_url, api_key).model(model)
        }
        _ if runtime.has_api_key() => Provider::inherit(),
        _ => anyhow::bail!(
            "set OPENHUMAN_EXAMPLE_BASE_URL + OPENHUMAN_EXAMPLE_API_KEY for BYOK, or \
             OPENHUMAN_EXAMPLE_TINYHUMANS_API_KEY for managed inference"
        ),
    };

    // ── step 2: the agents ───────────────────────────────────────────────
    let cwd = std::env::current_dir()?;

    #[cfg_attr(not(feature = "skills"), allow(unused_mut))]
    let mut reviewer = AgentSpec::new("reviewer")
        .system_prompt("You are a careful code reviewer. You read; you never change files.")
        .provider(provider.clone())
        .access(Access::readonly())
        .action_dir(&cwd);
    if let Some(dir) = std::env::var_os("OPENHUMAN_EXAMPLE_SKILLS_DIR") {
        #[cfg(feature = "skills")]
        {
            reviewer = reviewer.skills_dir(PathBuf::from(dir));
        }
        #[cfg(not(feature = "skills"))]
        {
            let _ = PathBuf::from(dir);
            eprintln!("this build has no `skills` feature; ignoring the skills directory");
        }
    }
    let reviewer = runtime.agent(reviewer)?;

    // `Access::full()` means real shell commands and real file edits under
    // `action_dir`. It is pointed at a scratch directory here on purpose.
    let scratch = runtime.root_dir().join("fixer-scratch");
    std::fs::create_dir_all(&scratch)?;
    let fixer = runtime.agent(
        AgentSpec::new("fixer")
            .system_prompt("You act on review findings inside your working directory.")
            .provider(provider)
            .access(Access::full())
            .action_dir(&scratch),
    )?;

    println!("agents: {:?}", runtime.agent_ids());
    println!("reviewer action dir: {}", reviewer.action_dir().display());
    println!("fixer action dir:    {}", fixer.action_dir().display());

    let review = reviewer.run(&prompt).await?;
    println!(
        "\n[reviewer] session {}\n{}",
        review.session_id, review.reply
    );

    let fix = fixer
        .turn(format!(
            "A reviewer said:\n\n{}\n\nWrite a short NOTES.md in your working directory summarising it.",
            review.reply
        ))
        .send()
        .await?;
    println!("\n[fixer] session {}\n{}", fix.session_id, fix.reply);

    Ok(())
}
