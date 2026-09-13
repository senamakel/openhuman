//! `MemoryEntities`, `MemoryGraph`, and `MemoryDiff` forwarding for
//! [`ModuleMemoryProvider`].

use async_trait::async_trait;
use tinymemory_api::error::MemoryError;
use tinymemory_api::provider::types::{
    ChunkEntityOccurrence, DiffReport, EntityHit, EntityOccurrence, SnapshotRef,
};
use tinymemory_api::provider::{MemoryDiff, MemoryEntities, MemoryGraph};
use tinymemory_api::types::{GraphRelationRecord, MemoryKvRecord};
use tinymemory_bus::names::methods;

use super::provider::{module_call, ModuleMemoryProvider};

#[async_trait]
impl MemoryEntities for ModuleMemoryProvider {
    async fn entities(
        &self,
        namespace: &str,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityHit>, MemoryError> {
        module_call!(
            self,
            "entities",
            methods::ENTITIES,
            (namespace, query.map(str::to_string), limit)
        )
    }
    async fn entity_edges(
        &self,
        namespace: &str,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        module_call!(
            self,
            "entity_edges",
            methods::ENTITY_EDGES,
            (namespace, entity_id, limit)
        )
    }
    async fn touch_entities(
        &self,
        namespace: &str,
        entity_ids: &[String],
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "touch_entities",
            methods::TOUCH_ENTITIES,
            (namespace, entity_ids.to_vec())
        )
    }
    async fn top_entities(
        &self,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<EntityOccurrence>, MemoryError> {
        module_call!(self, "top_entities", methods::TOP_ENTITIES, (kind, limit))
    }
    async fn chunk_entities(
        &self,
        chunk_ids: &[String],
        kinds: Option<&[String]>,
    ) -> Result<Vec<ChunkEntityOccurrence>, MemoryError> {
        module_call!(
            self,
            "chunk_entities",
            methods::CHUNK_ENTITIES,
            (chunk_ids, kinds)
        )
    }
    async fn entity_chunk_ids(
        &self,
        entity_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, MemoryError> {
        module_call!(
            self,
            "entity_chunk_ids",
            methods::ENTITY_CHUNK_IDS,
            (entity_id, limit)
        )
    }
}

#[async_trait]
impl MemoryGraph for ModuleMemoryProvider {
    async fn kv_get(
        &self,
        namespace: Option<&str>,
        key: &str,
    ) -> Result<Option<MemoryKvRecord>, MemoryError> {
        module_call!(
            self,
            "kv_get",
            methods::KV_GET,
            (namespace.map(str::to_string), key)
        )
    }
    async fn kv_put(
        &self,
        namespace: Option<&str>,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), MemoryError> {
        module_call!(
            self,
            "kv_put",
            methods::KV_PUT,
            (namespace.map(str::to_string), key, value)
        )
    }
    async fn kv_delete(&self, namespace: Option<&str>, key: &str) -> Result<bool, MemoryError> {
        module_call!(
            self,
            "kv_delete",
            methods::KV_DELETE,
            (namespace.map(str::to_string), key)
        )
    }
    async fn kv_list(
        &self,
        namespace: Option<&str>,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryKvRecord>, MemoryError> {
        module_call!(
            self,
            "kv_list",
            methods::KV_LIST,
            (
                namespace.map(str::to_string),
                prefix.map(str::to_string),
                limit
            )
        )
    }
    async fn relations(
        &self,
        namespace: Option<&str>,
        subject: Option<&str>,
        predicate: Option<&str>,
        limit: usize,
    ) -> Result<Vec<GraphRelationRecord>, MemoryError> {
        module_call!(
            self,
            "relations",
            methods::RELATIONS,
            (
                namespace.map(str::to_string),
                subject.map(str::to_string),
                predicate.map(str::to_string),
                limit
            )
        )
    }
    async fn put_relation(&self, relation: GraphRelationRecord) -> Result<(), MemoryError> {
        module_call!(self, "put_relation", methods::PUT_RELATION, (relation,))
    }
}

#[async_trait]
impl MemoryDiff for ModuleMemoryProvider {
    async fn capture_snapshot(&self, source_id: &str) -> Result<SnapshotRef, MemoryError> {
        module_call!(
            self,
            "capture_snapshot",
            methods::CAPTURE_SNAPSHOT,
            (source_id,)
        )
    }
    async fn snapshots(
        &self,
        source_id: &str,
        limit: usize,
    ) -> Result<Vec<SnapshotRef>, MemoryError> {
        module_call!(self, "snapshots", methods::SNAPSHOTS, (source_id, limit))
    }
    async fn diff(
        &self,
        source_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<DiffReport, MemoryError> {
        module_call!(
            self,
            "diff",
            methods::DIFF,
            (source_id, from.map(str::to_string), to)
        )
    }
}
