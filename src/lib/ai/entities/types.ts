/** Entity types in the platform graph */
export type EntityType =
  | "contact"
  | "chat"
  | "message"
  | "email"
  | "wallet"
  | "token"
  | "transaction";

/** Source system for an entity */
export type EntitySource = "telegram" | "gmail" | "manual" | "onchain";

/** Relationship types between entities */
export type RelationType =
  | "member_of"
  | "sent_by"
  | "sent_to"
  | "owns"
  | "traded"
  | "replied_to";

/** Core entity record */
export interface Entity {
  id: string;
  /** Mapped from Rust `type` field */
  type: EntityType;
  source: EntitySource;
  sourceId: string | null;
  title: string | null;
  summary: string | null;
  /** JSON blob for type-specific fields */
  metadata: string | null;
  createdAt: number;
  updatedAt: number;
}

/** Entity as returned from Rust IPC (snake_case) */
export interface EntityRust {
  id: string;
  entity_type: string;
  source: string;
  source_id: string | null;
  title: string | null;
  summary: string | null;
  metadata: string | null;
  created_at: number;
  updated_at: number;
}

/** Relationship between two entities */
export interface EntityRelation {
  id: string;
  fromEntityId: string;
  toEntityId: string;
  relationType: RelationType;
  metadata: string | null;
  createdAt: number;
}

/** Relation as returned from Rust IPC (snake_case) */
export interface EntityRelationRust {
  id: string;
  from_entity_id: string;
  to_entity_id: string;
  relation_type: string;
  metadata: string | null;
  created_at: number;
}

/** Tag attached to an entity */
export interface EntityTag {
  entityId: string;
  tag: string;
}

/** Search result from entity FTS */
export interface EntitySearchResult extends Entity {
  score: number;
}

/** Entity search result from Rust IPC */
export interface EntitySearchResultRust extends EntityRust {
  score: number;
}

/** Contact metadata */
export interface ContactMetadata {
  username?: string;
  phone?: string;
  bio?: string;
}

/** Chat metadata */
export interface ChatMetadata {
  chatType?: "group" | "channel" | "private";
  memberCount?: number;
  isChannel?: boolean;
}

/** Message metadata */
export interface MessageMetadata {
  chatId?: string;
  replyToId?: string;
  mediaType?: string;
}

/** Email metadata */
export interface EmailMetadata {
  threadId?: string;
  labels?: string[];
  hasAttachments?: boolean;
}

/** Wallet metadata */
export interface WalletMetadata {
  chain?: string;
  address?: string;
  label?: string;
}

/** Token metadata */
export interface TokenMetadata {
  chain?: string;
  contract?: string;
  symbol?: string;
  decimals?: number;
}

/** Transaction metadata */
export interface TransactionMetadata {
  chain?: string;
  txHash?: string;
  value?: string;
  method?: string;
}

// --- Conversion helpers ---

/** Convert Rust entity to TypeScript entity */
export function fromRustEntity(e: EntityRust): Entity {
  return {
    id: e.id,
    type: e.entity_type as EntityType,
    source: e.source as EntitySource,
    sourceId: e.source_id,
    title: e.title,
    summary: e.summary,
    metadata: e.metadata,
    createdAt: e.created_at,
    updatedAt: e.updated_at,
  };
}

/** Convert TypeScript entity to Rust entity */
export function toRustEntity(e: Entity): EntityRust {
  return {
    id: e.id,
    entity_type: e.type,
    source: e.source,
    source_id: e.sourceId,
    title: e.title,
    summary: e.summary,
    metadata: e.metadata,
    created_at: e.createdAt,
    updated_at: e.updatedAt,
  };
}

/** Convert Rust relation to TypeScript relation */
export function fromRustRelation(r: EntityRelationRust): EntityRelation {
  return {
    id: r.id,
    fromEntityId: r.from_entity_id,
    toEntityId: r.to_entity_id,
    relationType: r.relation_type as RelationType,
    metadata: r.metadata,
    createdAt: r.created_at,
  };
}

/** Convert TypeScript relation to Rust relation */
export function toRustRelation(r: EntityRelation): EntityRelationRust {
  return {
    id: r.id,
    from_entity_id: r.fromEntityId,
    to_entity_id: r.toEntityId,
    relation_type: r.relationType,
    metadata: r.metadata,
    created_at: r.createdAt,
  };
}

/** Convert Rust search result to TypeScript search result */
export function fromRustSearchResult(r: EntitySearchResultRust): EntitySearchResult {
  return {
    ...fromRustEntity(r),
    score: r.score,
  };
}
