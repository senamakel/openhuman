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
    let p = compose_current_prompt("j1", "water", "drink water", "user: hi");
    assert!(p.starts_with("[cron:j1 water] "));
    assert!(p.contains(UNATTENDED_PREAMBLE));
    assert!(p.contains("NO_REPLY"));
    assert!(p.contains("user: hi"));
    assert!(p.ends_with("Scheduled task: drink water"));
    let bare = compose_current_prompt("j1", "water", "drink water", "");
    assert!(!bare.contains("Recent conversation"));
}
