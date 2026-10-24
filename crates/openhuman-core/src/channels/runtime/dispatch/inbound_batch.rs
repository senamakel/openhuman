//! Channel messages as [`crate::inbound_debounce`] items, and the merge that
//! turns one debounced batch into the single message its turn runs on.

use super::processor::AdmittedInbound;
use crate::inbound_debounce::DebounceItem;
use tinychannels_bus::channel::ContentPart;

const MEDIA_MARKERS: [&str; 5] = ["[IMAGE:", "[FILE:", "[VIDEO:", "[DOCUMENT:", "[Attachment:"];

/// An admitted message with its receive order on the dispatch queue.
#[derive(Debug, Clone)]
pub(crate) struct QueuedInbound {
    pub(crate) seq: u64,
    pub(crate) inbound: AdmittedInbound,
}

impl DebounceItem for QueuedInbound {
    fn chars(&self) -> usize {
        self.inbound.msg.text.chars().count()
    }

    fn carries_media(&self) -> bool {
        let content = &self.inbound.msg.text;
        MEDIA_MARKERS.iter().any(|marker| content.contains(marker))
    }

    fn mergeable(&self) -> bool {
        let trimmed = self.inbound.msg.text.trim_start();
        !trimmed.is_empty() && !trimmed.starts_with('/')
    }
}

/// Merge a batch into one message: ordered by (timestamp, receive order),
/// addressed like the newest message, with the non-blank bodies joined by
/// newlines.
pub(crate) fn merge_channel_batch(mut batch: Vec<QueuedInbound>) -> Option<AdmittedInbound> {
    batch.sort_by_key(|item| {
        (
            item.inbound.msg.timestamp_unix_ms.unwrap_or_default(),
            item.seq,
        )
    });
    if batch.len() <= 1 {
        return batch.pop().map(|item| item.inbound);
    }
    let sender_name = batch
        .iter()
        .rev()
        .find_map(|item| item.inbound.msg.sender.name.clone());
    let content = batch
        .iter()
        .map(|item| item.inbound.msg.text.as_str())
        .filter(|content| !content.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let mut parts = Vec::new();
    let mut has_content = false;
    for item in &batch {
        if item.inbound.msg.text.trim().is_empty() {
            continue;
        }
        if has_content {
            parts.push(ContentPart::Text {
                text: "\n".into(),
                markdown_dialect: None,
            });
        }
        parts.extend(item.inbound.msg.parts.iter().cloned());
        has_content = true;
    }
    let count = batch.len();
    let mut merged = batch.pop()?.inbound;
    merged.msg.text = content;
    merged.msg.parts = parts;
    merged.msg.sender.name = sender_name;
    tracing::debug!(
        channel = %merged.msg.channel.id,
        message_id = %merged.msg.message_id,
        merged = count,
        "[dispatch][debounce] merged inbound batch"
    );
    Some(merged)
}

#[cfg(test)]
#[path = "inbound_batch_tests.rs"]
mod tests;
