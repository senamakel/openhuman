//! Which memory-sources registry row a Composio connection belongs to.
//!
//! One matching rule with more than one reader: the pass's sync-depth cap and
//! the history row's source id both find the row this way (openhuman#6257).
//! Call sites that each open-code one row rule are how two of them came to
//! disagree before (openhuman#6007).

use crate::config::Config;
use crate::memory::sources::SourceKind;

/// Whether a registry row's toolkit and connection name this connection.
///
/// Matched the way the engine keys the rows: toolkit case-insensitively and
/// trimmed, connection trimmed.
fn names_connection(
    row_toolkit: Option<&str>,
    row_connection: Option<&str>,
    toolkit: &str,
    connection_id: &str,
) -> bool {
    row_toolkit.is_some_and(|slug| slug.trim().eq_ignore_ascii_case(toolkit.trim()))
        && row_connection.is_some_and(|id| id.trim() == connection_id.trim())
}

/// The per-source "Sync depth (days)" cap for one connection, from the
/// memory-sources registry the pass's own `config` names.
///
/// Resolved here rather than threaded through every caller because there are
/// five of them (the row button, All In, the periodic loop, the connection
/// bootstrap, Slack's own RPC), and `max_items` already showed what happens
/// when each open-codes the same rule: two of the five disagreed
/// (openhuman#6007). Read through `config`, not the process environment: a
/// pass is bound to one workspace, and the global registry path would answer a
/// caller bound to workspace B with workspace A's rows — the cross-workspace
/// leak the registry's `_in` variants exist to prevent. `None` when the row is
/// missing, carries no cap, or the registry cannot be read — each means "no
/// lower bound", which is what every release before the field existed did, so
/// a registry hiccup degrades to the old behaviour rather than to a failed
/// sync.
pub(super) fn source_sync_depth_days(
    config: &Config,
    toolkit: &str,
    connection_id: &str,
) -> Option<u32> {
    let sources = match crate::memory::sources::registry::list_sources_in(config) {
        Ok(sources) => sources,
        Err(error) => {
            tracing::warn!(
                toolkit = %toolkit,
                connection_id = %connection_id,
                error = %error,
                "[composio] memory-sources registry unreadable for the sync depth; \
                 syncing without a lower bound"
            );
            return None;
        }
    };
    pick_source_sync_depth_days(
        sources
            .iter()
            .filter(|source| source.kind == SourceKind::Composio)
            .map(|source| {
                (
                    source.toolkit.as_deref(),
                    source.connection_id.as_deref(),
                    source.sync_depth_days,
                )
            }),
        toolkit,
        connection_id,
    )
}

/// The cap of the row matching `toolkit` and `connection_id`, if any.
///
/// Matched by [`names_connection`], and a cap of zero reads as none: the
/// settings field stores "unlimited" as an empty value, and a zero typed by
/// hand would otherwise ask Gmail for mail newer than today.
pub(crate) fn pick_source_sync_depth_days<'a>(
    rows: impl IntoIterator<Item = (Option<&'a str>, Option<&'a str>, Option<u32>)>,
    toolkit: &str,
    connection_id: &str,
) -> Option<u32> {
    rows.into_iter()
        .find_map(|(row_toolkit, row_connection, depth)| {
            names_connection(row_toolkit, row_connection, toolkit, connection_id)
                .then_some(depth)
                .flatten()
                .filter(|days| *days > 0)
        })
}

/// The registry id of the Composio row for this connection, if the pass's
/// workspace has one.
///
/// `None` when no row matches or the registry cannot be read. A history row
/// then names the connection's scope instead, which still identifies the run
/// it describes, so neither case is worth failing a recording over.
pub(super) fn source_id_for_connection(
    config: &Config,
    toolkit: &str,
    connection_id: &str,
) -> Option<String> {
    let sources = match crate::memory::sources::registry::list_sources_in(config) {
        Ok(sources) => sources,
        Err(error) => {
            tracing::warn!(
                toolkit = %toolkit,
                connection_id = %connection_id,
                error = %error,
                "[composio] memory-sources registry unreadable; the run's history row \
                 names its scope instead of a source"
            );
            return None;
        }
    };
    sources
        .into_iter()
        .find(|source| {
            source.kind == SourceKind::Composio
                && names_connection(
                    source.toolkit.as_deref(),
                    source.connection_id.as_deref(),
                    toolkit,
                    connection_id,
                )
        })
        .map(|source| source.id)
}

#[cfg(test)]
#[path = "source_rows_tests.rs"]
mod tests;
