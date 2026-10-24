use super::*;

fn m(role: &str, text: &str) -> (String, String) {
    (role.to_string(), text.to_string())
}

#[test]
fn tail_keeps_the_newest_messages_in_order() {
    let msgs: Vec<_> = (0..15)
        .map(|i| {
            m(
                if i % 2 == 0 { "user" } else { "assistant" },
                &format!("msg{i}"),
            )
        })
        .collect();
    let tail = bounded_tail(&msgs, 10, 1400);
    let lines: Vec<_> = tail.lines().collect();
    assert_eq!(lines.len(), 10);
    assert_eq!(lines[0], "assistant: msg5");
    assert_eq!(lines[9], "user: msg14");
}

#[test]
fn tail_is_bounded_by_characters_and_keeps_the_newest() {
    let long = "x".repeat(1000);
    let msgs = vec![
        m("user", "old one"),
        m("user", &long),
        m("assistant", &long),
    ];
    let tail = bounded_tail(&msgs, 10, 1400);
    assert!(tail.chars().count() <= 1400, "len {}", tail.chars().count());
    assert!(tail.ends_with(&"x".repeat(100)));
    assert!(tail.lines().last().unwrap().starts_with("assistant: "));
    assert!(!tail.contains("old one"));
}

#[test]
fn tail_skips_empty_and_non_chat_roles() {
    let msgs = vec![m("system", "secret"), m("user", "  "), m("user", "hi")];
    assert_eq!(bounded_tail(&msgs, 10, 1400), "user: hi");
    assert_eq!(bounded_tail(&[], 10, 1400), "");
}

#[test]
fn current_prompt_has_preamble_context_and_task() {
    let p = compose_current_prompt("j1", "water", "drink water", "user: hi", false);
    assert!(p.starts_with("[cron:j1 water] "));
    assert!(p.contains(DELIVERING_PREAMBLE));
    assert!(p.contains("user: hi"));
    assert!(p.ends_with("Scheduled task: drink water"));
    let bare = compose_current_prompt("j1", "water", "drink water", "", false);
    assert!(!bare.contains("Recent conversation"));
}

#[test]
fn only_the_conditional_preamble_offers_no_reply() {
    assert!(!DELIVERING_PREAMBLE.contains("NO_REPLY"));
    assert!(CONDITIONAL_PREAMBLE.contains("NO_REPLY"));
    let delivering = compose_current_prompt("j1", "water", "drink water", "", false);
    assert!(!delivering.contains("NO_REPLY"));
    let conditional =
        compose_current_prompt("j1", "inbox", "reply NO_REPLY if nothing changed", "", true);
    assert!(conditional.contains(CONDITIONAL_PREAMBLE));
}

fn current_job(prompt: &str) -> CronJob {
    use tinyflows_bus::schedule::types::{DeliveryConfig, JobType, Schedule, SessionTarget};
    CronJob {
        claim_generation: 0,
        policy: Default::default(),
        id: "job-1".into(),
        expression: "*/5 * * * *".into(),
        schedule: Schedule::Every { every_ms: 300_000 },
        command: String::new(),
        prompt: Some(prompt.into()),
        name: Some("task".into()),
        job_type: JobType::Agent,
        session_target: SessionTarget::Current,
        model: None,
        agent_id: None,
        enabled: true,
        delivery: DeliveryConfig {
            mode: "origin".into(),
            channel: None,
            to: None,
            best_effort: true,
        },
        delete_after_run: false,
        created_at: chrono::Utc::now(),
        next_run: chrono::Utc::now(),
        last_run: None,
        last_status: None,
        last_output: None,
        origin: None,
    }
}

#[tokio::test]
async fn a_reminder_runs_with_the_delivering_preamble() {
    let tmp = tempfile::TempDir::new().unwrap();
    let config = Config {
        workspace_dir: tmp.path().to_path_buf(),
        ..Config::default()
    };
    let ctx = DispatchCtx::ambient();
    let reminder = build_run_prompt(
        &ctx,
        &config,
        &current_job("Remind the user to drink water"),
        "task",
    )
    .await;
    assert!(reminder.contains(DELIVERING_PREAMBLE), "{reminder}");
    assert!(!reminder.contains("NO_REPLY"), "{reminder}");

    let check = build_run_prompt(
        &ctx,
        &config,
        &current_job("Check the inbox; reply NO_REPLY if nothing changed"),
        "task",
    )
    .await;
    assert!(check.contains(CONDITIONAL_PREAMBLE), "{check}");
}
