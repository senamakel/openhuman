//! Clearing a channel's memory chunks on disconnect.

use crate::config::Config;
use tinymemory_api::chunks::SourceKind;
use tinymemory_api::provider::ForgetSelector;

/// Drop everything this channel put into memory, and report how many chunks
/// went.
///
/// Two selectors because a channel files content under two shapes: the bare
/// `channel_id` (the channel itself) and `channel_id:<conversation>` (each
/// thread inside it). [`ForgetSelector::Source`] is exact by contract, so the
/// first would leave every per-conversation source behind on its own; the
/// prefix arm is matched literally, so a provider id containing `%` or `_`
/// means itself.
///
/// No `spawn_blocking`: the driver owns whether its own reads and writes
/// block, and the module's do not run on this thread at all.
pub(super) async fn clear_channel_memory(
    config: &Config,
    channel_id: &str,
) -> anyhow::Result<usize> {
    let kind = SourceKind::Chat.as_str().to_string();
    let exact = forget_matching(
        config,
        &ForgetSelector::Source {
            source_kind: kind.clone(),
            source_id: channel_id.to_string(),
        },
    )
    .await?;
    let prefixed = forget_matching(
        config,
        &ForgetSelector::SourcePrefix {
            source_kind: kind,
            source_id_prefix: format!("{channel_id}:"),
        },
    )
    .await?;
    Ok(exact.saturating_add(prefixed))
}

/// Run one [`ForgetSelector`] through the bound memory driver and return the
/// chunk count it removed.
///
/// A driver without the `Sources` family is **refused**, not degraded to zero.
/// The read paths elsewhere answer empty for a missing family because "this
/// driver holds nothing" is a true answer to what they were asked; this is a
/// delete, and its only empty answer — zero chunks removed — is
/// byte-identical to a successful delete of nothing. The caller renders that
/// number as `memory_chunks_deleted` in the disconnect reply, so degrading
/// here would tell a user their channel's history was cleared when it is
/// still on disk.
///
/// `trees_cleaned` is dropped on purpose: `memory_chunks_deleted` has always
/// been a chunk count, and the disconnect reply's shape does not change here.
async fn forget_matching(config: &Config, selector: &ForgetSelector) -> anyhow::Result<usize> {
    let binding = crate::memory::binding::for_config(config)
        .map_err(|e| anyhow::anyhow!("forget_matching: {e}"))?;
    let Some(sources) = binding.provider().as_sources() else {
        return Err(anyhow::anyhow!(
            "forget_matching: driver '{}' does not serve Sources",
            binding.driver_id()
        ));
    };
    let outcome = sources
        .forget_matching(selector)
        .await
        .map_err(|e| anyhow::anyhow!("forget_matching: {e}"))?;
    log::debug!(
        "[channels][memory] forget_matching removed chunks={} trees={} (driver='{}')",
        outcome.chunks_removed,
        outcome.trees_cleaned,
        binding.driver_id()
    );
    Ok(usize::try_from(outcome.chunks_removed).unwrap_or(usize::MAX))
}
