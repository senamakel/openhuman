//! `MemoryDocuments` and `MemoryTree` forwarding for [`ModuleMemoryProvider`].

use async_trait::async_trait;
use tinymemory_api::chunks::Chunk;
use tinymemory_api::error::MemoryError;
use tinymemory_api::provider::types::SourceScope;
use tinymemory_api::provider::{
    MemoryDocuments, MemoryTree, RootSummary, SummaryContext, SummaryInput, SummaryOutput,
};
use tinymemory_api::tree::{
    IngestRequest, QueryResult, SummaryForest, TreeLeaf, TreeNode, TreeStatus,
};
use tinymemory_api::types::{
    NamespaceDocumentInput, NamespaceRetrievalContext, StoredMemoryDocument,
};
use tinymemory_bus::names::methods;

use super::provider::{module_call, ModuleMemoryProvider};

#[async_trait]
impl MemoryDocuments for ModuleMemoryProvider {
    async fn put_document(&self, input: NamespaceDocumentInput) -> Result<String, MemoryError> {
        module_call!(self, "put_document", methods::PUT_DOCUMENT, (input,))
    }
    async fn get_document(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StoredMemoryDocument>, MemoryError> {
        module_call!(
            self,
            "get_document",
            methods::GET_DOCUMENT,
            (namespace, key)
        )
    }
    async fn list_documents(
        &self,
        namespace: Option<&str>,
    ) -> Result<serde_json::Value, MemoryError> {
        module_call!(
            self,
            "list_documents",
            methods::LIST_DOCUMENTS,
            (namespace.map(str::to_string),)
        )
    }
    async fn list_namespaces(&self) -> Result<Vec<String>, MemoryError> {
        module_call!(self, "list_namespaces", methods::LIST_NAMESPACES, ())
    }
    async fn delete_document(
        &self,
        namespace: &str,
        document_id: &str,
    ) -> Result<serde_json::Value, MemoryError> {
        module_call!(
            self,
            "delete_document",
            methods::DELETE_DOCUMENT,
            (namespace, document_id)
        )
    }
    async fn clear_namespace(&self, namespace: &str) -> Result<(), MemoryError> {
        module_call!(
            self,
            "clear_namespace",
            methods::CLEAR_NAMESPACE,
            (namespace,)
        )
    }
    async fn query_documents(
        &self,
        namespace: &str,
        query: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        module_call!(
            self,
            "query_documents",
            methods::QUERY_DOCUMENTS,
            (namespace, query, limit)
        )
    }
    async fn recall_documents(
        &self,
        namespace: &str,
        limit: usize,
    ) -> Result<NamespaceRetrievalContext, MemoryError> {
        module_call!(
            self,
            "recall_documents",
            methods::RECALL_DOCUMENTS,
            (namespace, limit)
        )
    }
}

#[async_trait]
impl MemoryTree for ModuleMemoryProvider {
    async fn append(&self, request: IngestRequest) -> Result<(), MemoryError> {
        module_call!(self, "append", methods::APPEND, (request,))
    }
    async fn query_source(
        &self,
        namespace: &str,
        source_id: &str,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<Chunk>, MemoryError> {
        module_call!(
            self,
            "query_source",
            methods::QUERY_SOURCE,
            (namespace, source_id, limit, scope.cloned())
        )
    }
    async fn drill_down(&self, namespace: &str, node_id: &str) -> Result<QueryResult, MemoryError> {
        module_call!(
            self,
            "drill_down",
            methods::DRILL_DOWN,
            (namespace, node_id)
        )
    }
    async fn seal(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        module_call!(self, "seal", methods::SEAL, (namespace,))
    }
    async fn cascade(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        module_call!(self, "cascade", methods::CASCADE, (namespace,))
    }
    async fn summary_forest(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<SummaryForest, MemoryError> {
        module_call!(
            self,
            "summary_forest",
            methods::SUMMARY_FOREST,
            (limit, scope)
        )
    }

    async fn flush_source_tree(&self, source_scope: &str) -> Result<u64, MemoryError> {
        module_call!(
            self,
            "flush_source_tree",
            methods::FLUSH_SOURCE_TREE,
            (source_scope,)
        )
    }
    async fn recent_leaves(
        &self,
        limit: usize,
        scope: Option<&SourceScope>,
    ) -> Result<Vec<TreeLeaf>, MemoryError> {
        module_call!(
            self,
            "recent_leaves",
            methods::RECENT_LEAVES,
            (limit, scope)
        )
    }
    /// The one member here that costs a provider call rather than a store read,
    /// so it is also the one whose bus deadline could bind. It rides the
    /// default: the module clamps the fold to the `token_budget` this caller
    /// supplied, and a summariser that outruns the deadline is the same failure
    /// a caller must already handle — the contract puts the deterministic
    /// fallback on the *caller* and states that the driver never substitutes
    /// one, precisely so a fallback cannot be mistaken for a model's own work
    /// once it is in the tree. An `Ok` here is therefore always the model's
    /// text, or empty when there was nothing to fold; a model that errors,
    /// times out or refuses arrives as `Err`, never as a filled-in summary.
    async fn summarise(
        &self,
        inputs: &[SummaryInput],
        context: &SummaryContext,
    ) -> Result<SummaryOutput, MemoryError> {
        module_call!(self, "summarise", methods::SUMMARISE, (inputs, context))
    }
    /// The wire member is `RootSummaries`; the caps are in the signature on
    /// both sides, so the name carries only what distinguishes the call.
    async fn root_summaries_with_caps(
        &self,
        per_namespace_cap: usize,
        total_cap: usize,
    ) -> Result<Vec<RootSummary>, MemoryError> {
        module_call!(
            self,
            "root_summaries_with_caps",
            methods::ROOT_SUMMARIES,
            (per_namespace_cap, total_cap)
        )
    }

    // ── The runtime-tree and flavour doors ──────────────────────────────────
    //
    // The seven below are named through `tinymemory_bus::names::methods`
    // rather than as string literals, unlike their neighbours above. The
    // failure a literal invites is precisely the one this family is prone to:
    // a member the pinned artifact does not serve answers `Unsupported` at run
    // time, so a mistyped wire name is indistinguishable from a stale pin, and
    // both look like "the module is old". The constants make the typo a
    // compile error and leave `Unsupported` meaning only what it should.
    //
    // Every one of them is **defaulted** on the trait, which is what makes
    // forwarding them mandatory rather than optional: an override that is
    // missing here does not fail to compile, it silently inherits
    // `Err(Unsupported)` and the driver underneath is never asked.

    async fn runtime_buffer_write(
        &self,
        namespace: &str,
        content: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
        metadata: Option<serde_json::Value>,
    ) -> Result<String, MemoryError> {
        module_call!(
            self,
            "runtime_buffer_write",
            methods::RUNTIME_BUFFER_WRITE,
            (namespace, content, timestamp, metadata)
        )
    }

    async fn runtime_read_node(
        &self,
        namespace: &str,
        node_id: &str,
    ) -> Result<Option<TreeNode>, MemoryError> {
        module_call!(
            self,
            "runtime_read_node",
            methods::RUNTIME_READ_NODE,
            (namespace, node_id)
        )
    }

    async fn runtime_read_children(
        &self,
        namespace: &str,
        parent_id: &str,
    ) -> Result<Vec<TreeNode>, MemoryError> {
        module_call!(
            self,
            "runtime_read_children",
            methods::RUNTIME_READ_CHILDREN,
            (namespace, parent_id)
        )
    }

    async fn runtime_tree_status(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        module_call!(
            self,
            "runtime_tree_status",
            methods::RUNTIME_TREE_STATUS,
            (namespace,)
        )
    }

    /// Long-running on [`Self::summarise`]'s terms — the fold is one provider
    /// call per hour group drained, plus the propagation above them — and it
    /// rides the default deadline for the same reason: the module clamps each
    /// fold to the level's own token budget, and a summariser that outruns the
    /// deadline is the failure every caller of this surface already handles.
    async fn runtime_summarize(
        &self,
        namespace: &str,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<TreeNode>, MemoryError> {
        module_call!(
            self,
            "runtime_summarize",
            methods::RUNTIME_SUMMARIZE,
            (namespace, timestamp)
        )
    }

    /// As [`Self::runtime_summarize`], over every level of the tree at once.
    async fn runtime_rebuild(&self, namespace: &str) -> Result<TreeStatus, MemoryError> {
        module_call!(
            self,
            "runtime_rebuild",
            methods::RUNTIME_REBUILD,
            (namespace,)
        )
    }

    async fn flavour_profile(&self, scope: &str) -> Result<Option<String>, MemoryError> {
        module_call!(self, "flavour_profile", methods::FLAVOUR_PROFILE, (scope,))
    }
}
