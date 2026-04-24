//! Read ops: list_labels, list_messages, search, get_message.
//!
//! Strategy per-op:
//!
//! * **list_labels** — DOM snapshot of the sidebar. Cheap and reliable:
//!   Gmail renders labels as `<a role="link" aria-label="…">` inside the
//!   left nav. We read that tree directly via
//!   `DOMSnapshot.captureSnapshot` — no JS eval, no network round-trip.
//! * **list_messages / search / get_message** — scaffolded with
//!   structured errors for the first cut. These depend on either
//!   intercepting Gmail's internal batch endpoints via `Network.*` or a
//!   broader DOM walk; both need careful follow-up work to stay stable
//!   across Gmail UI churn. See plan §deferred.
//!
//! Everything here is CEF-only — CDP requires a remote-debugging port
//! which wry doesn't expose.

use super::session;
use super::types::{GmailLabel, GmailMessage};
use crate::cdp::Snapshot;

pub async fn list_labels(account_id: &str) -> Result<Vec<GmailLabel>, String> {
    log::debug!("[gmail][{account_id}] list_labels");
    let (mut cdp, session_id) = session::attach(account_id).await?;
    let snap = match Snapshot::capture(&mut cdp, &session_id).await {
        Ok(s) => s,
        Err(e) => {
            session::detach(&mut cdp, &session_id).await;
            return Err(format!("gmail[{account_id}]: snapshot failed: {e}"));
        }
    };
    let labels = scrape_labels(&snap);
    session::detach(&mut cdp, &session_id).await;
    log::debug!(
        "[gmail][{account_id}] list_labels ok count={}",
        labels.len()
    );
    Ok(labels)
}

pub async fn list_messages(
    account_id: &str,
    _limit: u32,
    _label: Option<String>,
) -> Result<Vec<GmailMessage>, String> {
    log::debug!("[gmail][{account_id}] list_messages (not implemented)");
    Err(format!(
        "gmail[{account_id}]: list_messages not implemented — follow-up work \
         per plan §deferred (Network MITM of mail.google.com sync endpoint)"
    ))
}

pub async fn search(
    account_id: &str,
    _query: String,
    _limit: u32,
) -> Result<Vec<GmailMessage>, String> {
    log::debug!("[gmail][{account_id}] search (not implemented)");
    Err(format!(
        "gmail[{account_id}]: search not implemented — follow-up work"
    ))
}

pub async fn get_message(account_id: &str, _message_id: String) -> Result<GmailMessage, String> {
    log::debug!("[gmail][{account_id}] get_message (not implemented)");
    Err(format!(
        "gmail[{account_id}]: get_message not implemented — follow-up work"
    ))
}

// ── label scrape ────────────────────────────────────────────────────────

/// Gmail's sidebar labels render as `<a>` or `<div>` with
/// `role="link"` and an `aria-label` attribute containing the label
/// name (and sometimes the unread count). We walk every such node in
/// the snapshot and dedupe by name.
///
/// System labels come in with upper-case English names (Inbox, Sent,
/// Drafts, Spam, Trash, Starred, Important, Snoozed, Scheduled,
/// All Mail, Chats, Categories). Anything else is assumed user-created.
fn scrape_labels(snap: &Snapshot) -> Vec<GmailLabel> {
    let mut out: Vec<GmailLabel> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let link_nodes = snap.find_all(|s, idx| {
        if !s.is_element(idx) {
            return false;
        }
        // Gmail sidebar items are anchors (`<a>`) or `<div role="link">`.
        let tag = s.tag(idx);
        if tag != "A" && tag != "a" && tag != "DIV" && tag != "div" {
            return false;
        }
        matches!(s.attr(idx, "role"), Some("link"))
    });

    for idx in link_nodes {
        let aria = match snap.attr(idx, "aria-label") {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let (name, unread) = parse_aria_label(aria);
        if name.is_empty() {
            continue;
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        let kind = if is_system_label(&name) {
            "system"
        } else {
            "user"
        };
        out.push(GmailLabel {
            id: name.clone(),
            name,
            kind: kind.to_string(),
            unread,
        });
    }
    out
}

/// Gmail's aria-labels look like:
///   `"Inbox 23 unread"`, `"Inbox, 23 unread messages"`,
///   `"Starred"`, `"Drafts 4"`, `"Spam, 1"`.
/// Peel any trailing `N unread(messages)?` / `N` count off and return
/// the plain label name plus the parsed unread count if present.
fn parse_aria_label(aria: &str) -> (String, Option<u64>) {
    let mut name = aria.trim().to_string();

    // 1. Strip English descriptors in order from most-specific to least.
    //    Keep going until no more of these match, which covers labels
    //    like "Spam, 1 unread messages" that chain two suffixes.
    loop {
        let lower = name.to_ascii_lowercase();
        let stripped_len = ["unread messages", "unread", "messages"]
            .iter()
            .find(|suf| lower.ends_with(*suf))
            .map(|suf| name.len() - suf.len());
        match stripped_len {
            Some(n) => {
                name.truncate(n);
                name = name.trim_end_matches([' ', ',']).to_string();
            }
            None => break,
        }
    }

    // 2. Now name is e.g. "Inbox 23" or "Spam, 1" or "Starred". Peel off
    //    a trailing integer (with any comma/space separator) as the
    //    unread count.
    let mut unread: Option<u64> = None;
    if let Some(last) = name.split(|c: char| c == ' ' || c == ',').next_back() {
        if !last.is_empty() {
            if let Ok(n) = last.parse::<u64>() {
                unread = Some(n);
                let cut = name.len() - last.len();
                name.truncate(cut);
                name = name.trim_end_matches([' ', ',']).to_string();
            }
        }
    }

    (name.trim().to_string(), unread)
}

fn is_system_label(name: &str) -> bool {
    matches!(
        name,
        "Inbox"
            | "Starred"
            | "Snoozed"
            | "Important"
            | "Sent"
            | "Drafts"
            | "Scheduled"
            | "All Mail"
            | "Spam"
            | "Trash"
            | "Chats"
            | "Categories"
            | "Updates"
            | "Promotions"
            | "Social"
            | "Forums"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aria_label_peels_trailing_count() {
        assert_eq!(
            parse_aria_label("Inbox 23 unread"),
            ("Inbox".into(), Some(23))
        );
        assert_eq!(parse_aria_label("Drafts 4"), ("Drafts".into(), Some(4)));
        assert_eq!(parse_aria_label("Starred"), ("Starred".into(), None));
        assert_eq!(
            parse_aria_label("Spam, 1 unread messages"),
            ("Spam".into(), Some(1))
        );
    }

    #[test]
    fn system_label_catalog_matches_known_names() {
        for n in ["Inbox", "Sent", "Drafts", "Trash", "Spam", "Starred"] {
            assert!(is_system_label(n), "expected system: {n}");
        }
        assert!(!is_system_label("Receipts"));
        assert!(!is_system_label("Personal/Finance"));
    }
}
