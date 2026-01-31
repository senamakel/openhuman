import { invoke } from "@tauri-apps/api/core";
import type {
  Entity,
  EntityRelation,
  EntitySearchResult,
  EntityType,
  EntitySource,
  RelationType,
  EntityRust,
  EntityRelationRust,
  EntitySearchResultRust,
} from "./types";
import {
  fromRustEntity,
  fromRustRelation,
  fromRustSearchResult,
  toRustEntity,
  toRustRelation,
} from "./types";

/**
 * EntityManager wraps Tauri commands for the entity relationship database.
 *
 * Provides a typed interface for CRUD operations on entities, relations,
 * and tags in the platform graph.
 */
export class EntityManager {
  private initialized = false;

  /** Initialize the entity database */
  async init(): Promise<void> {
    await invoke("ai_entity_db_init");
    this.initialized = true;
  }

  /** Ensure the database is initialized */
  private async ensureInit(): Promise<void> {
    if (!this.initialized) await this.init();
  }

  /**
   * Upsert an entity (insert or update).
   * If an entity with the same id or same source+sourceId exists, it's updated.
   */
  async upsert(entity: Entity): Promise<void> {
    await this.ensureInit();
    await invoke("ai_entity_upsert", { entity: toRustEntity(entity) });
  }

  /** Get an entity by ID */
  async get(id: string): Promise<Entity | null> {
    await this.ensureInit();
    const result = await invoke<EntityRust | null>("ai_entity_get", { id });
    return result ? fromRustEntity(result) : null;
  }

  /** Get an entity by source system reference */
  async getBySource(source: EntitySource, sourceId: string): Promise<Entity | null> {
    await this.ensureInit();
    const result = await invoke<EntityRust | null>("ai_entity_get_by_source", {
      source,
      sourceId,
    });
    return result ? fromRustEntity(result) : null;
  }

  /** Full-text search on entities */
  async search(
    query: string,
    types?: EntityType[],
    limit = 20,
  ): Promise<EntitySearchResult[]> {
    await this.ensureInit();
    const results = await invoke<EntitySearchResultRust[]>("ai_entity_search", {
      query,
      types: types ?? null,
      limit,
    });
    return results.map(fromRustSearchResult);
  }

  /** List entities by type with pagination */
  async list(
    entityType: EntityType,
    offset = 0,
    limit = 50,
  ): Promise<Entity[]> {
    await this.ensureInit();
    const results = await invoke<EntityRust[]>("ai_entity_list", {
      entityType,
      offset,
      limit,
    });
    return results.map(fromRustEntity);
  }

  /** Delete an entity and cascade relations/tags */
  async delete(id: string): Promise<void> {
    await this.ensureInit();
    await invoke("ai_entity_delete", { id });
  }

  /** Add a relationship between entities */
  async addRelation(relation: EntityRelation): Promise<void> {
    await this.ensureInit();
    await invoke("ai_entity_add_relation", {
      relation: toRustRelation(relation),
    });
  }

  /**
   * Get relationships for an entity.
   * @param entityId The entity to get relations for
   * @param direction "from" = outgoing, "to" = incoming, "both" = all (default)
   * @param relationType Optional filter by relation type
   */
  async getRelations(
    entityId: string,
    direction?: "from" | "to" | "both",
    relationType?: RelationType,
  ): Promise<EntityRelation[]> {
    await this.ensureInit();
    const results = await invoke<EntityRelationRust[]>("ai_entity_get_relations", {
      entityId,
      direction: direction ?? null,
      relationType: relationType ?? null,
    });
    return results.map(fromRustRelation);
  }

  /** Tag an entity */
  async addTag(entityId: string, tag: string): Promise<void> {
    await this.ensureInit();
    await invoke("ai_entity_add_tag", { entityId, tag });
  }

  /** Remove a tag from an entity */
  async removeTag(entityId: string, tag: string): Promise<void> {
    await this.ensureInit();
    await invoke("ai_entity_remove_tag", { entityId, tag });
  }

  /** Find entities by tag, optionally filtered by type */
  async getByTag(tag: string, entityType?: EntityType): Promise<Entity[]> {
    await this.ensureInit();
    const results = await invoke<EntityRust[]>("ai_entity_get_by_tag", {
      tag,
      entityType: entityType ?? null,
    });
    return results.map(fromRustEntity);
  }
}
