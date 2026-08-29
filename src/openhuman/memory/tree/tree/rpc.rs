//! RPC handler functions for the memory tree layer.
//!
//! Public JSON-RPC surface:
//! - `openhuman.memory_tree_ingest` — one unified ingest. Caller supplies
//!   `source_kind` + generic JSON `payload` (adapter-specific). Chat and
//!   document are canonicalised into contract items and handed to the bound
//!   driver's `Ingest` family; mail still canonicalises in process, for the
//!   reasons on [`ingest_rpc`].
//! - `openhuman.memory_tree_list_chunks` — listing with filters.
//! - `openhuman.memory_tree_get_chunk` — single chunk fetch.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::openhuman::config::Config;
use crate::openhuman::memory::api::provider::ChunkQuery;
use crate::rpc::RpcOutcome;
use tinycortex::memory::ingest::canonicalize::{
    chat::ChatBatch, document::DocumentInput, email::EmailThread,
};
use tinymemory_api::chunks::{Chunk, DataSource, SourceKind, SourceRef};
use tinymemory_api::provider::types::{IngestItem, IngestOutcome};
use tinymemory_api::types::MemoryTaint;

/// Unified ingest request. The `payload` shape is adapter-specific and is
/// validated inside the dispatch based on `source_kind`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IngestRequest {
    /// Which kind of source the payload represents.
    pub source_kind: SourceKind,
    /// Logical source id (channel/group for chat, thread for email, doc id).
    pub source_id: String,
    /// Account/user this content belongs to.
    #[serde(default)]
    pub owner: String,
    /// Optional labels/tags carried through.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Adapter-specific payload — shape matches the canonicaliser for
    /// `source_kind`:
    /// - `chat`     → [`ChatBatch`]
    /// - `email`    → [`EmailThread`]
    /// - `document` → [`DocumentInput`]
    pub payload: Value,
}

/// Response body of the `memory_tree_ingest` RPC.
///
/// Declared here rather than returned as the engine's own summary type,
/// because this is a wire shape the frontend reads: a body owned by a foreign
/// crate is one an upstream field rename can reshape without anything in this
/// repository failing to compile. Every key and JSON type is what that summary
/// serialised and must stay that way —
/// `the_response_body_serialises_exactly_as_the_engine_summary` is the pin, and
/// it is what the chat and document arms' move onto the driver contract had to
/// keep true. Those two build this body from an `IngestOutcome` now, mail still
/// from the pipeline's summary, and both spell the same six keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IngestResponse {
    /// Logical source id the ingest was scoped to — the one the caller
    /// supplied, echoed back so a batched caller can pair a reply with its
    /// request.
    pub source_id: String,
    /// Units persisted by this call.
    pub chunks_written: usize,
    /// Units produced and not admitted. Dropped units only: a call refused
    /// outright is [`Self::already_ingested`], not a drop of everything.
    pub chunks_dropped: usize,
    /// Ids of the units this call produced. A caller fetches a chunk back by
    /// these, so a write that names none is unusable even when the count is
    /// right.
    pub chunk_ids: Vec<String>,
    /// Follow-up extraction jobs this call scheduled. Read next to
    /// [`Self::chunks_written`] it answers whether the material just handed
    /// over will be picked up at all — rows can land with nothing scheduled to
    /// derive from them, and the write count alone reports that as success.
    pub extract_jobs_enqueued: usize,
    /// True when the call was a no-op because `(source_kind, source_id)` had
    /// been ingested before.
    ///
    /// Distinct from a zero-write result, and the distinction is the point:
    /// only a refusal is a reason to go and clear the source gate. The gate is
    /// keyed on the logical source rather than on the content, so re-sending
    /// *changed* material under a claimed `source_id` also writes nothing.
    pub already_ingested: bool,
}

/// Build the validation error returned when an ingest payload does not match
/// the canonicaliser schema for its `source_kind`.
///
/// Kept as the single construction site so the wording cannot drift away from
/// [`is_invalid_ingest_payload_message`], which the transport layer uses to
/// pick the Sentry severity. Same emit-site/classifier pairing as
/// `dispatch::UNKNOWN_METHOD_PREFIX` / `dispatch::unknown_method_name`.
fn invalid_payload_message(source_kind: SourceKind, err: &serde_json::Error) -> String {
    format!("invalid {} payload: {err}", source_kind.as_str())
}

/// Returns `true` when `message` is an ingest-payload schema-validation
/// failure produced by `invalid_payload_message`.
///
/// Such a failure is a **caller** error — the submitted JSON does not match
/// the canonicaliser's shape — not a core defect. The handler already returns
/// a precise, actionable JSON-RPC error naming the offending field, and no
/// core-side change can fix a producer that sends the wrong shape. Reporting
/// it at Sentry *error* severity therefore pages on someone else's payload
/// bug: #5169 (`CORE-RUST-1P0`) was 14 such events for a chat batch whose
/// messages omitted `timestamp`.
///
/// The transport layer demotes these to a warn-level capture — still recorded
/// for triage, because a spike genuinely means a producer regressed, but not
/// an error event. See `core::jsonrpc::rpc_handler`.
///
/// Anchored on the exact `invalid <kind> payload: ` prefix rather than a
/// loose `"invalid"` substring so unrelated failures keep paging.
pub fn is_invalid_ingest_payload_message(message: &str) -> bool {
    let Some(rest) = message.strip_prefix("invalid ") else {
        return false;
    };
    // Enumerated rather than parsed so a new `SourceKind` that forgets to
    // update this list stays *loud* (keeps paging) instead of silently
    // inheriting the demotion. The `all_source_kinds_are_recognised_*` test
    // below pins that every variant reachable from `ingest_rpc` is covered.
    [SourceKind::Chat, SourceKind::Email, SourceKind::Document]
        .iter()
        .any(|k| rest.starts_with(&format!("{} payload: ", k.as_str())))
}

/// Which `Ingest` member a canonicalised request lands on.
///
/// One type rather than two resolve sites, so the family lookup and its
/// refusal wording exist once: the arms differ only in which member they call.
enum DriverIngest {
    Chat(Vec<IngestItem>),
    Email(Vec<IngestItem>),
    Document(IngestItem),
}

/// Hand canonicalised items to the bound driver's `Ingest` family.
///
/// A missing family is **refused**, not degraded. The read handlers below
/// answer empty for one because "this driver stores no chunks" is a true
/// answer to what they were asked; this is a write, and the only empty answer
/// available to it — zero written, zero dropped — is byte-identical to a
/// successful ingest of nothing, so the content would go on the floor while
/// the caller was told it landed.
///
/// Resolved on `provider()`, the same seam [`store_stats`] and [`queue_stats`]
/// use, and deliberately not on `binding.guard()`: the guard's `Ingest`
/// decorator re-stamps `taint`, and `ExternalSync` — what it stamps whenever a
/// source scope is active — is exactly what the driver's own
/// `validate_ingest_item` refuses for the chunk tier, so routing this path
/// through the guard would fail every ingest issued during a sync.
async fn ingest_through_driver(
    config: &Config,
    call: DriverIngest,
) -> Result<IngestOutcome, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let Some(ingest) = binding.provider().as_ingest() else {
        return Err(format!(
            "ingest: driver '{}' does not serve Ingest",
            binding.driver_id()
        ));
    };
    match call {
        DriverIngest::Chat(messages) => ingest.ingest_chat(messages).await,
        DriverIngest::Email(messages) => ingest.ingest_email(messages).await,
        DriverIngest::Document(item) => ingest.ingest_document(item).await,
    }
    .map_err(|e| format!("ingest: {e}"))
}

/// The [`DataSource`] a chat payload's `platform` string names.
///
/// The verbatim string still crosses as `IngestItem::platform`, which is what
/// the driver rebuilds `ChatBatch::platform` from, so this enum only has to
/// agree with the arm it came from. An unrecognised platform is `Conversation`
/// — the generic chat member — rather than a rejection: this RPC has always
/// taken any platform string, and a new integration must not start failing at
/// the seam because its name is not in an enum here.
fn chat_data_source(platform: &str) -> DataSource {
    match DataSource::parse(platform) {
        Ok(source) if source.kind() == SourceKind::Chat => source,
        _ => DataSource::Conversation,
    }
}

/// As [`chat_data_source`], for a mail payload's `provider`.
///
/// `OtherEmail` is the fallback because it is the mail member that means "an
/// email provider this enum does not name", which is the honest reading of a
/// provider string that did not parse. Demoting to a non-mail member would put
/// the thread under the wrong `SourceKind` entirely.
fn email_data_source(provider: &str) -> DataSource {
    match DataSource::parse(provider) {
        Ok(source) if source.kind() == SourceKind::Email => source,
        _ => DataSource::OtherEmail,
    }
}

/// Canonicalise a mail thread into contract items.
///
/// **This is the exact inverse of the driver's reconstruction**, and it has to
/// be: the driver rebuilds an `EmailThread` from these fields and then calls
/// the same `ingest_pipeline::ingest_email` this arm used to call directly, so
/// any field that does not round-trip changes what is stored rather than
/// failing. Every one it reads through `unwrap_or` is therefore sent as
/// `Some`, so no fallback on the far side can substitute a different value:
///
/// | driver reads | sent here |
/// | --- | --- |
/// | `platform.unwrap_or(source)` | `platform: Some(provider)` |
/// | `channel_label.unwrap_or(source_id)` | `channel_label: Some(thread_subject)` |
/// | `author.unwrap_or(owner)` | `author: Some(from)` |
/// | `subject.unwrap_or(thread_subject)` | `subject: Some(subject)` |
/// | `timestamp.unwrap_or(now)` | `timestamp: Some(sent_at)` |
///
/// `to`, `cc` and `list_unsubscribe` cross verbatim. The unsubscribe header
/// matters more than it looks: an unsubscribe flow reads it back out of stored
/// mail, so dropping it makes that flow impossible rather than merely poorer.
///
/// # Empty bodies are filtered, not refused
///
/// `validate_ingest_item` answers `Invalid` for empty content, and the driver
/// validates every item before ingesting any — so one body-less message would
/// fail the whole thread, where the in-process pipeline wrote the rest of it.
/// A header-only message is entirely plausible in mail, so filtering first
/// (exactly as [`chat_items`] does) keeps the old behaviour instead of turning
/// a survivable thread into a rejected one.
fn email_items(
    source_id: &str,
    owner: &str,
    tags: &[String],
    thread: EmailThread,
) -> Vec<IngestItem> {
    let EmailThread {
        provider,
        thread_subject,
        messages,
    } = thread;
    let source = email_data_source(&provider);
    messages
        .into_iter()
        .filter(|message| !message.body.trim().is_empty())
        .map(|message| IngestItem {
            namespace: None,
            source,
            source_id: source_id.to_string(),
            owner: owner.to_string(),
            source_ref: message.source_ref.map(SourceRef::new),
            content: message.body,
            mime: None,
            timestamp: Some(message.sent_at),
            tags: tags.to_vec(),
            taint: MemoryTaint::Internal,
            path_scope: None,
            author: Some(message.from),
            channel_label: Some(thread_subject.clone()),
            to: message.to,
            cc: message.cc,
            subject: Some(message.subject),
            list_unsubscribe: message.list_unsubscribe,
            platform: Some(provider.clone()),
        })
        .collect()
}

/// As [`chat_data_source`], for a document payload's `provider`.
///
/// `Upload` is the fallback because it is the member that means "handed to the
/// memory layer directly, with no upstream to re-read it from", which is what
/// a document arriving over this RPC is.
fn document_data_source(provider: &str) -> DataSource {
    match DataSource::parse(provider) {
        Ok(source) if source.kind() == SourceKind::Document => source,
        _ => DataSource::Upload,
    }
}

/// Canonicalise a chat batch into the contract's items.
///
/// The attribution trio is what keeps the stored rows identical to the ones
/// the in-process pipeline wrote. The driver rebuilds a `ChatBatch` from the
/// **first** item's `platform` and `channel_label` and from each item's
/// `author`, falling back to values that are not this payload's (the
/// `DataSource` name and the `source_id`), so all three are set on every item
/// rather than only where they differ.
///
/// Messages whose text trims to empty are dropped before the call.
/// `validate_ingest_item` answers `Invalid` for empty content and the driver
/// checks every item before ingesting any, so one attachment-only message
/// would fail the whole batch where the in-process pipeline wrote the rest of
/// it. What the filter costs is that message's bare `## <ts> — <author>`
/// header, which carried no content. Same filter, for the same reason, as
/// `agent::harness::archivist::tree_ingest`.
fn chat_items(source_id: &str, owner: &str, tags: &[String], batch: ChatBatch) -> Vec<IngestItem> {
    let ChatBatch {
        platform,
        channel_label,
        messages,
    } = batch;
    let source = chat_data_source(&platform);
    messages
        .into_iter()
        .filter(|message| !message.text.trim().is_empty())
        .map(|message| IngestItem {
            namespace: None,
            source,
            source_id: source_id.to_string(),
            owner: owner.to_string(),
            source_ref: message.source_ref.map(SourceRef::new),
            content: message.text,
            // The payload carries no MIME. `None` is the honest answer — the
            // driver validates what it is told, and naming a type here would
            // be asserting one the caller never claimed.
            mime: None,
            timestamp: Some(message.timestamp),
            tags: tags.to_vec(),
            taint: MemoryTaint::Internal,
            path_scope: None,
            author: Some(message.author),
            channel_label: Some(channel_label.clone()),
            // Mail-only fields; this source is not mail, and the contract
            // documents empty/absent as the same statement as "not mail".
            to: Vec::new(),
            cc: Vec::new(),
            subject: None,
            list_unsubscribe: None,
            platform: Some(platform.clone()),
        })
        .collect()
}

/// Canonicalise a document payload into the contract's single item.
///
/// `title` does not cross, and the contract's `ingest_document` hard-codes an
/// empty one on the way back in. Nothing is lost by that: the document
/// canonicaliser writes the body alone into the stored markdown and carries
/// the title nowhere else — it reads it only to decide whether the payload was
/// wholly empty, which [`ingest_rpc`] now answers before the call.
fn document_item(source_id: &str, owner: &str, tags: &[String], doc: DocumentInput) -> IngestItem {
    IngestItem {
        namespace: None,
        source: document_data_source(&doc.provider),
        source_id: source_id.to_string(),
        owner: owner.to_string(),
        source_ref: doc.source_ref.map(SourceRef::new),
        content: doc.body,
        mime: None,
        timestamp: Some(doc.modified_at),
        tags: tags.to_vec(),
        taint: MemoryTaint::Internal,
        // This handler called `ingest_document_with_scope(.., None)`, so the
        // scope stays unset rather than falling back to `source_id`.
        path_scope: None,
        author: None,
        channel_label: None,
        // Mail-only fields; this source is not mail, and the contract
        // documents empty/absent as the same statement as "not mail".
        to: Vec::new(),
        cc: Vec::new(),
        subject: None,
        list_unsubscribe: None,
        platform: None,
    }
}

/// Map the driver's outcome onto the wire body.
///
/// `source_id` comes from the request, not from the outcome echoing it back:
/// the caller supplied it, and reading it off the producer is what would let a
/// reply be paired with the wrong request if a producer ever normalised it.
///
/// `written` / `skipped` are the contract's names for the two counts this wire
/// calls `chunks_written` / `chunks_dropped`, and they carry the same two
/// facts: `skipped` counts dropped units only, a refused call being
/// `already_ingested` instead, which is what `chunks_dropped` has always meant
/// here.
fn response_from_outcome(source_id: String, outcome: IngestOutcome) -> IngestResponse {
    IngestResponse {
        source_id,
        chunks_written: outcome.written as usize,
        chunks_dropped: outcome.skipped as usize,
        chunk_ids: outcome.ids,
        extract_jobs_enqueued: outcome.extract_jobs_enqueued as usize,
        already_ingested: outcome.already_ingested,
    }
}

/// Unified ingest RPC handler. Dispatches on `source_kind`.
///
/// Chat and document go to the bound driver's `Ingest` family. At the v1.3.0
/// module pin they could not: the contract's `IngestOutcome` carried neither
/// `already_ingested` nor `extract_jobs_enqueued` (both would have decoded to
/// their serde defaults, reporting every duplicate submission as a plain empty
/// write, forever) and its `skipped` counted duplicate-refusals rather than
/// dropped units, so `chunks_dropped` would have carried a different fact
/// under the same name. v1.4.0 closes all three, which is why the swap is a
/// swap and not a wire change —
/// `the_response_body_serialises_exactly_as_the_engine_summary` still holds.
///
/// **Mail is still on the in-process pipeline, and it is the last thing in this
/// file that is** (#5560). This paragraph used to list two blockers, and
/// **both are closed** — re-checked 2026-08-25 rather than inherited, because
/// leaving them stated is what would stop the next reader from finishing it:
///
/// 1. *"`IngestItem` carries no recipient list, no per-message subject and no
///    `List-Unsubscribe`."* It carries all of them now, plus `platform`, and
///    the contract's own field docs are written for this path — they say
///    dropping the unsubscribe header makes the unsubscribe flow impossible
///    rather than merely less complete. `chat_items` and `document_item`
///    already spell the five mail fields as their not-mail values.
/// 2. *"`ModuleMemoryProvider` does not forward `IngestEmail`."* It does —
///    `modules::memory` implements `ingest_email` beside `ingest_document` and
///    `ingest_chat`, and the module declares the member.
///
/// The driver's mapping is a lossless inverse: it rebuilds `EmailThread` with
/// `provider` from `platform`, `thread_subject` from `channel_label`, `from`
/// from `author`, and `to` / `cc` / `subject` / `list_unsubscribe` verbatim,
/// then calls the same `ingest_pipeline::ingest_email` this arm calls now.
///
/// So what is left is host work plus one verification, and neither is a
/// contract change:
///
/// - An `email_items(source_id, owner, tags, EmailThread)` mapper that is the
///   **exact** inverse of that reconstruction (`platform: Some(provider)`,
///   `channel_label: Some(thread_subject)`, `author: Some(from)`,
///   `subject: Some(msg.subject)` — always `Some`, so no `unwrap_or` fallback
///   on the far side can substitute a different value), a `DataSource` chooser
///   in the shape of `chat_data_source`, and a `DriverIngest::Email` arm.
/// - A decision on empty bodies, which is a real behaviour delta and wants its
///   own test: `validate_ingest_item` answers `Invalid` for empty content and
///   the driver validates every item before ingesting any, so one body-less
///   message fails the whole thread where the in-process pipeline writes the
///   rest of it. The chat arm answers this by filtering first; mail has to
///   choose the same, and a header-only message is more plausible in mail.
/// - **Check the released artifact, not the submodule.** The five mail fields
///   are `#[serde(default)]`, so a pinned module that predates them decodes
///   every one to its default and mail loses its headers **silently** — the
///   capability check stays green because the family and the member both
///   exist. `vendor/tinymemory` is currently *behind* the pinned release, so
///   grep the tag.
pub async fn ingest_rpc(
    config: &Config,
    req: IngestRequest,
) -> Result<RpcOutcome<IngestResponse>, String> {
    let IngestRequest {
        source_kind,
        source_id,
        owner,
        tags,
        payload,
    } = req;

    log::debug!(
        "[memory::rpc] ingest kind={} source_id={}",
        source_kind.as_str(),
        source_id
    );

    let response = match source_kind {
        SourceKind::Chat => {
            let batch: ChatBatch = serde_json::from_value(payload).map_err(|e| {
                let msg = invalid_payload_message(SourceKind::Chat, &e);
                log::warn!("[memory::rpc] invalid payload for chat");
                msg
            })?;
            let messages = chat_items(&source_id, &owner, &tags, batch);
            if messages.is_empty() {
                // Answered here rather than handed over. The in-process
                // pipeline returned an empty summary for a batch it could not
                // canonicalise, and "the driver returns zeros for zero items"
                // is a behaviour of the one driver we ship, not something the
                // contract promises of the next one.
                response_from_outcome(source_id.clone(), IngestOutcome::default())
            } else {
                let outcome = ingest_through_driver(config, DriverIngest::Chat(messages))
                    .await
                    .inspect_err(|_| {
                        log::warn!("[memory::rpc] chat ingestion failed");
                    })?;
                response_from_outcome(source_id.clone(), outcome)
            }
        }
        SourceKind::Email => {
            let thread: EmailThread = serde_json::from_value(payload).map_err(|e| {
                let msg = invalid_payload_message(SourceKind::Email, &e);
                log::warn!("[memory::rpc] invalid payload for email");
                msg
            })?;
            let messages = email_items(&source_id, &owner, &tags, thread);
            if messages.is_empty() {
                // The chat arm's answer, for the same reason: a thread whose
                // every message was body-less canonicalises to nothing, and
                // "the driver returns zeros for zero items" is a behaviour of
                // the one driver we ship rather than a contract promise.
                response_from_outcome(source_id.clone(), IngestOutcome::default())
            } else {
                let outcome = ingest_through_driver(config, DriverIngest::Email(messages))
                    .await
                    .inspect_err(|_| {
                        log::warn!("[memory::rpc] email ingestion failed");
                    })?;
                response_from_outcome(source_id.clone(), outcome)
            }
        }
        SourceKind::Document => {
            let doc: DocumentInput = serde_json::from_value(payload).map_err(|e| {
                let msg = invalid_payload_message(SourceKind::Document, &e);
                log::warn!("[memory::rpc] invalid payload for document");
                msg
            })?;
            let item = document_item(&source_id, &owner, &tags, doc);
            if item.content.trim().is_empty() {
                // The chat arm's filter, on this arm's single item. Empty
                // content is `Invalid` at the driver, so a body-less document
                // would arrive as a failed call; the in-process pipeline
                // instead canonicalised it to a lone whitespace chunk. Neither
                // is worth keeping — "nothing to ingest" is. What it gives up
                // is that a body-less payload under an already-claimed
                // `source_id` used to answer `already_ingested`, and there is
                // nothing behind that gate to go and clear when the body was
                // empty.
                response_from_outcome(source_id.clone(), IngestOutcome::default())
            } else {
                let outcome = ingest_through_driver(config, DriverIngest::Document(item))
                    .await
                    .inspect_err(|_| {
                        log::warn!("[memory::rpc] document ingestion failed");
                    })?;
                response_from_outcome(source_id.clone(), outcome)
            }
        }
    };

    Ok(RpcOutcome::single_log(
        response,
        format!(
            "memory_tree: ingest kind={} source_id={source_id}",
            source_kind.as_str()
        ),
    ))
}

/// Query shape for the `list_chunks` RPC.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListChunksRequest {
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub since_ms: Option<i64>,
    #[serde(default)]
    pub until_ms: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Response shape for the `list_chunks` RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListChunksResponse {
    pub chunks: Vec<Chunk>,
}

/// `list_chunks` RPC handler. Filters and returns persisted chunks ordered by
/// timestamp DESC.
///
/// `scope` is `None`, which is not an oversight: this listing has never
/// applied the per-turn source allowlist — the engine query it replaced set
/// `source_scope: None` — and it is reached by inspection surfaces rather than
/// by an agent turn. Narrowing it here would be a policy change wearing a
/// routing change's clothes.
pub async fn list_chunks_rpc(
    config: &Config,
    req: ListChunksRequest,
) -> Result<RpcOutcome<ListChunksResponse>, String> {
    // Parsed before the driver is resolved so an unknown kind stays a caller
    // error naming the offending value, rather than a driver round trip that
    // returns nothing and looks like an empty store.
    let query = ChunkQuery {
        source_kind: match req.source_kind.as_deref() {
            None => None,
            Some(s) => Some(SourceKind::parse(s)?),
        },
        source_id: req.source_id,
        owner: req.owner,
        since_ms: req.since_ms,
        until_ms: req.until_ms,
        limit: req.limit,
        offset: None,
        exclude_dropped: false,
        // The filtered-listing predicates this request does not carry. An empty
        // predicate is unfiltered, so the defaults leave the query exactly as
        // narrow as the fields above already make it.
        ..Default::default()
    };

    // No `spawn_blocking`: the driver owns whether its own reads block, and the
    // module's do not run on this thread at all.
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let rows = match binding.provider().as_chunks() {
        Some(chunks) => chunks
            .list_chunks(&query, None)
            .await
            .map_err(|e| format!("list_chunks: {e}"))?,
        // Read-only, so an empty page is the honest answer: a driver with no
        // chunk tier holds no rows to list, which is a true statement about it
        // rather than a fault the caller can act on.
        None => {
            log::debug!(
                "[memory-tree][rpc] list_chunks: driver '{}' does not serve Chunks; reporting empty",
                binding.driver_id()
            );
            Vec::new()
        }
    };

    let n = rows.len();
    Ok(RpcOutcome::single_log(
        ListChunksResponse { chunks: rows },
        format!("memory_tree: list_chunks n={n}"),
    ))
}

/// Request shape for the `get_chunk` RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetChunkRequest {
    pub id: String,
}

/// Response shape for the `get_chunk` RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetChunkResponse {
    pub chunk: Option<Chunk>,
}

/// `get_chunk` RPC handler. Returns the chunk identified by `id`, or `None`.
pub async fn get_chunk_rpc(
    config: &Config,
    req: GetChunkRequest,
) -> Result<RpcOutcome<GetChunkResponse>, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let chunk = match binding.provider().as_chunks() {
        Some(chunks) => chunks
            .get_chunk(&req.id)
            .await
            .map_err(|e| format!("get_chunk: {e}"))?,
        // `None` is already this handler's answer for an id the store does not
        // hold, and a driver with no chunk tier holds none — so the degrade is
        // indistinguishable from the ordinary miss, which is what makes it safe
        // here and not on a write.
        None => {
            log::debug!(
                "[memory-tree][rpc] get_chunk: driver '{}' does not serve Chunks; reporting none",
                binding.driver_id()
            );
            None
        }
    };
    Ok(RpcOutcome::single_log(
        GetChunkResponse { chunk },
        format!("memory_tree: get_chunk id={}", req.id),
    ))
}

// ── Driver diagnostics ───────────────────────────────────────────────────
//
// The numbers below used to come from `SELECT`s against TinyCortex's tables.
// They come from the bound driver now, which is what lets a workspace run on
// a driver that is not TinyCortex and still answer "how far behind is the
// pipeline".

/// The driver's identifier for a re-embed backfill job.
///
/// Job kinds are the driver's own vocabulary, not the contract's — a driver
/// that never enqueues this one answers zero for it, which is the honest
/// count and exactly what a status poll wants to hear.
const REEMBED_BACKFILL_KIND: &str = "reembed_backfill";

/// Aggregate counts over the bound driver's stored chunks.
///
/// Zeroed rather than refused when the driver does not serve `Maintenance`:
/// this feeds a status surface, and a status surface that errors tells the
/// user less than one reporting an empty store.
async fn store_stats(
    config: &Config,
) -> Result<crate::openhuman::memory::api::provider::types::StoreStats, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] store_stats: driver '{}' does not serve Maintenance; reporting empty",
            binding.driver_id()
        );
        return Ok(Default::default());
    };
    maintenance
        .store_stats()
        .await
        .map_err(|e| format!("store_stats: {e}"))
}

/// The bound driver's queue state, optionally narrowed to one job kind.
///
/// A driver error propagates, the same way [`store_stats`] propagates its own.
/// That matters most for `backfill_status_rpc`, which is asked whether a modal
/// may close: guessing "nothing pending" would dismiss it over a live
/// backfill. A driver that does not serve `Maintenance` still reports empty
/// rather than erroring, because it has no queue to be behind on.
async fn queue_stats(
    config: &Config,
    kind: Option<&str>,
) -> Result<crate::openhuman::memory::api::provider::types::QueueStats, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] queue_stats: driver '{}' does not serve Maintenance; reporting empty",
            binding.driver_id()
        );
        return Ok(Default::default());
    };
    maintenance
        .queue_stats(kind)
        .await
        .map_err(|e| format!("queue_stats: {e}"))
}

/// Whether the driver has a re-embed backfill chain running.
///
/// Scoped to the driver's process, not to this store — the contract member says
/// so in its own signature, which is why it is a member rather than a field on
/// [`queue_stats`]. A driver serving several stores answers the same for all of
/// them.
///
/// A driver without Maintenance reports `false` rather than erroring, matching
/// its siblings: "this driver runs no backfill" is true of it, not a fault the
/// caller can act on.
async fn backfill_in_progress(config: &Config) -> Result<bool, String> {
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] backfill_in_progress: driver '{}' does not serve Maintenance; reporting false",
            binding.driver_id()
        );
        return Ok(false);
    };
    maintenance
        .backfill_in_progress()
        .await
        .map_err(|e| format!("backfill_in_progress: {e}"))
}

/// Response from the `memory_backfill_status` RPC (#1574 §4b). The frontend
/// polls this while the re-embed modal is open to surface progress and to
/// dismiss the modal once the new embedding space is fully covered.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackfillStatusResponse {
    /// True while a re-embed backfill chain still has work pending — the
    /// #1365 flag OR a queued/running `reembed_backfill` job.
    pub in_progress: bool,
    /// Count of `reembed_backfill` jobs in `ready` or `running` state. `0`
    /// with `in_progress=false` means the active embedding space is fully
    /// covered (modal can close).
    pub pending_jobs: u64,
}

/// `memory_backfill_status` RPC handler (#1574 §4b). No inputs — reports
/// whether a per-model re-embed backfill is in flight so the UI can warn
/// the user that semantic recall is reduced until it drains.
pub async fn backfill_status_rpc(
    config: &Config,
) -> Result<RpcOutcome<BackfillStatusResponse>, String> {
    log::debug!("[memory::rpc] backfill_status: entry");
    // Asked of the bound driver rather than of TinyCortex's tables. No
    // `spawn_blocking` here any more: the driver owns whether its own reads
    // block, and a host that wraps them a second time is guessing about
    // storage it no longer talks to.
    let queue = queue_stats(config, Some(REEMBED_BACKFILL_KIND))
        .await
        .map_err(|e| {
            let msg = format!("memory_backfill_status: {e}");
            log::debug!("[memory::rpc] backfill_status: error: {msg}");
            msg
        })?;
    // Ready + running, not `total - done`: a failed backfill job is finished
    // with, and counting it as pending leaves the modal open forever.
    let pending_jobs: u64 = queue.ready + queue.running;
    // Asked of the driver, not of the host-linked engine's process-global. That
    // static covers the instant between one backfill link settling and the next
    // being enqueued, which the counts cannot see — but re-embedding runs in the
    // module now, and a `cdylib` has its own statics, so the host-side copy reads
    // `false` forever and the modal closes while work is still being prepared.
    //
    // The member is process-wide rather than store-scoped, and says so in its
    // signature; that is why it is not a `QueueStats` field, where a per-store
    // snapshot would have implied a scoping it does not have. A read failure
    // degrades to the counts rather than failing the polled RPC.
    let driver_backfilling = backfill_in_progress(config).await.unwrap_or_else(|e| {
        log::warn!("[memory::rpc] backfill_status: backfill_in_progress read failed: {e}");
        false
    });
    let in_progress = driver_backfilling || pending_jobs > 0;
    Ok(RpcOutcome::single_log(
        BackfillStatusResponse {
            in_progress,
            pending_jobs,
        },
        format!("memory_tree: backfill_status in_progress={in_progress} pending={pending_jobs}"),
    ))
}

// ── pipeline_status / set_enabled (#1856 Part 1) ─────────────────────────

/// Per-status counters for the `mem_tree_jobs` table — snapshot returned by
/// the `memory_tree_pipeline_status` RPC. Only the three states the status
/// panel surfaces are exposed; `done` / `cancelled` are intentionally
/// omitted to keep the wire payload small.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineJobCounts {
    /// Jobs queued and waiting for a worker (`status = 'ready'`).
    pub ready: u64,
    /// Jobs currently being processed by a worker (`status = 'running'`).
    pub running: u64,
    /// Jobs that exhausted retries and remain in the table for diagnosis
    /// (`status = 'failed'`).
    pub failed: u64,
}

/// Response from the `memory_tree_pipeline_status` RPC (#1856 Part 1).
///
/// Aggregates "is the Memory Tree healthy?" signals into a single payload
/// the UI status panel can render without secondary fetches:
///
/// - `status` is a coarse, UI-shaped string (`running`/`paused`/`syncing`/
///   `error`/`idle`) derived from the other fields so the frontend stays
///   purely presentational.
/// - `wiki_size_bytes` is a recursive walk of the on-disk `wiki/` sub-tree
///   under the memory-tree content root; recomputed every call (cheap for
///   typical workspaces). The walk is scoped to `wiki/` so the figure
///   reflects the user-visible wiki only — not the sibling `raw/`,
///   `email/`, `chat/`, `document/` staging directories.
/// - `pipeline_jobs` is a snapshot of the queue — running > 0 implies
///   active sync, failed > 0 implies degraded.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PipelineStatusResponse {
    /// Aggregated status string: `running` | `paused` | `syncing` |
    /// `degraded` | `error` | `idle`. Derivation:
    /// 1. `is_paused` (scheduler-gate `off`) wins → `paused`.
    /// 2. otherwise failed > 0 → `error`.
    /// 3. otherwise degraded (#002, recall/structure reduced) → `degraded`.
    /// 4. otherwise running > 0 → `syncing`.
    /// 5. otherwise total_chunks > 0 → `running`.
    /// 6. otherwise → `idle`.
    pub status: String,
    /// Optional human-readable reason — populated when status is
    /// `paused` or `error`. `None` otherwise.
    pub reason: Option<String>,
    /// Epoch milliseconds of the most-recent chunk timestamp across all
    /// sources. Zero when the store is empty.
    pub last_sync_ms: i64,
    /// Total `mem_tree_chunks` rows across all sources.
    pub total_chunks: u64,
    /// Recursive byte size of the on-disk `wiki/` sub-tree under the
    /// memory-tree content root. Zero when the `wiki/` directory does not
    /// exist yet or cannot be read. Scoped to `wiki/` so the value matches
    /// the user-visible "Wiki size" tile (#1856 follow-up).
    pub wiki_size_bytes: u64,
    /// Snapshot counts from `mem_tree_jobs`.
    pub pipeline_jobs: PipelineJobCounts,
    /// Convenience flag: at least one job is currently `running`.
    pub is_syncing: bool,
    /// Convenience flag: scheduler-gate is in `off` mode, so all LLM-bound
    /// background work is paused cooperatively.
    pub is_paused: bool,
    /// #002 (FR-002/FR-004): "the pipeline ran but output quality is reduced"
    /// — `semantic_recall` true when embeddings were skipped (no usable
    /// provider, so recall falls back to recency), `structure` true when
    /// extraction yielded nothing across the board (empty wiki). Carries the
    /// typed `cause` so the UI can render an actionable remediation. Additive:
    /// `#[serde(default)]` keeps older clients deserialising the response.
    #[serde(default)]
    pub degraded: crate::openhuman::memory::tree::health::DegradedState,
    /// #002 (FR-004): the single first blocking/most-significant cause, as a
    /// typed failure with an i18n remediation key. Populated from a failed
    /// job's classified reason or the active degradation cause; `None` when
    /// the pipeline is healthy. The frontend renders this verbatim (resolving
    /// `remediation_key`) instead of re-deriving a cause from raw counters.
    #[serde(default)]
    pub first_blocking_cause: Option<crate::openhuman::memory::tree::health::PipelineFailure>,
    /// #002 (FR-010 / US5): fraction of chunks with ≥1 indexed entity, in
    /// `[0.0, 1.0]`. Near 0 with `total_chunks > 0` means extraction is
    /// producing no structure (the "empty-but-built wiki"). `None` when the
    /// metric could not be measured (DB read error) — deliberately distinct
    /// from a genuine `Some(0.0)` so the status surface never misreports a
    /// broken measurement path as a structure failure. Additive
    /// (`#[serde(default)]` → `None` for older clients).
    #[serde(default)]
    pub extraction_coverage: Option<f32>,
    /// openhuman#5820: the most recent corrupt-store quarantine in this
    /// workspace, derived from disk (`memory_tree/chunks.db.corrupt-<ts>`),
    /// so it survives restarts and reaches a renderer that was not connected
    /// when the quarantine happened. `None` when nothing was ever quarantined.
    /// Reported until the rebuilt store holds a chunk again (`resynced`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine: Option<QuarantineStatus>,
}

/// `memory_tree_pipeline_status` RPC handler (#1856 Part 1).
///
/// A corrupt-store quarantine as the status surface reports it (openhuman#5820).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineStatus {
    /// Epoch milliseconds of the quarantine, parsed from the file name's UTC
    /// timestamp (`chunks.db.corrupt-%Y%m%dT%H%M%SZ`).
    pub quarantined_at_ms: i64,
    /// The preserved copy of the damaged database. Local to this machine and
    /// shown only to its own user, so the user can hand it to recovery tooling.
    pub quarantined_path: String,
    /// Whether the rebuilt store holds any chunk again. The quarantine leaves
    /// an empty schema, so a non-empty store means the user has re-synced
    /// and the notice can retire. Deliberately not a timestamp comparison:
    /// chunk timestamps are *content* time (a mail's `sent_at`, a file's
    /// `modified_at`), so restored history predates the quarantine forever.
    pub resynced: bool,
}

/// Newest `chunks.db.corrupt-<ts>` under `<workspace>/memory_tree`, if any.
///
/// Disk is the durable record: the engine's quarantine renames the damaged
/// file beside the store and never deletes it, so a status read after a
/// restart — or from a renderer that missed the live event — still finds it.
/// Side-file quarantines (`chunks.db-wal.corrupt-…`) do not match the prefix.
fn latest_quarantine(
    workspace_dir: &std::path::Path,
    total_chunks: u64,
) -> Option<QuarantineStatus> {
    const PREFIX: &str = "chunks.db.corrupt-";
    let dir = workspace_dir.join("memory_tree");
    let newest = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let stamp = name.strip_prefix(PREFIX)?.to_owned();
            let at = chrono::NaiveDateTime::parse_from_str(&stamp, "%Y%m%dT%H%M%SZ").ok()?;
            Some((at.and_utc().timestamp_millis(), entry.path()))
        })
        .max_by_key(|(at, _)| *at)?;
    let (quarantined_at_ms, path) = newest;
    Some(QuarantineStatus {
        quarantined_at_ms,
        quarantined_path: path.display().to_string(),
        resynced: total_chunks > 0,
    })
}

/// Aggregates `list_sources` + `count_by_status` + a recursive disk-size
/// probe into the [`PipelineStatusResponse`] the UI status panel renders.
/// All blocking work is dispatched onto `spawn_blocking` so the async
/// runtime isn't held during SQLite or filesystem I/O.
pub async fn pipeline_status_rpc(
    config: &Config,
) -> Result<RpcOutcome<PipelineStatusResponse>, String> {
    use tinymemory_api::host::SchedulerGateMode;

    log::debug!("[memory-tree][rpc] pipeline_status: entry");

    // Chunk aggregates — count, extracted count and newest timestamp, in one
    // observation of the driver. Splitting them is what let a write land
    // between the count and the extracted count and report an extraction
    // coverage above 100%.
    let store = store_stats(config).await.map_err(|e| {
        log::warn!("[memory-tree][rpc] pipeline_status: {e}");
        e
    })?;
    let total_chunks = store.chunks;
    // The wire field is a plain `i64` where the driver answers `Option`, and
    // `0` is its established "never synced" value — an empty store has no
    // newest chunk, which is not a chunk stamped at the epoch.
    let last_sync_ms = store.most_recent_chunk_ms.unwrap_or(0).max(0);

    // Job counters — one observation of the queue, where this used to be five
    // separate reads at five instants. That mattered: `failed_unrecoverable`
    // is the #3365 left-right split (of the failed jobs, how many are the
    // hard, user-actionable kind vs transient ones that self-heal via
    // auto-requeue, since only the former escalates to `error`), and a retry
    // landing between the two reads could report more unrecoverable failures
    // than failures.
    //
    // #5324's stall signal comes from the same snapshot for the same reason —
    // an idle time computed against counts taken at a different instant reads
    // as a stall that never happened.
    let now_ms = chrono::Utc::now().timestamp_millis();
    let queue = queue_stats(config, None).await.map_err(|e| {
        log::warn!("[memory-tree][rpc] pipeline_status: {e}");
        e
    })?;
    let pipeline_jobs = PipelineJobCounts {
        ready: queue.ready,
        running: queue.running,
        failed: queue.failed,
    };
    let failed_unrecoverable = queue.failed_unrecoverable;
    let queue_idle_ms = queue_idle_ms(&queue, now_ms);

    // Disk size — best-effort. Permission errors etc. degrade to 0 with a
    // warn log rather than failing the whole RPC. Scoped to the `wiki/`
    // sub-directory so the tile lives up to its "Wiki size" label — the
    // sibling `raw/` / `email/` / `chat/` / `document/` staging directories
    // hold pre-canonicalised content and should not roll into the figure
    // surfaced to the user (#1856 CodeRabbit feedback).
    let wiki_root = config.memory_tree_content_root().join("wiki");
    let wiki_size_bytes = tokio::task::spawn_blocking(move || compute_dir_size_bytes(&wiki_root))
        .await
        .map_err(|e| {
            let msg = format!("pipeline_status size-walk join error: {e}");
            log::warn!("[memory-tree][rpc] pipeline_status: {msg}");
            msg
        })?;

    let is_paused = config.scheduler_gate.mode == SchedulerGateMode::Off;
    let is_syncing = pipeline_jobs.running > 0;

    // #002: read the process-global degradation snapshot (set by the embed /
    // extract stages) so a half-working sync surfaces as `degraded` with a
    // cause rather than a misleading `running`. The structure-degraded latch is
    // a liveness signal ("the extraction model is timing out") kept honest at
    // its source in `extract::llm` — it self-clears on the next *completed*
    // extraction (#3365), so the status surface never consults the unrelated
    // `extraction_coverage` metric to second-guess it here.
    let degraded = crate::openhuman::memory::tree::health::current_degraded_state();

    let (status, reason) = derive_pipeline_status(
        is_paused,
        config.scheduler_gate.mode,
        is_syncing,
        pipeline_jobs.failed,
        failed_unrecoverable,
        total_chunks,
        &degraded,
        queue_idle_ms,
    );

    // #002 first_blocking_cause (FR-004): the most-recent failed job's typed
    // reason, surfaced verbatim by the UI. Best-effort — log-then-drop, so a
    // read failure is distinguishable in the log from "no blocking cause"
    // while never failing the polled status RPC. The `spawn_blocking` this
    // used to sit in is the driver's business now.
    let latest_failure = latest_failed_job_failure(config).await.unwrap_or_else(|e| {
        log::warn!(
            "[memory-tree][rpc] pipeline_status: latest_failed_job_failure read failed: {e}"
        );
        None
    });

    // #002 extraction_coverage (FR-010/US5): fraction of chunks with
    // structure, surfaced as its own display metric — deliberately NOT folded
    // into the status pill (#3365: coverage is a cumulative measure, unrelated
    // to the live structure-degraded liveness signal).
    //
    // Derived from the `store_stats` snapshot above, so the numerator and
    // denominator are one observation and the fraction cannot exceed 1.0 —
    // which two separate reads could produce, and did.
    //
    // An empty store still reports `Some(0.0)`, matching what this returned
    // before. `None` here has always meant "unavailable", and while `0.0` for
    // a store with nothing to extract is arguably the wrong reading, changing
    // it is a decision about what the panel shows, not a consequence of moving
    // the read behind the contract.
    let extraction_coverage = Some(if store.chunks == 0 {
        0.0
    } else {
        store.chunks_with_structure as f32 / store.chunks as f32
    });

    // A hard failed-job reason is more urgent than a soft degradation; fall
    // back to the active degradation cause, then `None` when healthy.
    let first_blocking_cause = latest_failure.or_else(|| degraded.cause.clone());

    // openhuman#5820: disk-derived so it is durable and replayable; "resynced"
    // reads the same `store` observation as the chunk tile, so the two agree.
    let quarantine = latest_quarantine(config.workspace_dir.as_path(), total_chunks);
    if let Some(q) = &quarantine {
        log::debug!(
            "[memory-tree][rpc] pipeline_status: quarantine at={} resynced={} path={}",
            q.quarantined_at_ms,
            q.resynced,
            q.quarantined_path
        );
    }

    let payload = PipelineStatusResponse {
        status: status.clone(),
        reason: reason.clone(),
        last_sync_ms,
        total_chunks,
        wiki_size_bytes,
        pipeline_jobs,
        is_syncing,
        is_paused,
        degraded,
        first_blocking_cause,
        extraction_coverage,
        quarantine,
    };

    log::debug!(
        "[memory-tree][rpc] pipeline_status: ok status={status} total_chunks={total_chunks} wiki_size_bytes={wiki_size_bytes} ready={r} running={n} failed={f} reason={reason:?}",
        r = payload.pipeline_jobs.ready,
        n = payload.pipeline_jobs.running,
        f = payload.pipeline_jobs.failed,
    );

    Ok(RpcOutcome::single_log(
        payload,
        format!(
            "memory_tree: pipeline_status status={status} total_chunks={total_chunks} is_paused={is_paused} is_syncing={is_syncing}",
        ),
    ))
}

/// `memory_tree_doctor` RPC handler (#002 FR-009). Runs the one-shot
/// pipeline diagnostic and returns the [`DoctorReport`] — per-stage health,
/// the first blocking cause, the degraded snapshot, and counters. Exposed for
/// the agent tool + CLI so the agent can self-diagnose an empty/stalled wiki.
/// Synchronous + cheap (config + queue counters + degraded flags), so no
/// blocking-pool dispatch is needed.
pub async fn doctor_rpc(
    config: &Config,
) -> Result<RpcOutcome<crate::openhuman::memory::tree::health::DoctorReport>, String> {
    // Offload the doctor's blocking SQLite reads off the async runtime thread.
    let report = crate::openhuman::memory::tree::health::async_run_doctor(config).await;
    let summary = if report.healthy {
        "memory_tree: doctor — healthy".to_string()
    } else {
        format!(
            "memory_tree: doctor — first_blocking_cause={}",
            report
                .first_blocking_cause
                .as_ref()
                .map(|f| f.code.as_str())
                .unwrap_or("unknown")
        )
    };
    Ok(RpcOutcome::single_log(report, summary))
}

/// Response from `memory_tree_retry_failed` (#002 FR-011).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RetryFailedResponse {
    /// Number of `failed` jobs flipped back to `ready` for retry.
    pub requeued: u64,
}

/// `memory_tree_retry_failed` RPC handler (#002 FR-011). Flips every
/// terminally-`failed` `mem_tree_jobs` row back to `ready` (fresh attempt
/// budget, typed reason cleared) so jobs that failed under a now-fixed config
/// re-run without re-ingesting source data. Backs the "Retry failed" button.
pub async fn retry_failed_rpc(config: &Config) -> Result<RpcOutcome<RetryFailedResponse>, String> {
    // Requeue and wake are one operation at the driver. They were two calls
    // here, which is one call away from a retry that moves rows and then lets
    // them sit until the next scheduled window.
    let requeued = crate::openhuman::memory::ops::maintenance::retry_failed(config).await?;
    Ok(RpcOutcome::single_log(
        RetryFailedResponse { requeued },
        format!("memory_tree: retry_failed requeued={requeued}"),
    ))
}

/// #002 (FR-004): the typed [`PipelineFailure`] of the most-recently-failed
/// `mem_tree_jobs` row, when it carries a classified `failure_reason` **and that
/// failure is still the pipeline's current blocking cause**. Returns `Ok(None)`
/// when there is no failed job with a typed reason (older failures predating the
/// typed-failure columns, or none at all), or when the failure has been
/// superseded (below). Best-effort: the status panel is a UI convenience, so a
/// DB error degrades to `Ok(None)` rather than failing the whole status RPC.
///
/// # Supersession — why the newest failed row is not automatically the cause
///
/// An unrecoverable failure is terminal by design: it is never retried, so its
/// row sits in `failed` forever with whatever `failure_reason` it died with.
/// Reading that row unconditionally means the panel keeps rendering the *first*
/// diagnosis it ever saw, indefinitely, no matter what the pipeline has done
/// since.
///
/// In production that surfaced as a signed-in user being told "No embeddings
/// credentials found. Log in to OpenHuman" — the remediation for an
/// `auth_missing` batch that had failed **27 days earlier**, while the queue had
/// been completing jobs normally the whole time. The banner was a tombstone, and
/// following it was impossible: the user was already logged in.
///
/// So a failure only counts as the *current* blocking cause when the queue has
/// not settled a job successfully since it. `completed_at_ms` on the newest
/// `done` row is that watermark: if the pipeline has produced output more
/// recently than the failure, the failure describes the past, not the present.
/// The failure is still counted (`failed_unrecoverable` keeps the status at
/// `error` and the "N unrecoverable failure(s) need action" reason), and "Retry
/// failed" is how the user clears it — but the *remediation text*, which tells
/// the user what to go and do right now, is withheld once it stops being true.
async fn latest_failed_job_failure(
    config: &Config,
) -> Result<Option<crate::openhuman::memory::tree::health::PipelineFailure>, String> {
    // The failure and the success watermark arrive together, as ONE answer.
    // That is not a convenience: asking twice lets a job settle between the
    // two and flip the supersession decision below, which is the race the
    // #5427 review flagged. The driver reads both on one connection; taking
    // `QueueFailure::last_success_ms` from a second call would undo that.
    let binding = crate::openhuman::memory::binding::for_config(config)?;
    let Some(maintenance) = binding.provider().as_maintenance() else {
        log::debug!(
            "[memory-tree][rpc] pipeline_status: driver '{}' does not serve Maintenance; no blocking cause",
            binding.driver_id()
        );
        return Ok(None);
    };
    let reported = maintenance
        .latest_queue_failure()
        .await
        .map_err(|e| format!("latest_failed_job_failure: {e}"))?;
    Ok(reported.as_ref().and_then(blocking_cause))
}

/// The supersession rule, over one failure the driver reported.
///
/// Split from the fetch above so it stays exercisable without a bound driver.
/// The rule is the part with edge cases — an untimestamped failure, a
/// watermark on the same millisecond, a reason this build does not know — and
/// a test that has to stand up a driver to reach it tends not to cover them.
fn blocking_cause(
    reported: &crate::openhuman::memory::api::provider::types::QueueFailure,
) -> Option<crate::openhuman::memory::tree::health::PipelineFailure> {
    use crate::openhuman::memory::tree::health::{FailureClass, FailureCode, PipelineFailure};

    let reason = &reported.reason;
    let class = reported.class.clone();
    let failed_at_ms = reported.completed_at_ms;
    let last_success_ms = reported.last_success_ms;

    // Log every supersession branch, not only the withheld one, so the decision
    // is greppable from the logs alone.
    match failed_at_ms {
        Some(failed_at_ms)
            if last_success_ms.is_some_and(|success_ms| success_ms > failed_at_ms) =>
        {
            log::debug!(
                "[memory-tree][rpc] pipeline_status: withholding blocking cause reason={reason} \
                 — the queue has completed a job since it failed (superseded)"
            );
            return None;
        }
        Some(_) => {
            log::debug!(
                "[memory-tree][rpc] pipeline_status: blocking cause is live reason={reason} \
                 — no successful settle since it failed"
            );
        }
        None => {
            log::debug!(
                "[memory-tree][rpc] pipeline_status: blocking cause reason={reason} has no \
                 completion timestamp — surfacing unconditionally (legacy row)"
            );
        }
    }

    // A reason this build has no code for is not a cause it can render.
    let code = FailureCode::from_str(reason)?;
    // Trust the persisted class when present and parseable; otherwise derive
    // from the code (keeps a forward-compatible default if the column is NULL
    // on an older row).
    let mut failure = PipelineFailure::new(code);
    if let Some(c) = class.as_deref() {
        if c == "transient" {
            failure.class = FailureClass::Transient;
        } else if c == "unrecoverable" {
            failure.class = FailureClass::Unrecoverable;
        }
    }
    Some(failure)
}

/// #5324: how long the queue has been sitting on eligible work without
/// finishing anything, or `None` when there is no eligible work waiting.
///
/// This is the "queued but never processed" signal. Getting the predicate
/// right matters more than it looks, because the naive versions produce false
/// alarms for exactly the heavy users this issue is about:
///
/// - **Not** `MIN(created_at_ms)` over all `ready` rows. `mark_deferred` parks
///   a backing-off job by leaving `status = 'ready'` and pushing
///   `available_at_ms` forward, so deferred work would count as waiting when
///   it is deliberately asleep.
/// - **Not** the age of the oldest eligible row either. A re-embed backfill
///   enqueues thousands of rows in one burst; six hours into a perfectly
///   healthy drain of a 68k-chunk workspace, the oldest un-drained row is by
///   definition hours old. That would flag the exact case the issue's reporter
///   was in — a big, slow, *working* backfill — as broken.
///
/// So the measure is **idle time, not backlog age**: how long since the queue
/// last settled *any* job. A pipeline making progress refreshes
/// `completed_at_ms` continuously no matter how deep the backlog is, while a
/// pipeline whose jobs all fail unrecoverably or whose worker never runs goes
/// quiet. `completed_at_ms` is stamped on failure as well as success, so a
/// fast-failing pipeline reports `error` (via `failed_unrecoverable`) rather
/// than being mislabelled as stalled.
///
/// Returns `Some(idle_ms)` only when eligible work is actually waiting — an
/// idle queue with nothing to do is not stalled, it is done. When nothing has
/// ever settled (fresh workspace whose worker has never run), idle time falls
/// back to how long the oldest eligible job has been waiting.
///
/// Derived from a snapshot rather than read on its own, so the eligible count
/// and the timestamps it is measured against come from one instant. Reading
/// them separately is what makes an idle window appear across a settle that
/// happened between two queries.
fn queue_idle_ms(
    queue: &crate::openhuman::memory::api::provider::types::QueueStats,
    now_ms: i64,
) -> Option<i64> {
    // Nothing eligible is waiting ⇒ nothing is being held up.
    if queue.eligible_now == 0 {
        return None;
    }
    let (last_settled_ms, oldest_eligible_ms) = (queue.last_completed_ms, queue.oldest_eligible_ms);
    // Idle time is "how long since the queue last made progress on the work
    // that is waiting *now*" — so start the clock at the LATER of the last
    // settle and the oldest eligible job's arrival. Using `last_settled_ms`
    // alone (`.or`) mis-reads a real shape: if the queue drained everything,
    // sat empty for days, then a fresh job arrives, the stale completion is
    // hours/days old while the new work is seconds old. Taking the max means
    // freshly-enqueued work starts its own idle window instead of inheriting
    // an ancient completion, so a just-arrived job can't be flagged `degraded`
    // before the worker has had a chance to touch it. Fall back to the oldest
    // eligible job's wait when the queue has never settled a job at all.
    let reference_ms = match (last_settled_ms, oldest_eligible_ms) {
        (Some(last_settled), Some(oldest_eligible)) => Some(last_settled.max(oldest_eligible)),
        (Some(last_settled), None) => Some(last_settled),
        (None, Some(oldest_eligible)) => Some(oldest_eligible),
        (None, None) => None,
    };
    // Clamp at zero: clock skew / a future-dated row must read as "just now",
    // never as a negative age.
    reference_ms.map(|since| (now_ms - since).max(0))
}

/// Recursive byte-count of files under `root`. Returns `0` when the root
/// does not exist or any traversal error occurs (best-effort; the status
/// panel is a UI convenience, not an audit surface).
fn compute_dir_size_bytes(root: &std::path::Path) -> u64 {
    if !root.exists() {
        return 0;
    }
    let mut total: u64 = 0;
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        match entry {
            Ok(e) if e.file_type().is_file() => {
                if let Ok(meta) = e.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
            Ok(_) => {}
            Err(err) => {
                // Both `err.path()` and `walkdir::Error`'s `Display` impl
                // embed the absolute on-disk path (which lives under the
                // user's home directory), so we redact: log only whether a
                // path was attached and the underlying `io::ErrorKind`.
                // That's enough for diagnosis while keeping the user's
                // workspace layout out of the log file.
                log::warn!(
                    "[memory-tree][rpc] pipeline_status: dir walk error has_path={} kind={:?}",
                    err.path().is_some(),
                    err.io_error().map(|e| e.kind())
                );
            }
        }
    }
    total
}

/// #5324: how long the queue may hold eligible work without settling a single
/// job before the pipeline is reported as `degraded` rather than
/// `running`/`idle`.
///
/// A working pipeline settles jobs continuously, so this is idle time, not
/// backlog depth — a deep-but-draining backfill never approaches it. Six hours
/// is far outside a normal flush window (minutes) yet well inside the "broken
/// for a month" window the issue describes, so it cannot fire on a busy
/// machine or a laptop that was asleep for an hour.
pub(crate) const QUEUE_STALL_THRESHOLD_MS: i64 = 6 * 60 * 60 * 1000;

/// Pure derivation of `(status, reason)` from raw signals. Split out so the
/// unit tests can exercise the precedence rules without spinning up a
/// store.
///
/// `queue_idle_ms` is how long the queue has held eligible work without
/// settling any job, or `None` when no eligible work is waiting (or the
/// metric could not be read).
fn derive_pipeline_status(
    is_paused: bool,
    mode: tinymemory_api::host::SchedulerGateMode,
    is_syncing: bool,
    failed: u64,
    failed_unrecoverable: u64,
    total_chunks: u64,
    degraded: &crate::openhuman::memory::tree::health::DegradedState,
    queue_idle_ms: Option<i64>,
) -> (String, Option<String>) {
    if is_paused {
        return (
            "paused".to_string(),
            Some(format!("scheduler gate mode = {}", mode.as_str())),
        );
    }
    // Host storage is unusable (EIO/ENOSPC/EROFS on the memory_tree path). This
    // is a foundational, unrecoverable error — the DB can't even open, so it
    // outranks the per-content recall/structure degradation below AND fires
    // regardless of `total_chunks` (on a dead disk we may not be able to count
    // chunks at all). Only the user can fix it (reseat/replace/free storage);
    // the actionable remediation text rides the `StorageUnavailable`
    // remediation key surfaced by the doctor's `first_blocking_cause`.
    if degraded.storage {
        return (
            "error".to_string(),
            Some("memory storage unavailable — check your disk / SD card".to_string()),
        );
    }
    // #3365: split the failed bucket by class. Only an UNRECOVERABLE failure
    // (budget / auth / dim-mismatch) is a hard `error` the user must act on —
    // it stays parked and can't self-heal. Transient failures are auto-requeued
    // by `requeue_transient_failed`, so they must NOT escalate to `error`; they
    // fall through to `degraded` ("failed, retrying") below. This fixes the prior
    // `failed > 0 → error` that flashed a scary error for a job about to retry.
    if failed_unrecoverable > 0 {
        return (
            "error".to_string(),
            Some(format!(
                "{failed_unrecoverable} unrecoverable failure(s) need action"
            )),
        );
    }
    // #5324: the queue is accepting work but not draining it. This is the
    // "silently broken for a month" shape — new files keep getting detected
    // and queued, health checks keep reporting `ok` because the process is
    // alive, and nothing ever becomes searchable memory. Liveness is not
    // output, so a queue whose oldest ready job has been waiting past the
    // threshold reports `degraded`, never `running`/`idle`.
    //
    // Sits below `error` (a typed unrecoverable failure is the more specific
    // diagnosis and carries its own remediation) and above the recall/structure
    // degradation, and is deliberately NOT gated on `total_chunks` — a queue
    // that never drained has no chunks to gate on, which is exactly the case
    // that must not read as `idle`.
    if queue_idle_ms.is_some_and(|idle| idle >= QUEUE_STALL_THRESHOLD_MS) {
        let hours = queue_idle_ms.unwrap_or(0) / (60 * 60 * 1000);
        return (
            "degraded".to_string(),
            Some(format!(
                "queue has not completed any job in {hours}h — memory is not growing"
            )),
        );
    }
    // #002 (FR-005): "degraded" sits below error but above syncing/running —
    // the pipeline is making progress, but recall/structure is reduced (or some
    // jobs failed transiently and are retrying) and the user should be told why.
    // Beats syncing/running so a half-working sync isn't reported as plain
    // "running"/"syncing".
    //
    // Only fires when there are chunks: degraded recall/structure is only
    // meaningful when there's actual content affected. An empty workspace with
    // a misconfigured embedder should show "idle" (nothing to recall) rather
    // than "degraded" (recall is broken for existing content).
    //
    // `failed` here is transient-only — any unrecoverable failure returned
    // `error` above, so a non-zero `failed` at this point means jobs that will
    // be auto-requeued.
    if (degraded.is_degraded() || failed > 0) && total_chunks > 0 {
        let mut parts: Vec<String> = Vec::new();
        if degraded.semantic_recall {
            parts.push("semantic recall disabled".to_string());
        }
        if degraded.structure {
            parts.push("wiki structure incomplete".to_string());
        }
        if failed > 0 {
            parts.push(format!("{failed} job(s) failed, retrying"));
        }
        return ("degraded".to_string(), Some(parts.join("; ")));
    }
    if is_syncing {
        return ("syncing".to_string(), None);
    }
    if total_chunks > 0 {
        return ("running".to_string(), None);
    }
    ("idle".to_string(), None)
}

/// Request shape for `memory_tree_set_enabled`. Single field — the caller
/// asks to enable (auto-mode) or pause (off-mode) all LLM-bound background
/// work.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetEnabledRequest {
    /// `true` ⇒ scheduler-gate mode becomes `auto`. `false` ⇒ `off`.
    pub enabled: bool,
}

/// Response shape for `memory_tree_set_enabled`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetEnabledResponse {
    /// Echo of the requested `enabled` state (post-write).
    pub enabled: bool,
    /// `true` when the saved mode actually flipped; `false` for no-ops.
    pub changed: bool,
    /// New scheduler-gate mode as wire string (`auto` / `off`).
    pub mode: String,
}

/// `memory_tree_set_enabled` RPC handler (#1856 Part 1).
///
/// Flips `config.scheduler_gate().mode` to either `Auto` (enabled) or `Off`
/// (paused), persists to disk via `config.save()`, and hot-reloads the
/// live scheduler-gate state so any in-flight workers immediately observe
/// the new policy at their next `wait_for_capacity()` await.
///
/// Notes:
/// - This is intentionally a single-field RPC (no batched
///   `MemoryTreeSettingsPatch`) — keeps the surface tight while #1856
///   Part 2 work lands the broader settings story.
/// - The 20-min Composio fetch loop is *not* paused by this toggle yet —
///   that requires a separate `Notify` signal and is queued for Part 2.
pub async fn set_enabled_rpc(
    config: &mut Config,
    req: SetEnabledRequest,
) -> Result<RpcOutcome<SetEnabledResponse>, String> {
    use tinymemory_api::host::SchedulerGateMode;

    let prev_mode = config.scheduler_gate.mode;
    let new_mode = if req.enabled {
        SchedulerGateMode::Auto
    } else {
        SchedulerGateMode::Off
    };

    log::debug!(
        "[memory-tree][rpc] set_enabled: requested enabled={} prev_mode={} new_mode={}",
        req.enabled,
        prev_mode.as_str(),
        new_mode.as_str(),
    );

    if prev_mode == new_mode {
        log::info!(
            "[memory-tree][rpc] set_enabled: no-op (mode already {})",
            new_mode.as_str()
        );
        return Ok(RpcOutcome::single_log(
            SetEnabledResponse {
                enabled: req.enabled,
                changed: false,
                mode: new_mode.as_str().to_string(),
            },
            format!(
                "memory_tree: set_enabled no-op enabled={} mode={}",
                req.enabled,
                new_mode.as_str()
            ),
        ));
    }

    config.scheduler_gate.mode = new_mode;
    config.save().await.map_err(|e| {
        let msg = format!("set_enabled: config.save failed: {e}");
        log::warn!("[memory-tree][rpc] {msg}");
        msg
    })?;

    // Hot-reload the live gate state — workers re-poll inside
    // `wait_for_capacity` and pick up the new policy without a restart.
    crate::openhuman::cron::scheduler_gate::gate::update_config(config.scheduler_gate.clone());

    log::info!(
        "[memory-tree][rpc] set_enabled: scheduler_gate.mode {} -> {} (enabled={})",
        prev_mode.as_str(),
        new_mode.as_str(),
        req.enabled,
    );

    Ok(RpcOutcome::single_log(
        SetEnabledResponse {
            enabled: req.enabled,
            changed: true,
            mode: new_mode.as_str().to_string(),
        },
        format!(
            "memory_tree: set_enabled enabled={} mode={} changed=true",
            req.enabled,
            new_mode.as_str()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use tempfile::TempDir;
    use tinycortex::memory::ingest::canonicalize::document::DocumentInput;
    use tinymemory_api::chunks::SourceKind;

    fn test_config() -> (TempDir, Config) {
        let tmp = TempDir::new().unwrap();
        let mut cfg = Config::default();
        cfg.workspace_dir = tmp.path().to_path_buf();
        cfg.memory_tree.embedding_endpoint = None;
        cfg.memory_tree.embedding_model = None;
        cfg.memory_tree.embedding_strict = false;
        (tmp, cfg)
    }

    /// Bind a driver reporting fixed diagnostics as `cfg`'s memory driver.
    ///
    /// See `binding::FixedDiagnostics` for why the status handlers need one:
    /// they read through the contract now, and the real driver is a compiled
    /// module that a unit test cannot load.
    fn bind_diagnostics(
        cfg: &Config,
        store: crate::openhuman::memory::api::provider::types::StoreStats,
        queue: crate::openhuman::memory::api::provider::types::QueueStats,
    ) {
        crate::openhuman::memory::binding::install_diagnostics_for_test(
            &cfg.workspace_dir,
            &cfg.subsystems.memory,
            store,
            queue,
        );
    }

    /// #5169 (`CORE-RUST-1P0`) — a chat batch whose messages omit `timestamp`
    /// must ingest, defaulting to `now()`, not reject the whole batch.
    ///
    /// The tolerance lives in `tinycortex` (`ChatMessage::timestamp` carries
    /// `#[serde(default = "chrono_now")]`), which is a **separate repository**
    /// vendored here as a submodule. Nothing in this repo guarded that
    /// contract, so a submodule bump could silently reintroduce the hard
    /// rejection and the 4xx-shaped payload would page again. This test is
    /// that guard: it fails on the parent-repo side the moment the vendored
    /// schema stops tolerating an absent timestamp.
    #[test]
    fn chat_payload_without_timestamp_is_accepted() {
        let payload = json!({
            "platform": "slack",
            "channel_label": "#general",
            "messages": [{ "author": "alice", "text": "no timestamp here" }],
        });

        let batch: ChatBatch = serde_json::from_value(payload)
            .expect("a chat message omitting `timestamp` must default, not reject the batch");

        assert_eq!(batch.messages.len(), 1);
        assert_eq!(batch.messages[0].text, "no timestamp here");
    }

    /// Sibling contract for the document arm: `modified_at` is likewise
    /// optional (`#[serde(default = "now_utc")]` in tinycortex).
    ///
    /// The payload is deliberately minimal — `title` and `body` are the only
    /// required fields on `DocumentInput`. `provider` (`default_provider`),
    /// `source_ref` (`Option`) and `modified_at` (`now_utc`) all carry serde
    /// defaults, so omitting them together pins the whole optional set rather
    /// than just the timestamp.
    #[test]
    fn document_payload_without_modified_at_is_accepted() {
        let payload = json!({ "title": "Launch plan", "body": "ship it" });

        let doc: DocumentInput = serde_json::from_value(payload)
            .expect("a document omitting `modified_at` must default, not reject");

        assert_eq!(doc.title, "Launch plan");
    }

    /// Every `SourceKind` reachable from `ingest_rpc` must produce a message
    /// the classifier recognises — otherwise that arm's caller errors keep
    /// paging while its siblings are demoted, which is the silent-drift
    /// failure the enumerated list in `is_invalid_ingest_payload_message`
    /// is meant to make impossible to miss.
    #[test]
    fn all_source_kinds_are_recognised_as_caller_payload_errors() {
        let err = serde_json::from_str::<ChatBatch>("{}").unwrap_err();
        for kind in [SourceKind::Chat, SourceKind::Email, SourceKind::Document] {
            let message = invalid_payload_message(kind, &err);
            assert!(
                is_invalid_ingest_payload_message(&message),
                "{} payload errors must classify as caller errors, got {message:?}",
                kind.as_str()
            );
        }
    }

    /// The verbatim #5169 message shape, and the negative half: unrelated
    /// failures must keep their error severity so real defects still page.
    #[test]
    fn only_ingest_payload_errors_are_demoted() {
        assert!(is_invalid_ingest_payload_message(
            "invalid chat payload: missing field `timestamp`"
        ));

        for other in [
            "invalid",
            "invalid payload",
            "invalid audio payload: missing field `timestamp`",
            "ingest: chunk store unavailable",
            "chat payload: missing field `timestamp`",
            "something failed: invalid chat payload: missing field `timestamp`",
            "",
        ] {
            assert!(
                !is_invalid_ingest_payload_message(other),
                "{other:?} must keep paging"
            );
        }
    }

    /// The ingest response is this crate's declaration of a wire the frontend
    /// reads, so nothing upstream keeps its keys honest any more.
    ///
    /// It used to be asserted against the engine's own `IngestResult`, on the
    /// reasoning that comparing to the upstream type beat hand-writing a key
    /// list. That held while the engine produced the body. It does not now:
    /// every arm builds from the contract's `IngestOutcome`, so a comparison
    /// against the engine summary would pin a shape nothing in this path
    /// produces — and it kept the engine linked here purely to describe a wire
    /// this crate owns.
    ///
    /// So the expectation is written out. That is the honest form once this
    /// crate is the declaring side: the keys below are what the frontend
    /// parses, and renaming a field on `IngestResponse` fails here rather than
    /// reaching a reader.
    #[test]
    fn the_response_body_serialises_exactly_as_the_declared_wire() {
        let ours = IngestResponse {
            source_id: "doc-launch".into(),
            chunks_written: 3,
            chunks_dropped: 1,
            chunk_ids: vec!["chunk-a".into(), "chunk-b".into(), "chunk-c".into()],
            extract_jobs_enqueued: 2,
            already_ingested: true,
        };

        assert_eq!(
            serde_json::to_value(&ours).unwrap(),
            serde_json::json!({
                "source_id": "doc-launch",
                "chunks_written": 3,
                "chunks_dropped": 1,
                "chunk_ids": ["chunk-a", "chunk-b", "chunk-c"],
                "extract_jobs_enqueued": 2,
                "already_ingested": true,
            }),
            "the ingest response wire moved — the frontend reads these names"
        );
    }

    fn sample_document(title: &str, body: &str) -> DocumentInput {
        DocumentInput {
            provider: "notion".into(),
            title: title.into(),
            body: body.into(),
            modified_at: Utc::now(),
            source_ref: Some("notion://page/launch".into()),
        }
    }

    /// Ingest reports what it wrote.
    ///
    /// Bound to the in-process TinyCortex driver rather than left to resolve on
    /// its own: the handler asks the driver for the `Ingest` family now, and
    /// what a bare test workspace binds is the null driver, which serves none.
    /// This is the engine the loadable module wraps, so the counts asserted
    /// below are the ones production gets over the bus.
    #[tokio::test]
    async fn ingest_document_reports_the_chunks_it_wrote() {
        let (_tmp, cfg) = test_config();
        crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
        let outcome = ingest_rpc(
            &cfg,
            IngestRequest {
                source_kind: SourceKind::Document,
                source_id: "doc-launch".into(),
                owner: "alice".into(),
                tags: vec!["launch".into()],
                payload: serde_json::to_value(sample_document(
                    "Launch Plan",
                    "Phoenix launch canary checklist with rollback steps.",
                ))
                .unwrap(),
            },
        )
        .await
        .unwrap();
        assert_eq!(outcome.value.source_id, "doc-launch");
        assert_eq!(outcome.value.chunks_dropped, 0);
        assert!(outcome.value.chunks_written > 0);
        assert!(
            !outcome.value.chunk_ids.is_empty(),
            "the ids are what a caller fetches a chunk back by, so a write \
             that names none is unusable even when the count is right"
        );
    }

    /// The listing degrades rather than fails when the bound driver has no
    /// chunk tier.
    ///
    /// `FixedDiagnostics` is `NullMemoryProvider`-backed, so `as_chunks()` is
    /// `None` — the shape of a driver that serves memory without exposing the
    /// engine's storage model. The handler is read-only, and an empty page is a
    /// true statement about such a driver, so it must not become a
    /// caller-facing error. The log still has to report the count it served,
    /// because a silent empty and a degraded empty look identical downstream.
    #[tokio::test]
    async fn list_chunks_reports_empty_when_the_driver_has_no_chunk_tier() {
        let (_tmp, cfg) = test_config();
        bind_diagnostics(&cfg, Default::default(), Default::default());

        let listed = list_chunks_rpc(
            &cfg,
            ListChunksRequest {
                source_kind: Some("document".into()),
                source_id: Some("doc-launch".into()),
                limit: Some(10),
                ..Default::default()
            },
        )
        .await
        .expect("a driver without the chunk family is not an error");
        assert!(listed.value.chunks.is_empty());
        assert!(listed.logs[0].contains("n=0"), "log: {}", listed.logs[0]);
    }

    /// The source gate is the driver's, and it survives the move onto the
    /// contract: `IngestOutcome::already_ingested` is the field the v1.3.0 pin
    /// did not have, and reporting a refused call as a plain empty write is
    /// exactly what this test would have started passing over.
    #[tokio::test]
    async fn ingest_document_is_idempotent_for_duplicate_source_id() {
        let (_tmp, cfg) = test_config();
        crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
        let req = IngestRequest {
            source_kind: SourceKind::Document,
            source_id: "doc-dup".into(),
            owner: "alice".into(),
            tags: vec![],
            payload: serde_json::to_value(sample_document("Launch Plan", "First body")).unwrap(),
        };

        let first = ingest_rpc(&cfg, req.clone()).await.unwrap().value;
        let second = ingest_rpc(&cfg, req).await.unwrap().value;
        assert!(first.chunks_written > 0);
        assert!(!first.already_ingested);
        // `already_ingested` with a zero write count is the whole claim:
        // documents are append-only, so a repeat submission must be recognised
        // rather than duplicated — and told apart from a write that produced
        // nothing, which is the same two numbers with a different cause.
        assert_eq!(second.chunks_written, 0);
        assert!(second.already_ingested);
        assert_eq!(second.source_id, first.source_id);
    }

    /// Regression #3568 / CORE-2K: chat payloads with RFC-3339 timestamps must
    /// be accepted — not rejected with "expected unix timestamp in milliseconds".
    #[tokio::test]
    async fn ingest_chat_accepts_rfc3339_timestamps() {
        let (_tmp, cfg) = test_config();
        crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
        let outcome = ingest_rpc(
            &cfg,
            IngestRequest {
                source_kind: SourceKind::Chat,
                source_id: "slack:#rfc3339-test".into(),
                owner: "alice".into(),
                tags: vec![],
                payload: json!({
                    "platform": "slack",
                    "channel_label": "#eng",
                    "messages": [
                        {
                            "author": "alice",
                            "timestamp": "2026-05-17T19:30:00Z",
                            "text": "planning the launch"
                        },
                        {
                            "author": "bob",
                            "timestamp": 1779046260000_i64,
                            "text": "confirmed"
                        }
                    ]
                }),
            },
        )
        .await
        .unwrap();
        assert!(!outcome.value.chunk_ids.is_empty());
    }

    /// Regression #3568 / CORE-2K: email payloads with RFC-3339 timestamps must
    /// be accepted.
    ///
    /// A driver is bound, like every sibling here. The note this replaces said
    /// the mail arm was "still on the in-process pipeline" and that the test
    /// would need `install_tinycortex_for_test` "when it moves" — it has moved:
    /// the `Email` arm now goes through `ingest_through_driver`, which resolves
    /// `provider().as_ingest()` and refuses a driver that does not serve it.
    /// Without the binding the test only passed because CI happens to set
    /// `TINYMEMORY_TEST_MODULE` to a module that serves `Ingest`, so it would
    /// fail on a machine that does not.
    #[tokio::test]
    async fn ingest_email_accepts_rfc3339_timestamps() {
        let (_tmp, cfg) = test_config();
        crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
        let outcome = ingest_rpc(
            &cfg,
            IngestRequest {
                source_kind: SourceKind::Email,
                source_id: "gmail:rfc3339-test".into(),
                owner: "alice@example.com".into(),
                tags: vec![],
                payload: json!({
                    "provider": "gmail",
                    "thread_subject": "Launch",
                    "messages": [
                        {
                            "from": "bob@example.com",
                            "to": ["alice@example.com"],
                            "subject": "Launch",
                            "sent_at": "2026-05-17T19:30:00Z",
                            "body": "Let's ship this."
                        }
                    ]
                }),
            },
        )
        .await
        .unwrap();
        assert!(!outcome.value.chunk_ids.is_empty());
    }

    /// One empty message must not fail the batch around it.
    ///
    /// `validate_ingest_item` answers `Invalid` for content that trims to
    /// empty, and the driver validates every item before ingesting any — so an
    /// attachment-only message, which reaches this handler as a message with no
    /// text, would turn a batch that has real content in it into a failed call.
    /// The in-process pipeline wrote the rest of the batch and rendered that
    /// message as a bare header; the filter keeps the first half of that and
    /// gives up only the header.
    #[tokio::test]
    async fn an_empty_chat_message_does_not_fail_the_batch_around_it() {
        let (_tmp, cfg) = test_config();
        crate::openhuman::memory::test_support::install_tinycortex_for_test(&cfg);
        let outcome = ingest_rpc(
            &cfg,
            IngestRequest {
                source_kind: SourceKind::Chat,
                source_id: "slack:#attachment-only".into(),
                owner: "alice".into(),
                tags: vec![],
                payload: json!({
                    "platform": "slack",
                    "channel_label": "#eng",
                    "messages": [
                        {
                            "author": "alice",
                            "timestamp": "2026-05-17T19:30:00Z",
                            "text": "   "
                        },
                        {
                            "author": "bob",
                            "timestamp": "2026-05-17T19:31:00Z",
                            "text": "here is the plan"
                        }
                    ]
                }),
            },
        )
        .await
        .expect("an empty message is dropped, not a batch failure");
        assert!(
            !outcome.value.chunk_ids.is_empty(),
            "the surviving message must still be written"
        );
    }

    /// An ingest is a write, so a driver without the family is refused rather
    /// than answered with zeros.
    ///
    /// The counts have no way to say "nothing was handed over": zero written
    /// and zero dropped is what a successful ingest of nothing looks like too,
    /// so degrading here would report content dropped on the floor as a
    /// success. `FixedDiagnostics` advertises `Capabilities::all()` while
    /// serving no `Ingest` accessor, which also pins that the refusal keys off
    /// the accessor and not off the advertised set.
    #[tokio::test]
    async fn ingest_refuses_a_driver_that_does_not_serve_the_ingest_family() {
        let (_tmp, cfg) = test_config();
        bind_diagnostics(&cfg, Default::default(), Default::default());

        let err = ingest_rpc(
            &cfg,
            IngestRequest {
                source_kind: SourceKind::Chat,
                source_id: "slack:#no-ingest".into(),
                owner: "alice".into(),
                tags: vec![],
                payload: json!({
                    "platform": "slack",
                    "channel_label": "#eng",
                    "messages": [{ "author": "alice", "text": "anything at all" }],
                }),
            },
        )
        .await
        .unwrap_err();
        assert!(
            err.contains("does not serve Ingest"),
            "the refusal must name the missing family: {err}"
        );
        assert!(
            err.contains("fixed-diagnostics"),
            "the refusal must name the driver that refused: {err}"
        );
    }

    #[tokio::test]
    async fn ingest_rpc_rejects_invalid_document_payload() {
        let (_tmp, cfg) = test_config();
        let err = ingest_rpc(
            &cfg,
            IngestRequest {
                source_kind: SourceKind::Document,
                source_id: "doc-invalid".into(),
                owner: String::new(),
                tags: vec![],
                payload: json!({"title": "Missing body"}),
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("invalid document payload"));
    }

    #[tokio::test]
    async fn list_chunks_rejects_unknown_source_kind() {
        let (_tmp, cfg) = test_config();
        let err = list_chunks_rpc(
            &cfg,
            ListChunksRequest {
                source_kind: Some("nonsense".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("unknown source kind: nonsense"));
    }

    /// An id the driver cannot resolve is `Ok(None)`, never an error — and the
    /// same is true of a driver with no chunk tier at all, which is why one
    /// test covers both. The two cases are indistinguishable to a caller by
    /// design: "no such chunk" is the honest answer to either.
    #[tokio::test]
    async fn get_chunk_returns_none_for_missing_id() {
        let (_tmp, cfg) = test_config();
        bind_diagnostics(&cfg, Default::default(), Default::default());
        let outcome = get_chunk_rpc(
            &cfg,
            GetChunkRequest {
                id: "missing-chunk".into(),
            },
        )
        .await
        .unwrap();
        assert!(outcome.value.chunk.is_none());
    }

    /// #1574 §4b: `backfill_status_rpc` reports what the driver says is
    /// queued for the backfill kind, and a non-zero count forces
    /// `in_progress` so the modal stays open.
    ///
    /// The empty case now asserts `in_progress` too. It could not before: the
    /// flag was a process-global that parallel tests shared. It comes from the
    /// bound driver now, so it is this test's to set.
    ///
    /// Ready + running, and deliberately not `total - done`: a backfill job
    /// that failed is finished with, and counting it as pending would leave
    /// the modal open forever.
    #[tokio::test]
    async fn backfill_status_reports_the_drivers_pending_count() {
        use crate::openhuman::memory::api::provider::types::QueueStats;

        let (_tmp, cfg) = test_config();

        bind_diagnostics(&cfg, Default::default(), QueueStats::default());
        let s0 = backfill_status_rpc(&cfg).await.unwrap().value;
        assert_eq!(s0.pending_jobs, 0, "idle space has no pending backfill");

        bind_diagnostics(
            &cfg,
            Default::default(),
            QueueStats {
                ready: 1,
                running: 2,
                // Neither of these is pending work.
                done: 7,
                failed: 3,
                ..Default::default()
            },
        );
        let s1 = backfill_status_rpc(&cfg).await.unwrap().value;
        assert_eq!(
            s1.pending_jobs, 3,
            "ready + running is what is still to do; done and failed are not"
        );
        assert!(s1.in_progress, "pending>0 forces in_progress=true");
    }

    /// The backfill flag is the driver's answer, not the host's engine static.
    ///
    /// This is the gap the counts cannot express: a backfill chain re-enqueues
    /// itself, so between one link settling and the next being written there is
    /// an instant with nothing ready, nothing running, and the work unfinished.
    /// A poll that trusted the counts alone closes the re-embed modal there.
    ///
    /// It has to come from the driver rather than
    /// `tinymemory_core::queue::backfill_in_progress()`, because re-embedding
    /// runs in the module and a `cdylib` has its own statics — the host-linked
    /// copy reads `false` forever on that path, which is worse than coarse.
    #[tokio::test]
    async fn backfill_status_reports_the_drivers_flag_when_the_counts_are_empty() {
        use crate::openhuman::memory::api::provider::types::QueueStats;

        let (_tmp, cfg) = test_config();

        let driver = std::sync::Arc::new(
            crate::openhuman::memory::binding::FixedDiagnostics::new(
                Default::default(),
                QueueStats::default(),
            )
            .backfilling(),
        );
        crate::openhuman::memory::binding::install_for_test(
            &cfg.workspace_dir,
            &cfg.subsystems.memory,
            driver as std::sync::Arc<dyn crate::openhuman::memory::api::provider::MemoryProvider>,
        );

        let status = backfill_status_rpc(&cfg).await.unwrap().value;
        assert_eq!(
            status.pending_jobs, 0,
            "precondition: the counts say the queue is empty"
        );
        assert!(
            status.in_progress,
            "and the driver still says a backfill is running, which is the whole point"
        );
    }

    // ── pipeline_status / set_enabled (#1856 Part 1) ─────────────────────

    /// `derive_pipeline_status` precedence is locked in here so the UI can
    /// rely on the wire status string without re-deriving it from the raw
    /// counters.
    #[test]
    fn latest_quarantine_reads_the_newest_copy_and_derives_resynced() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("memory_tree");
        std::fs::create_dir_all(&dir).unwrap();
        // No quarantine file: nothing to report.
        assert!(latest_quarantine(tmp.path(), 0).is_none());

        std::fs::write(dir.join("chunks.db.corrupt-20260101T000000Z"), b"old").unwrap();
        std::fs::write(dir.join("chunks.db.corrupt-20260827T070304Z"), b"new").unwrap();
        // Side files never match the main-file prefix.
        std::fs::write(dir.join("chunks.db-wal.corrupt-20261231T235959Z"), b"wal").unwrap();
        // Garbage that starts with the prefix but has no parsable stamp is ignored.
        std::fs::write(dir.join("chunks.db.corrupt-notastamp"), b"x").unwrap();

        let at = chrono::NaiveDate::from_ymd_opt(2026, 8, 27)
            .unwrap()
            .and_hms_opt(7, 3, 4)
            .unwrap()
            .and_utc()
            .timestamp_millis();

        // The rebuilt store is still empty: the notice stands.
        let pending = latest_quarantine(tmp.path(), 0).expect("newest quarantine");
        assert_eq!(pending.quarantined_at_ms, at);
        assert!(pending
            .quarantined_path
            .ends_with("chunks.db.corrupt-20260827T070304Z"));
        assert!(!pending.resynced);

        // Any chunk in the rebuilt store: the user re-synced, the notice retires.
        // Chunk *content* time is irrelevant here: restored history predates the
        // quarantine forever, which is exactly why this is not a timestamp test.
        let done = latest_quarantine(tmp.path(), 1).expect("newest quarantine");
        assert!(done.resynced);
    }

    #[test]
    fn derive_pipeline_status_precedence_matches_spec() {
        use crate::openhuman::memory::tree::health::{DegradedState, FailureCode, PipelineFailure};
        use tinymemory_api::host::SchedulerGateMode;

        let healthy = DegradedState::default();
        let recall_degraded = DegradedState {
            semantic_recall: true,
            structure: false,
            storage: false,
            cause: Some(PipelineFailure::new(FailureCode::EmbeddingsUnconfigured)),
        };
        let structure_degraded = DegradedState {
            semantic_recall: false,
            structure: true,
            storage: false,
            cause: Some(PipelineFailure::new(FailureCode::ExtractionTimeout)),
        };
        let storage_degraded = DegradedState {
            semantic_recall: false,
            structure: false,
            storage: true,
            cause: Some(PipelineFailure::new(FailureCode::StorageUnavailable)),
        };

        // Args: (is_paused, mode, is_syncing, failed, failed_unrecoverable,
        //        total_chunks, &degraded, queue_idle_ms).

        // paused beats everything else (even degradation)
        let (s, reason) = derive_pipeline_status(
            true,
            SchedulerGateMode::Off,
            true,
            5,
            5,
            100,
            &recall_degraded,
            None,
        );
        assert_eq!(s, "paused");
        assert!(reason.unwrap().contains("off"));

        // paused still beats a storage failure (user explicitly stood the
        // worker down; the flag won't be freshly set anyway).
        let (s, _) = derive_pipeline_status(
            true,
            SchedulerGateMode::Off,
            false,
            0,
            0,
            0,
            &storage_degraded,
            None,
        );
        assert_eq!(s, "paused", "paused beats storage");

        // storage failure → error, and it fires even with ZERO chunks (unlike
        // recall/structure degradation, which is content-relative) — a dead
        // disk is broken regardless of how much content exists.
        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            0, // no chunks — must still surface
            &storage_degraded,
            None,
        );
        assert_eq!(
            s, "error",
            "storage failure is a hard error at any chunk count"
        );
        assert!(reason.unwrap().contains("storage"));

        // storage outranks transient-failed degradation too.
        let (s, _) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            true,
            3,
            0,
            100,
            &storage_degraded,
            None,
        );
        assert_eq!(s, "error", "storage beats transient-degraded");

        // error beats degraded / syncing / running / idle — but ONLY for
        // unrecoverable failures (#3365).
        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            true,
            2,
            2, // both failures unrecoverable
            100,
            &recall_degraded,
            None,
        );
        assert_eq!(s, "error");
        assert!(reason.unwrap().contains("unrecoverable"));

        // #3365: transient-only failures (failed > 0, none unrecoverable) do NOT
        // escalate to error — they self-heal via auto-requeue, so they surface
        // as `degraded` ("retrying"), beating syncing/running.
        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            true,
            3,
            0,
            100,
            &healthy,
            None,
        );
        assert_eq!(s, "degraded", "transient failures must not read as error");
        assert!(reason.unwrap().contains("3 job(s) failed, retrying"));

        // #002: degraded beats syncing / running / idle (but loses to paused/error)
        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            true, // syncing
            0,
            0,
            100,
            &recall_degraded,
            None,
        );
        assert_eq!(s, "degraded", "degraded must beat syncing");
        assert!(reason.unwrap().contains("semantic recall disabled"));

        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            100,
            &structure_degraded,
            None,
        );
        assert_eq!(s, "degraded");
        assert!(reason.unwrap().contains("wiki structure incomplete"));

        // syncing beats running / idle (when healthy)
        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            true,
            0,
            0,
            100,
            &healthy,
            None,
        );
        assert_eq!(s, "syncing");
        assert!(reason.is_none());

        // running when chunks exist but nothing in flight
        let (s, _) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            100,
            &healthy,
            None,
        );
        assert_eq!(s, "running");

        // idle when the store is empty and nothing is in flight (transient
        // failures with no content don't manufacture a `degraded`).
        let (s, _) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            2,
            0,
            0,
            &healthy,
            None,
        );
        assert_eq!(s, "idle");
    }

    /// #5324: a queue that accepts work but never drains it must report
    /// `degraded`, not the `running`/`idle` that made a month-long outage look
    /// healthy. Pins the threshold boundary and the full precedence chain.
    #[test]
    fn stalled_queue_degrades_instead_of_reading_healthy() {
        use crate::openhuman::memory::tree::health::{DegradedState, FailureCode, PipelineFailure};
        use tinymemory_api::host::SchedulerGateMode;

        let healthy = DegradedState::default();
        let stalled = Some(QUEUE_STALL_THRESHOLD_MS);
        let just_under = Some(QUEUE_STALL_THRESHOLD_MS - 1);

        // The regression itself: chunks exist, nothing failed, nothing running
        // — previously "running", which is what let the outage hide.
        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            100,
            &healthy,
            stalled,
        );
        assert_eq!(s, "degraded", "a stalled queue must not read as running");
        assert!(reason.unwrap().contains("has not completed any job"));

        // NOT gated on total_chunks: a queue that never drained has no chunks,
        // and that case must not read as `idle`.
        let (s, _) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            0,
            &healthy,
            stalled,
        );
        assert_eq!(s, "degraded", "empty-but-stalled must not read as idle");

        // Boundary: one millisecond under the threshold is still healthy, so a
        // merely slow flush window can't trip it.
        let (s, _) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            100,
            &healthy,
            just_under,
        );
        assert_eq!(s, "running", "under the threshold stays healthy");

        // `None` (no ready jobs, or an unreadable metric) never manufactures a
        // degraded verdict.
        let (s, _) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            0,
            0,
            100,
            &healthy,
            None,
        );
        assert_eq!(s, "running", "absent metric must not claim a stall");

        // Precedence: paused and error both outrank the stall — a typed
        // unrecoverable failure is the more specific, more actionable answer.
        let (s, _) = derive_pipeline_status(
            true,
            SchedulerGateMode::Off,
            false,
            0,
            0,
            100,
            &healthy,
            stalled,
        );
        assert_eq!(s, "paused", "paused beats stalled");

        let (s, reason) = derive_pipeline_status(
            false,
            SchedulerGateMode::Auto,
            false,
            1,
            1,
            100,
            &healthy,
            stalled,
        );
        assert_eq!(s, "error", "unrecoverable failure beats stalled");
        assert!(reason.unwrap().contains("unrecoverable"));

        // Sanity: the budget-exhausted failure this issue is about is indeed
        // classified unrecoverable, so it lands in the `error` branch above and
        // carries its own remediation key.
        let budget = PipelineFailure::new(FailureCode::BudgetExhausted);
        assert!(budget.is_unrecoverable());
        assert_eq!(
            budget.remediation_key,
            "memory.health.remediation.budget_exhausted"
        );
    }

    /// #5324: `queue_idle_ms` measures idle time, not backlog depth. Pins the
    /// two shapes that must NOT be reported as stalled, both of which a
    /// backlog-age metric would have flagged — and both of which describe the
    /// heavy users this issue is about.
    /// One queue snapshot, spelled out.
    ///
    /// These read as SQL fixtures before the driver owned the query. What
    /// `queue_idle_ms` decides has never depended on the rows, only on the
    /// three numbers below, so the tests say those directly now. Which rows
    /// produce which numbers is the driver's rule and is pinned in the
    /// driver's own suite — notably that deferred work counts as `ready`
    /// without becoming `eligible_now`, the distinction the third case here
    /// relies on.
    fn queue(
        eligible_now: u64,
        last_completed_ms: Option<i64>,
        oldest_eligible_ms: Option<i64>,
    ) -> crate::openhuman::memory::api::provider::types::QueueStats {
        crate::openhuman::memory::api::provider::types::QueueStats {
            eligible_now,
            last_completed_ms,
            oldest_eligible_ms,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn queue_idle_ms_ignores_deep_but_draining_and_deferred_backlogs() {
        let now = 1_800_000_000_000_i64;
        let long_ago = now - 48 * 60 * 60 * 1000;

        // Nothing queued at all ⇒ not stalled (an empty queue is done, not stuck).
        assert_eq!(queue_idle_ms(&queue(0, None, None), now), None);

        // A deep backlog whose oldest eligible job arrived 48h ago, and which
        // has never settled anything. A naive backlog-age metric reads 48h and
        // cries "stalled" — and here it is right, because a queue that has
        // never settled a job IS stalled.
        assert!(
            queue_idle_ms(&queue(3, None, Some(long_ago)), now).unwrap()
                >= QUEUE_STALL_THRESHOLD_MS,
            "a queue that has never settled a job IS stalled"
        );

        // Same 48h-old backlog, but one job settled a minute ago — the
        // pipeline is draining. This is the shape a backlog-age metric gets
        // wrong, and it describes the heavy users this issue is about.
        let idle = queue_idle_ms(&queue(3, Some(now - 60_000), Some(long_ago)), now)
            .expect("work still queued");
        assert!(
            idle < QUEUE_STALL_THRESHOLD_MS,
            "a deep but draining backlog must not read as stalled (idle={idle}ms)"
        );

        // Wholly deferred: every job is backing off, so nothing is runnable.
        // Asleep on purpose is not stuck.
        assert_eq!(
            queue_idle_ms(&queue(0, Some(long_ago), None), now),
            None,
            "wholly-deferred work is asleep, not stalled"
        );
    }

    /// #5324 regression (CodeRabbit/Codex): a queue that drained everything,
    /// sat quiet for two days, then received one fresh eligible job must start
    /// the idle clock at the NEW job's arrival — not inherit the ancient
    /// completion. The prior `last_settled_ms.or(oldest_eligible_ms)` picked
    /// the stale 48h-old settle and reported `degraded` the instant new work
    /// appeared, before the worker had any chance to touch it.
    #[tokio::test]
    async fn queue_idle_ms_starts_from_fresh_work_not_ancient_completion() {
        let now = 1_800_000_000_000_i64;
        let long_ago = now - 48 * 60 * 60 * 1000;
        let just_now = now - 60_000;

        // The queue settled its last job 48h ago and went quiet; one fresh
        // eligible job arrived a minute ago.
        let idle = queue_idle_ms(&queue(1, Some(long_ago), Some(just_now)), now)
            .expect("fresh work is waiting");
        assert!(
            idle < QUEUE_STALL_THRESHOLD_MS,
            "freshly-enqueued work must start its own idle window, not inherit a 48h-old \
             completion (idle={idle}ms)"
        );
        assert_eq!(
            idle,
            now - just_now,
            "the idle clock starts at the new job's arrival, not the stale settle"
        );
    }

    /// One failure as the driver reports it.
    ///
    /// These used to plant rows and read the answer back through a `SELECT`.
    /// Which rows the driver reports is the driver's rule and is pinned in the
    /// driver's own suite; what the host decides to *do* with a reported
    /// failure is the rule below, and it depends on nothing but these three
    /// values.
    fn reported_failure(
        reason: &str,
        failed_at_ms: Option<i64>,
        last_success_ms: Option<i64>,
    ) -> crate::openhuman::memory::api::provider::types::QueueFailure {
        crate::openhuman::memory::api::provider::types::QueueFailure {
            reason: reason.to_string(),
            class: Some("unrecoverable".to_string()),
            completed_at_ms: failed_at_ms,
            last_success_ms,
        }
    }

    /// The active production defect: a signed-in user was told "No embeddings
    /// credentials found. Log in to OpenHuman" because a batch of `auth_missing`
    /// jobs had failed 27 days earlier and, being unrecoverable, was never
    /// retried. The queue had been completing jobs the whole time since.
    ///
    /// A failure the pipeline has already worked past is not the current
    /// blocking cause, so no remediation is surfaced for it.
    #[test]
    fn blocking_cause_is_withheld_once_the_queue_has_succeeded_since() {
        let failed_at = 1_800_000_000_000_i64;
        let succeeded_after = failed_at + 27 * 24 * 60 * 60 * 1000;

        assert!(
            blocking_cause(&reported_failure(
                "auth_missing",
                Some(failed_at),
                Some(succeeded_after)
            ))
            .is_none(),
            "a month-old auth failure the queue has since worked past must not be \
             presented as the user's current problem"
        );
    }

    /// The other half of the same rule: a failure with no successful settle
    /// after it IS the current blocking cause and must still surface, otherwise
    /// the fix would silence the diagnosis it exists to deliver.
    #[test]
    fn blocking_cause_surfaces_when_nothing_has_succeeded_since() {
        use crate::openhuman::memory::tree::health::{FailureClass, FailureCode};

        let succeeded_before = 1_800_000_000_000_i64;
        let failed_after = succeeded_before + 60_000;

        let failure = blocking_cause(&reported_failure(
            "budget_exhausted",
            Some(failed_after),
            Some(succeeded_before),
        ))
        .expect("a failure with no success after it is the live cause");
        assert_eq!(failure.code, FailureCode::BudgetExhausted);
        assert_eq!(failure.class, FailureClass::Unrecoverable);
        assert_eq!(
            failure.remediation_key,
            "memory.health.remediation.budget_exhausted"
        );
    }

    /// A queue that has never completed anything has no watermark to compare
    /// against, so the failure stands — this is the "broken from the first
    /// sync" shape, where the diagnosis matters most.
    #[test]
    fn blocking_cause_surfaces_when_the_queue_has_never_succeeded() {
        let failure = blocking_cause(&reported_failure(
            "auth_invalid",
            Some(1_800_000_000_000_i64),
            None,
        ))
        .expect("no successful settle exists to supersede this failure");
        assert_eq!(
            failure.remediation_key,
            "memory.health.remediation.auth_invalid"
        );
    }

    /// A settle on the same millisecond as the failure does NOT supersede it.
    ///
    /// The comparison is strictly `>`, and it has to be: `completed_at_ms` is
    /// stamped on failure as well as success, so a job that fails and a job
    /// that succeeds within the same millisecond are ordered by nothing. `>=`
    /// would resolve that tie by hiding the failure, which is the direction
    /// that loses a real diagnosis.
    #[test]
    fn a_settle_in_the_same_millisecond_does_not_supersede_the_failure() {
        let at = 1_800_000_000_000_i64;
        assert!(
            blocking_cause(&reported_failure("auth_invalid", Some(at), Some(at))).is_some(),
            "a success that cannot be shown to be later must not withhold the diagnosis"
        );
    }

    /// A failure carrying no completion time has nothing to compare against,
    /// so it surfaces unconditionally rather than being withheld by a
    /// watermark it cannot be ordered against.
    #[test]
    fn an_untimestamped_failure_surfaces_regardless_of_the_watermark() {
        let succeeded_at = 1_800_000_000_000_i64;
        assert!(
            blocking_cause(&reported_failure("auth_invalid", None, Some(succeeded_at))).is_some(),
            "without a failure timestamp there is no ordering, and withholding would \
             hide a live cause on a guess"
        );
    }

    /// On a fresh workspace the panel must report `idle` with zero
    /// counters — the UI uses this to swap the loading skeleton for a
    /// "no memory yet" state.
    #[tokio::test]
    async fn pipeline_status_returns_idle_for_empty_store() {
        // #002: the degraded flags are process-global; reset+serialise so a
        // parallel test (factory None-path, extract transport-fail) can't leak
        // a "degraded" signal into this fresh-workspace assertion.
        let _g = crate::openhuman::memory::tree::health::test_guard();
        let (_tmp, cfg) = test_config();
        // An empty driver, bound explicitly. Without a binding installed this
        // resolves the real one, which means loading the compiled module — and
        // in a test process that blocks rather than failing.
        bind_diagnostics(&cfg, Default::default(), Default::default());
        let out = pipeline_status_rpc(&cfg).await.unwrap().value;
        assert_eq!(out.status, "idle");
        assert_eq!(out.total_chunks, 0);
        assert_eq!(out.last_sync_ms, 0);
        assert_eq!(out.pipeline_jobs.ready, 0);
        assert_eq!(out.pipeline_jobs.running, 0);
        assert_eq!(out.pipeline_jobs.failed, 0);
        assert!(!out.is_syncing);
        assert!(!out.is_paused);
        assert_eq!(out.wiki_size_bytes, 0, "no content dir yet");
        assert!(out.reason.is_none());
    }

    /// When the scheduler gate is `off`, the aggregated status flips to
    /// `paused` regardless of the rest of the signals. This is the
    /// invariant the toggle relies on.
    #[tokio::test]
    async fn pipeline_status_reflects_paused_when_scheduler_off() {
        use tinymemory_api::host::SchedulerGateMode;

        let (_tmp, mut cfg) = test_config();
        cfg.scheduler_gate.mode = SchedulerGateMode::Off;
        bind_diagnostics(&cfg, Default::default(), Default::default());
        let out = pipeline_status_rpc(&cfg).await.unwrap().value;
        assert_eq!(out.status, "paused");
        assert!(out.is_paused);
        let reason = out.reason.expect("paused must carry a reason");
        assert!(reason.contains("off"), "reason should name the mode");
    }

    /// `pipeline_status` renders the aggregates the driver reports, and
    /// derives a terminal status from them.
    ///
    /// This used to ingest a document and assert the counters moved. That
    /// half — an ingest raising the chunk count — is the driver's, and is
    /// pinned in the driver's conformance suite against a real store. What is
    /// the host's, and what this pins, is that the reported numbers reach the
    /// wire unchanged and that a populated, idle store reads as terminal
    /// rather than syncing.
    #[tokio::test]
    async fn pipeline_status_renders_the_drivers_chunk_aggregates() {
        use crate::openhuman::memory::api::provider::types::{QueueStats, StoreStats};

        // #002: reset+serialise the process-global degraded flags so this
        // "running" assertion isn't flipped to "degraded" by a parallel test.
        let _g = crate::openhuman::memory::tree::health::test_guard();
        let (_tmp, cfg) = test_config();

        let ingested_at = 1_800_000_000_000_i64;
        bind_diagnostics(
            &cfg,
            StoreStats {
                chunks: 4,
                chunks_with_structure: 1,
                most_recent_chunk_ms: Some(ingested_at),
            },
            QueueStats::default(),
        );

        let out = pipeline_status_rpc(&cfg).await.unwrap().value;
        assert_eq!(out.total_chunks, 4, "the driver's count reaches the wire");
        assert_eq!(
            out.last_sync_ms, ingested_at,
            "and so does its newest chunk's timestamp"
        );
        assert_eq!(
            out.extraction_coverage,
            Some(0.25),
            "coverage is the pair the driver reported, divided once"
        );
        // Provider availability differs between local and CI harnesses, so a
        // populated store may read as fully running or as degraded because
        // semantic recall or wiki structure was skipped. Both are terminal,
        // non-syncing states and both preserve the aggregates above.
        match out.status.as_str() {
            "running" => assert!(out.reason.is_none()),
            "degraded" => {
                let reason = out.reason.as_deref().unwrap_or_default();
                assert!(
                    reason.contains("semantic recall disabled")
                        || reason.contains("wiki structure incomplete"),
                    "degraded status should explain recall or structure loss: {:?}",
                    out.reason
                );
            }
            other => panic!("expected running or degraded for a populated store, got {other}"),
        }
        assert!(!out.is_syncing);
    }

    /// `set_enabled` flips the persisted scheduler-gate mode and reports
    /// `changed=true`; calling it again with the same value is a no-op
    /// reporting `changed=false`. Uses an isolated `config_path` under
    /// the workspace tempdir so `config.save()` doesn't touch the
    /// host's real ~/.openhuman directory.
    #[tokio::test]
    async fn set_enabled_toggles_scheduler_gate_mode() {
        use tinymemory_api::host::SchedulerGateMode;

        let (tmp, mut cfg) = test_config();
        // Pin config_path inside the tempdir so `save()` stays sandboxed.
        cfg.config_path = tmp.path().join("config.toml");

        assert_eq!(cfg.scheduler_gate.mode, SchedulerGateMode::Auto);

        let off = set_enabled_rpc(&mut cfg, SetEnabledRequest { enabled: false })
            .await
            .unwrap()
            .value;
        assert!(!off.enabled);
        assert!(off.changed);
        assert_eq!(off.mode, "off");
        assert_eq!(cfg.scheduler_gate.mode, SchedulerGateMode::Off);

        // Calling with the same value must report no-op.
        let again = set_enabled_rpc(&mut cfg, SetEnabledRequest { enabled: false })
            .await
            .unwrap()
            .value;
        assert!(!again.changed, "duplicate toggle must be a no-op");

        // Flip back.
        let on = set_enabled_rpc(&mut cfg, SetEnabledRequest { enabled: true })
            .await
            .unwrap()
            .value;
        assert!(on.enabled);
        assert!(on.changed);
        assert_eq!(on.mode, "auto");
        assert_eq!(cfg.scheduler_gate.mode, SchedulerGateMode::Auto);
    }
}
