---
description: "An Agent combines one identity, derived context and independently configured tool/model behavior on a shared Runtime."
---

# Agents

Register an `AgentSpec` with `Runtime::agent`. Its ID selects the agent's home, durable session identity and addressed cron/channel work. An `Agent` clone refers to the same instance; it does not copy state or start another core. Runtime-wide credentials remain shared, so two agents are not two authenticated customers.

Keep `action_dir` separate from the internal workspace. The action directory is where tools act; the agent home stores internal transcripts, skills and other state. The example gives a reviewer and a fixer distinct folders and prompts while sharing one provider connection setup.

## Runtime and agent scope

| Runtime responsibility | Agent or call responsibility |
| --- | --- |
| Core, shared configuration path and registry capacity | ID, context, prompt, action directory, provider and per-agent stores |

## Verified example

<!-- BEGIN EMBED: crates/openhuman-embed/examples/two_agents.rs#two_agents -->

```rust
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
```

<!-- END EMBED -->

[Complete runnable example](https://github.com/tinyhumansai/openhuman/blob/main/crates/openhuman-embed/examples/two_agents.rs). The full file includes imports, runtime setup and local fixtures used by this excerpt.

## Behavior to account for

IDs stay reserved until teardown completes, including while the last handle's destructor is clearing state. `remove_agent(id).await` marks retained handles removed, cancels active turns, denies parked approvals and releases the ID after cleanup. Add `.purge()` only when the agent's persisted home should also be deleted. Cancelling a started removal still performs teardown and a requested purge.

An old polling handle or approval callback never follows a reused ID to its replacement. Active host tools cancel cooperatively and may finish their external work after removal; the host still owns those external lifetimes. See [turns and sessions](turns-sessions.md) and [access and approvals](access-approvals.md).
