//! SQLite entity database for the full platform graph.
//!
//! Stores metadata references (not full content) for all entities across
//! the platform: chats, messages, emails, contacts, wallets, tokens,
//! transactions. Enables cross-entity search and relationship traversal.
//!
//! Database location: `~/.alphahuman/entities.db`

use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::encryption::get_data_dir;

/// Global database connection (initialized once).
static ENTITY_DB: OnceCell<Mutex<rusqlite::Connection>> = OnceCell::new();

/// Core entity record.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub source: String,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    /// JSON blob for type-specific fields.
    pub metadata: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Relationship between two entities.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityRelation {
    pub id: String,
    pub from_entity_id: String,
    pub to_entity_id: String,
    pub relation_type: String,
    pub metadata: Option<String>,
    pub created_at: i64,
}

/// Tag attached to an entity.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityTag {
    pub entity_id: String,
    pub tag: String,
}

/// Search result from FTS5.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntitySearchResult {
    pub id: String,
    pub entity_type: String,
    pub source: String,
    pub source_id: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub metadata: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub score: f64,
}

/// Entity with its relations for traversal queries.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntityWithRelations {
    pub entity: Entity,
    pub relations: Vec<EntityRelation>,
}

/// Get the entity database path (~/.alphahuman/entities.db).
fn get_entity_db_path() -> Result<PathBuf, String> {
    Ok(get_data_dir()?.join("entities.db"))
}

/// Initialize the entity database connection and create tables.
fn init_entity_db() -> Result<rusqlite::Connection, String> {
    let db_path = get_entity_db_path()?;
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("Open entity database: {e}"))?;

    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("WAL mode: {e}"))?;

    conn.execute_batch(
        "
        -- Core entity table (polymorphic)
        CREATE TABLE IF NOT EXISTS entities (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            source TEXT NOT NULL,
            source_id TEXT,
            title TEXT,
            summary TEXT,
            metadata TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE(source, source_id)
        );

        -- Relationships between entities
        CREATE TABLE IF NOT EXISTS entity_relations (
            id TEXT PRIMARY KEY,
            from_entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            to_entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            relation_type TEXT NOT NULL,
            metadata TEXT,
            created_at INTEGER NOT NULL,
            UNIQUE(from_entity_id, to_entity_id, relation_type)
        );

        -- Tags for flexible categorization
        CREATE TABLE IF NOT EXISTS entity_tags (
            entity_id TEXT NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            PRIMARY KEY (entity_id, tag)
        );

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(type);
        CREATE INDEX IF NOT EXISTS idx_entities_source ON entities(source, source_id);
        CREATE INDEX IF NOT EXISTS idx_entities_updated ON entities(updated_at);
        CREATE INDEX IF NOT EXISTS idx_relations_from ON entity_relations(from_entity_id);
        CREATE INDEX IF NOT EXISTS idx_relations_to ON entity_relations(to_entity_id);
        CREATE INDEX IF NOT EXISTS idx_relations_type ON entity_relations(relation_type);
        CREATE INDEX IF NOT EXISTS idx_tags_tag ON entity_tags(tag);

        -- FTS for entity search
        CREATE VIRTUAL TABLE IF NOT EXISTS entities_fts USING fts5(
            title, summary, id UNINDEXED, type UNINDEXED
        );

        -- Enable foreign key constraints
        PRAGMA foreign_keys = ON;
        ",
    )
    .map_err(|e| format!("Create entity tables: {e}"))?;

    Ok(conn)
}

/// Get or initialize the global entity database connection.
fn get_entity_db() -> Result<&'static Mutex<rusqlite::Connection>, String> {
    ENTITY_DB.get_or_try_init(|| {
        let conn = init_entity_db()?;
        Ok::<Mutex<rusqlite::Connection>, String>(Mutex::new(conn))
    })
}

// --- Tauri Commands ---

/// Initialize the entity database. Creates tables if they don't exist.
#[tauri::command]
pub async fn ai_entity_db_init() -> Result<bool, String> {
    get_entity_db()?;
    Ok(true)
}

/// Upsert an entity (insert or update by id, or by source+source_id).
#[tauri::command]
pub async fn ai_entity_upsert(entity: Entity) -> Result<bool, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    // Delete existing FTS entry if entity exists
    conn.execute(
        "DELETE FROM entities_fts WHERE id = ?1",
        rusqlite::params![entity.id],
    )
    .map_err(|e| format!("Delete entity FTS: {e}"))?;

    conn.execute(
        "INSERT OR REPLACE INTO entities (id, type, source, source_id, title, summary, metadata, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            entity.id,
            entity.entity_type,
            entity.source,
            entity.source_id,
            entity.title,
            entity.summary,
            entity.metadata,
            entity.created_at,
            entity.updated_at,
        ],
    )
    .map_err(|e| format!("Upsert entity: {e}"))?;

    // Insert FTS entry
    conn.execute(
        "INSERT INTO entities_fts (title, summary, id, type) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            entity.title.as_deref().unwrap_or(""),
            entity.summary.as_deref().unwrap_or(""),
            entity.id,
            entity.entity_type,
        ],
    )
    .map_err(|e| format!("Insert entity FTS: {e}"))?;

    Ok(true)
}

/// Get an entity by ID.
#[tauri::command]
pub async fn ai_entity_get(id: String) -> Result<Option<Entity>, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    let mut stmt = conn
        .prepare(
            "SELECT id, type, source, source_id, title, summary, metadata, created_at, updated_at
             FROM entities WHERE id = ?1",
        )
        .map_err(|e| format!("Prepare: {e}"))?;

    let result = stmt
        .query_row(rusqlite::params![id], |row| {
            Ok(Entity {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                source: row.get(2)?,
                source_id: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .ok();

    Ok(result)
}

/// Get an entity by source and source_id.
#[tauri::command]
pub async fn ai_entity_get_by_source(
    source: String,
    source_id: String,
) -> Result<Option<Entity>, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    let mut stmt = conn
        .prepare(
            "SELECT id, type, source, source_id, title, summary, metadata, created_at, updated_at
             FROM entities WHERE source = ?1 AND source_id = ?2",
        )
        .map_err(|e| format!("Prepare: {e}"))?;

    let result = stmt
        .query_row(rusqlite::params![source, source_id], |row| {
            Ok(Entity {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                source: row.get(2)?,
                source_id: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .ok();

    Ok(result)
}

/// Full-text search on entities.
#[tauri::command]
pub async fn ai_entity_search(
    query: String,
    types: Option<Vec<String>>,
    limit: i64,
) -> Result<Vec<EntitySearchResult>, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    // Build query with optional type filter
    let base_sql = "
        SELECT e.id, e.type, e.source, e.source_id, e.title, e.summary, e.metadata,
               e.created_at, e.updated_at, rank AS score
        FROM entities_fts
        JOIN entities e ON e.id = entities_fts.id
        WHERE entities_fts MATCH ?1";

    let sql = if let Some(ref type_list) = types {
        if type_list.is_empty() {
            format!("{base_sql} ORDER BY rank LIMIT ?2")
        } else {
            let placeholders: Vec<String> = type_list
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 3))
                .collect();
            format!(
                "{base_sql} AND e.type IN ({}) ORDER BY rank LIMIT ?2",
                placeholders.join(", ")
            )
        }
    } else {
        format!("{base_sql} ORDER BY rank LIMIT ?2")
    };

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Prepare FTS: {e}"))?;

    // Build params dynamically
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(query), Box::new(limit)];
    if let Some(ref type_list) = types {
        for t in type_list {
            params.push(Box::new(t.clone()));
        }
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let results = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(EntitySearchResult {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                source: row.get(2)?,
                source_id: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
                score: row.get::<_, f64>(9)?.abs(),
            })
        })
        .map_err(|e| format!("Entity FTS search: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// List entities by type with pagination.
#[tauri::command]
pub async fn ai_entity_list(
    entity_type: String,
    offset: i64,
    limit: i64,
) -> Result<Vec<Entity>, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    let mut stmt = conn
        .prepare(
            "SELECT id, type, source, source_id, title, summary, metadata, created_at, updated_at
             FROM entities WHERE type = ?1 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3",
        )
        .map_err(|e| format!("Prepare: {e}"))?;

    let results = stmt
        .query_map(rusqlite::params![entity_type, limit, offset], |row| {
            Ok(Entity {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                source: row.get(2)?,
                source_id: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| format!("List entities: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Delete an entity and cascade to relations and tags.
#[tauri::command]
pub async fn ai_entity_delete(id: String) -> Result<bool, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    // Delete FTS entry
    conn.execute(
        "DELETE FROM entities_fts WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| format!("Delete entity FTS: {e}"))?;

    // Delete relations (both directions)
    conn.execute(
        "DELETE FROM entity_relations WHERE from_entity_id = ?1 OR to_entity_id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| format!("Delete relations: {e}"))?;

    // Delete tags
    conn.execute(
        "DELETE FROM entity_tags WHERE entity_id = ?1",
        rusqlite::params![id],
    )
    .map_err(|e| format!("Delete tags: {e}"))?;

    // Delete entity
    conn.execute("DELETE FROM entities WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| format!("Delete entity: {e}"))?;

    Ok(true)
}

/// Add a relationship between entities.
#[tauri::command]
pub async fn ai_entity_add_relation(relation: EntityRelation) -> Result<bool, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    conn.execute(
        "INSERT OR REPLACE INTO entity_relations (id, from_entity_id, to_entity_id, relation_type, metadata, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            relation.id,
            relation.from_entity_id,
            relation.to_entity_id,
            relation.relation_type,
            relation.metadata,
            relation.created_at,
        ],
    )
    .map_err(|e| format!("Add relation: {e}"))?;

    Ok(true)
}

/// Get related entities with optional direction and type filter.
#[tauri::command]
pub async fn ai_entity_get_relations(
    entity_id: String,
    direction: Option<String>,
    relation_type: Option<String>,
) -> Result<Vec<EntityRelation>, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    let dir = direction.as_deref().unwrap_or("both");

    let sql = match (dir, relation_type.as_deref()) {
        ("from", Some(rt)) => {
            format!(
                "SELECT id, from_entity_id, to_entity_id, relation_type, metadata, created_at
                 FROM entity_relations
                 WHERE from_entity_id = ?1 AND relation_type = '{rt}'"
            )
        }
        ("to", Some(rt)) => {
            format!(
                "SELECT id, from_entity_id, to_entity_id, relation_type, metadata, created_at
                 FROM entity_relations
                 WHERE to_entity_id = ?1 AND relation_type = '{rt}'"
            )
        }
        ("from", None) => {
            "SELECT id, from_entity_id, to_entity_id, relation_type, metadata, created_at
             FROM entity_relations
             WHERE from_entity_id = ?1"
                .to_string()
        }
        ("to", None) => {
            "SELECT id, from_entity_id, to_entity_id, relation_type, metadata, created_at
             FROM entity_relations
             WHERE to_entity_id = ?1"
                .to_string()
        }
        (_, Some(rt)) => {
            format!(
                "SELECT id, from_entity_id, to_entity_id, relation_type, metadata, created_at
                 FROM entity_relations
                 WHERE (from_entity_id = ?1 OR to_entity_id = ?1) AND relation_type = '{rt}'"
            )
        }
        _ => "SELECT id, from_entity_id, to_entity_id, relation_type, metadata, created_at
             FROM entity_relations
             WHERE from_entity_id = ?1 OR to_entity_id = ?1"
            .to_string(),
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| format!("Prepare: {e}"))?;

    let results = stmt
        .query_map(rusqlite::params![entity_id], |row| {
            Ok(EntityRelation {
                id: row.get(0)?,
                from_entity_id: row.get(1)?,
                to_entity_id: row.get(2)?,
                relation_type: row.get(3)?,
                metadata: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Get relations: {e}"))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Tag an entity.
#[tauri::command]
pub async fn ai_entity_add_tag(entity_id: String, tag: String) -> Result<bool, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    conn.execute(
        "INSERT OR IGNORE INTO entity_tags (entity_id, tag) VALUES (?1, ?2)",
        rusqlite::params![entity_id, tag],
    )
    .map_err(|e| format!("Add tag: {e}"))?;

    Ok(true)
}

/// Remove a tag from an entity.
#[tauri::command]
pub async fn ai_entity_remove_tag(entity_id: String, tag: String) -> Result<bool, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    conn.execute(
        "DELETE FROM entity_tags WHERE entity_id = ?1 AND tag = ?2",
        rusqlite::params![entity_id, tag],
    )
    .map_err(|e| format!("Remove tag: {e}"))?;

    Ok(true)
}

/// Find entities by tag, with optional type filter.
#[tauri::command]
pub async fn ai_entity_get_by_tag(
    tag: String,
    entity_type: Option<String>,
) -> Result<Vec<Entity>, String> {
    let db = get_entity_db()?;
    let conn = db.lock();

    let sql = if entity_type.is_some() {
        "SELECT e.id, e.type, e.source, e.source_id, e.title, e.summary, e.metadata, e.created_at, e.updated_at
         FROM entities e
         JOIN entity_tags t ON t.entity_id = e.id
         WHERE t.tag = ?1 AND e.type = ?2
         ORDER BY e.updated_at DESC"
    } else {
        "SELECT e.id, e.type, e.source, e.source_id, e.title, e.summary, e.metadata, e.created_at, e.updated_at
         FROM entities e
         JOIN entity_tags t ON t.entity_id = e.id
         WHERE t.tag = ?1
         ORDER BY e.updated_at DESC"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| format!("Prepare: {e}"))?;

    let results = if let Some(ref et) = entity_type {
        stmt.query_map(rusqlite::params![tag, et], |row| {
            Ok(Entity {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                source: row.get(2)?,
                source_id: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| format!("Get by tag: {e}"))?
        .filter_map(|r| r.ok())
        .collect()
    } else {
        stmt.query_map(rusqlite::params![tag], |row| {
            Ok(Entity {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                source: row.get(2)?,
                source_id: row.get(3)?,
                title: row.get(4)?,
                summary: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|e| format!("Get by tag: {e}"))?
        .filter_map(|r| r.ok())
        .collect()
    };

    Ok(results)
}
