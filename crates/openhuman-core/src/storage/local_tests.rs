use super::*;
use crate::storage::documents::Repo;
use serde_json::json;
use tempfile::TempDir;

const TABLES: &[&str] = &["old_things"];

fn collections() -> Vec<CollectionSpec> {
    vec![CollectionSpec::new("things")]
}

fn legacy_file(dir: &TempDir) -> std::path::PathBuf {
    let path = dir.path().join("things.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE old_things (id TEXT PRIMARY KEY, n INTEGER);
         INSERT INTO old_things VALUES ('a', 1), ('b', 2);",
    )
    .unwrap();
    path
}

fn config(url: Option<&str>) -> Config {
    let mut config = Config::default();
    config.storage.url = url.map(str::to_string);
    config
}

fn plan(path: &std::path::Path) -> Vec<ImportDoc> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare("SELECT id, n FROM old_things ORDER BY id")
        .unwrap();
    stmt.query_map([], |row| {
        Ok(ImportDoc {
            collection: "things",
            id: row.get(0)?,
            doc: json!({ "n": row.get::<_, i64>(1)? }),
        })
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn open_things(config: &Config, path: &std::path::Path) -> Option<Opened> {
    let read = || Ok(plan(path));
    open(
        config,
        path,
        collections,
        &ImportPlan {
            domain: "things",
            tables: TABLES,
            read: &read,
        },
    )
    .unwrap()
}

#[test]
fn the_default_imports_then_retires_the_old_table() {
    let dir = TempDir::new().unwrap();
    let path = legacy_file(&dir);
    let opened = open_things(&config(None), &path).expect("default mode opens the file");
    assert_eq!(opened.scope, Scope::local());
    let repo = Repo::on(&opened.backend, &opened.scope, "things", collections).unwrap();
    let got = repo
        .run(|docs| async move { docs.get("things", "b").await })
        .unwrap()
        .unwrap();
    assert_eq!(got.doc, json!({ "n": 2 }));
    let tables = table_names(&path);
    assert!(tables.contains(&"_legacy_old_things".to_string()));
    assert!(!tables.contains(&"old_things".to_string()));
}

#[test]
fn a_retired_name_already_taken_gets_a_numeric_suffix() {
    let dir = TempDir::new().unwrap();
    let path = legacy_file(&dir);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE TABLE _legacy_old_things (x)")
        .unwrap();
    open_things(&config(None), &path).unwrap();
    let tables = table_names(&path);
    assert!(tables.contains(&"_legacy_old_things".to_string()));
    assert!(tables.contains(&"_legacy_old_things_2".to_string()));
}

#[test]
fn classic_and_the_empty_file_do_not_import() {
    let dir = TempDir::new().unwrap();
    let path = legacy_file(&dir);
    assert!(open_things(&config(Some("classic")), &path).is_none());
    assert!(table_names(&path).contains(&"old_things".to_string()));
    // An explicit URL that nothing installed keeps the legacy tables too.
    assert!(open_things(&config(Some("memory")), &path).is_none());

    let empty = dir.path().join("empty.db");
    assert!(open_things(&config(None), &empty).is_some());
}

#[test]
fn an_import_that_cannot_read_is_an_error_and_is_retried() {
    let dir = TempDir::new().unwrap();
    let path = legacy_file(&dir);
    let fails = || -> Result<Vec<ImportDoc>> { Err(anyhow!("boom")) };
    let failing = ImportPlan {
        domain: "things",
        tables: TABLES,
        read: &fails,
    };
    let error = open(&config(None), &path, collections, &failing)
        .err()
        .expect("a failed import is reported, not hidden behind the legacy tables");
    assert!(format!("{error:#}").contains("boom"), "{error:#}");
    assert!(
        table_names(&path).contains(&"old_things".to_string()),
        "the old table is untouched"
    );
    // The next call imports properly.
    assert!(open_things(&config(None), &path).is_some());
    assert!(table_names(&path).contains(&"_legacy_old_things".to_string()));
}

#[test]
fn retiring_a_table_releases_its_index_names() {
    let dir = TempDir::new().unwrap();
    let path = legacy_file(&dir);
    Connection::open(&path)
        .unwrap()
        .execute_batch("CREATE INDEX idx_old_things_n ON old_things(n)")
        .unwrap();
    open_things(&config(None), &path).unwrap();
    let conn = Connection::open(&path).unwrap();
    // A recreated legacy table can index itself under the same name again.
    conn.execute_batch(
        "CREATE TABLE old_things (id TEXT PRIMARY KEY, n INTEGER);
         CREATE INDEX idx_old_things_n ON old_things(n)",
    )
    .unwrap();
}
