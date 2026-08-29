//! Golden-workspace schema gate — the fixture-based replacement for the
//! subset-of-table-names check in `memory_golden_parity_e2e.rs`.
//!
//! # What this protects
//!
//! A memory-store schema change that reshapes, renames, or drops a table,
//! index, or trigger strands every existing user workspace. This suite is the
//! thing that fails first.
//!
//! # How it works
//!
//! `tests/fixtures/memory_golden/workspace/**.db` is a **real workspace,
//! captured from a real build** (see that directory's `README.md` for the SHA).
//! `manifest.txt` beside it is **derived from those DB files** by
//! `memory::store_golden::schema_manifest`, never hand-written.
//!
//! ## The ordering rule — read this before "fixing" a failure
//!
//! The old harness could be defeated by a two-line diff: rename a table in
//! `namespace_store/init.rs`, edit the matching `&'static [&str]` constant,
//! green. That cannot work here. The manifest is compared against a manifest
//! **recomputed from the committed `.db` files**, which were written by an
//! older binary. Editing the DDL and the manifest together still fails,
//! because the fixture has not moved. The only way to make this green is to
//! run the regenerator (below), which rewrites the `.db` blobs — a visible,
//! reviewable act that a reviewer can see in the diff.
//!
//! **This is the gate's weakest joint, and a test cannot fully close it.** An
//! author who regenerates the fixture in the same commit as the schema change
//! gets green again. The complementary control is a review rule: *a diff that
//! touches `tests/fixtures/memory_golden/` must be accompanied by an explicit
//! migration story for existing workspaces.* Treat a fixture change as a
//! schema-migration review, not a test-data refresh.
//!
//! ## Regenerating
//!
//! ```bash
//! scripts/regen-memory-golden-fixture.sh
//! ```
//!
//! Run with: `cargo test --test memory_golden_fixture_e2e`

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use tempfile::tempdir;

// The fixture seeder is a module of THIS test target, not of the library.
// It used to be `openhuman::memory::store_golden`, declared `pub mod` and so
// compiled into the shipped binary — seven `tinymemory_core::` references that
// kept the engine crate in the product dependency graph purely to seed a
// fixture (#5560).
#[path = "support/memory_golden.rs"]
mod golden;

// ── Fixture layout ───────────────────────────────────────────────────────────

/// Env var the second-process reopen check reads its workspace from.
const SECOND_PROCESS_WS_ENV: &str = "OPENHUMAN_GOLDEN_FIXTURE_SECOND_PROCESS_WS";

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/memory_golden")
}

fn fixture_workspace() -> PathBuf {
    fixture_root().join("workspace")
}

fn manifest_path() -> PathBuf {
    fixture_root().join("manifest.txt")
}

/// Copy the committed fixture into `dest`.
///
/// Every test works on a copy: SQLite writes to the file it opens (WAL, hot
/// journal, `PRAGMA user_version`), so touching the committed original would
/// dirty the working tree and silently rewrite the very thing under test.
fn copy_fixture_to(dest: &Path) {
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("create fixture copy dir");
        for entry in std::fs::read_dir(from).expect("read fixture dir").flatten() {
            let path = entry.path();
            let target = to.join(entry.file_name());
            if path.is_dir() {
                copy_dir(&path, &target);
            } else {
                std::fs::copy(&path, &target).expect("copy fixture file");
            }
        }
    }
    let src = fixture_workspace();
    assert!(
        src.is_dir(),
        "golden fixture workspace missing at {} — regenerate with \
         scripts/regen-memory-golden-fixture.sh",
        src.display()
    );
    copy_dir(&src, dest);
    eprintln!("[golden-fixture] copied fixture to {}", dest.display());
}

// ── Env isolation (mirrors memory_roundtrip_e2e / memory_golden_parity_e2e) ──

struct EnvVarGuard {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarGuard {
    fn set_to_path(key: &'static str, path: &Path) -> Self {
        let old = std::env::var(key).ok();
        // SAFETY: only used under env_lock(), which serialises env mutation.
        unsafe { std::env::set_var(key, path.as_os_str()) };
        Self { key, old }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.old {
            // SAFETY: see set_to_path; teardown runs under the same env_lock().
            Some(v) => unsafe { std::env::set_var(self.key, v) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static MEMORY_SEAMS_INIT: OnceLock<()> = OnceLock::new();

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env lock poisoned")
}

/// This integration target binds the transport-independent global memory
/// client directly, so it must provide the same host seams that normal core
/// startup installs before opening memory stores.
fn ensure_memory_seams(workspace: &Path) {
    MEMORY_SEAMS_INIT.get_or_init(|| {
        let workspace = workspace.to_path_buf();
        std::thread::Builder::new()
            .name("memory-golden-fixture-seams".to_string())
            .stack_size(8 * 1024 * 1024)
            .spawn(move || {
                let config = Arc::new(openhuman_core::openhuman::config::Config {
                    workspace_dir: workspace.clone(),
                    action_dir: workspace.clone(),
                    config_path: workspace.join("config.toml"),
                    ..openhuman_core::openhuman::config::Config::default()
                });
                openhuman_core::openhuman::memory::host_impls::install_memory_host_seams(
                    config.clone(),
                );
                #[cfg(feature = "modules")]
                openhuman_core::openhuman::modules::memory::set_modules_policy(config);
            })
            .expect("spawn golden fixture memory seam installer")
            .join()
            .expect("golden fixture memory seam installer panicked");
    });
}

// ── Assertions ───────────────────────────────────────────────────────────────

/// Compare two manifests for **set equality**, reporting what is missing and
/// what is extra. A subset check is what made the old harness vacuous.
fn assert_manifest_set_equal(expected: &BTreeSet<String>, actual: &BTreeSet<String>) {
    let missing: Vec<&String> = expected.difference(actual).collect();
    let extra: Vec<&String> = actual.difference(expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "golden workspace schema drifted from the committed manifest.\n\
         \n\
         MISSING ({} object(s) the fixture has that the code no longer produces):\n{}\n\
         \n\
         UNEXPECTED ({} object(s) the code produces that the fixture does not have):\n{}\n\
         \n\
         If this is an intentional schema change you must ALSO have a migration \
         for existing user workspaces, and you must regenerate the fixture with \
         scripts/regen-memory-golden-fixture.sh. Editing manifest.txt alone will \
         not make this pass.",
        missing.len(),
        missing
            .iter()
            .map(|line| format!("  - {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        extra.len(),
        extra
            .iter()
            .map(|line| format!("  + {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

fn committed_manifest() -> BTreeSet<String> {
    let path = manifest_path();
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read committed manifest {}: {e} — regenerate with \
             scripts/regen-memory-golden-fixture.sh",
            path.display()
        )
    });
    let manifest = golden::parse_manifest(&text);
    assert!(
        !manifest.is_empty(),
        "committed manifest {} is empty",
        path.display()
    );
    manifest
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// Gate 1 — the committed fixture's on-disk schema still matches the committed
/// manifest, object for object.
///
/// Touches no process globals, so it can run alongside anything.
#[test]
fn golden_fixture_schema_matches_the_committed_manifest() {
    let tmp = tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    copy_fixture_to(&workspace);

    let actual = golden::schema_manifest(&workspace).expect("dump fixture schema");
    eprintln!(
        "[golden-fixture] fixture holds {} schema objects",
        actual.len()
    );
    assert_manifest_set_equal(&committed_manifest(), &actual);
}

/// Gate 2 — a **fresh** workspace built by the current code has exactly the
/// schema the fixture captured.
///
/// Gate 3 reopens the committed fixture, which cannot see an *in-place*
/// redefinition: `CREATE TABLE / INDEX / TRIGGER IF NOT EXISTS` is a no-op
/// against a DB that already holds the name, so changing an existing object's
/// definition leaves an old workspace untouched. A fresh DB takes the new DDL,
/// so this half catches exactly that edit.
///
/// Touches no process globals.
#[tokio::test]
async fn fresh_workspace_schema_matches_the_committed_manifest() {
    let tmp = tempdir().expect("tempdir");
    let workspace = tmp.path().join("workspace");
    golden::init_fresh_schema(&workspace)
        .await
        .expect("initialise a fresh workspace schema");

    let actual = golden::schema_manifest(&workspace).expect("dump fresh schema");
    eprintln!(
        "[golden-fixture] fresh workspace holds {} schema objects",
        actual.len()
    );
    assert_manifest_set_equal(&committed_manifest(), &actual);
}

/// Gate 3 — the code the current build produces still yields the same schema
/// the fixture captured.
///
/// This is the half that catches a DDL edit: it opens the *fixture copy* with
/// the current `UnifiedMemory::new` + tinycortex init (both of which run their
/// `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE` bootstrap on every open), then
/// re-dumps. A new table, index, or trigger shows up as UNEXPECTED; a renamed
/// one shows up as both MISSING and UNEXPECTED.
///
/// Everything that binds the process-global memory client lives in this one
/// test, for the reason `memory_golden_parity_e2e` documents: the client is
/// process-global and binds to its first workspace, so splitting these across
/// tests makes them pass or fail by scheduling order.
#[tokio::test]
async fn golden_fixture_rows_read_back_and_schema_is_stable_after_reopen() {
    let _lock = env_lock();
    let tmp = tempdir().expect("tempdir");
    let _home = EnvVarGuard::set_to_path("HOME", tmp.path());
    let workspace = tmp.path().join("workspace");
    copy_fixture_to(&workspace);
    let _ws = EnvVarGuard::set_to_path("OPENHUMAN_WORKSPACE", &workspace);
    ensure_memory_seams(&workspace);

    let before = golden::schema_manifest(&workspace).expect("dump schema before open");

    tinymemory_core::global::init(workspace.clone())
        .expect("bind global memory client to the fixture copy");

    // ── Row-level read-back through memory::ops ──
    let readback = golden::read_back(&workspace)
        .await
        .expect("read the golden workspace back");
    eprintln!("[golden-fixture] readback: {readback:#?}");

    assert_eq!(
        readback.primary_doc_keys,
        vec![golden::DOC_KEY_PRIMARY.to_string()],
        "primary-namespace document lost"
    );
    assert_eq!(
        readback.secondary_doc_keys,
        vec![golden::DOC_KEY_SECONDARY.to_string()],
        "secondary-namespace document lost — namespace scoping is broken"
    );
    assert!(readback.kv_global_present, "global-scope KV value lost");
    assert!(
        readback.kv_namespace_present,
        "namespace-scope KV value lost"
    );
    assert_eq!(readback.graph_hits, 1, "graph triple lost");
    assert_eq!(
        readback.episodic_sessions,
        vec![golden::SESSION_ID.to_string()],
        "episodic row lost"
    );
    assert_eq!(
        readback.segment_ids,
        vec![golden::SEGMENT_ID.to_string()],
        "conversation segment lost"
    );
    assert_eq!(
        readback.event_ids,
        vec![golden::EVENT_ID.to_string()],
        "event row lost"
    );
    assert_eq!(
        readback.profile_keys,
        vec![golden::PROFILE_KEY.to_string()],
        "user_profile facet lost"
    );
    assert_eq!(
        readback.summary_ids,
        vec![golden::SUMMARY_ID.to_string()],
        "summary node lost"
    );
    assert!(
        readback.tree_sealed,
        "summary tree is no longer sealed to its root node"
    );
    assert_eq!(
        readback.chunk_ids.len(),
        1,
        "expected exactly one seeded leaf chunk, got {:?}",
        readback.chunk_ids
    );
    assert!(
        readback.embeddings_match,
        "at least one embedding tier did not return the exact seeded vector — \
         a vector encoding or column change would strand every existing embedding"
    );
    // Fixed-query recall: the exact set, not a "contains". Retrieval spans
    // documents, KV values and events, so this pins the whole hit assembly —
    // dropping any tier from the recall path changes this list.
    assert_eq!(
        readback.recall_chunks,
        vec![
            "Decided to pin the memory schema with a captured fixture.".to_string(),
            golden::DOC_CONTENT_PRIMARY.to_string(),
            r#"{"fixture":"golden","v":1}"#.to_string(),
            r#"{"fixture":"golden","v":1}"#.to_string(),
        ],
        "fixed-query recall returned a different result set"
    );

    // ── Opening the workspace must not mutate its schema ──
    let after = golden::schema_manifest(&workspace).expect("dump schema after open");
    assert_manifest_set_equal(&committed_manifest(), &after);
    assert_manifest_set_equal(&before, &after);

    // ── Close and reopen in a SECOND PROCESS ──
    //
    // A fresh process gets a fresh SQLite library state and a cold page cache,
    // so this is what catches WAL / journal-mode surprises that an in-process
    // reopen would hide (the connection pool would just hand back the same
    // warm handle).
    run_second_process_readback(&workspace);
}

/// Spawn this same test binary to run [`second_process_readback`] against
/// `workspace`, and fail loudly with its output if it does not pass.
fn run_second_process_readback(workspace: &Path) {
    let exe = std::env::current_exe().expect("current test binary path");
    eprintln!(
        "[golden-fixture] reopening {} in a second process ({})",
        workspace.display(),
        exe.display()
    );
    let output = std::process::Command::new(exe)
        .args([
            "--exact",
            "second_process_readback",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SECOND_PROCESS_WS_ENV, workspace)
        .env("OPENHUMAN_WORKSPACE", workspace)
        .output()
        .expect("spawn second-process reopen check");
    assert!(
        output.status.success(),
        "second-process reopen of the golden workspace failed ({})\n\
         --- stdout ---\n{}\n--- stderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The second half of the close-and-reopen check. `#[ignore]` because it is
/// only meaningful when [`run_second_process_readback`] launches it with
/// `SECOND_PROCESS_WS_ENV` set; it is not a standalone test.
#[tokio::test]
#[ignore = "spawned as a child process by golden_fixture_rows_read_back_and_schema_is_stable_after_reopen"]
async fn second_process_readback() {
    let Ok(workspace) = std::env::var(SECOND_PROCESS_WS_ENV) else {
        panic!("{SECOND_PROCESS_WS_ENV} not set — this test is spawned, not run directly");
    };
    let workspace = PathBuf::from(workspace);
    ensure_memory_seams(&workspace);
    eprintln!("[golden-fixture][child] reopening {}", workspace.display());

    tinymemory_core::global::init(workspace.clone())
        .expect("bind global memory client in the child process");
    let readback = golden::read_back(&workspace)
        .await
        .expect("read the golden workspace back in a second process");

    assert_eq!(
        readback.primary_doc_keys,
        vec![golden::DOC_KEY_PRIMARY.to_string()],
        "document did not survive close-and-reopen in a second process"
    );
    assert!(
        readback.embeddings_match,
        "embeddings did not survive close-and-reopen in a second process"
    );
    assert!(
        readback.tree_sealed,
        "summary tree seal did not survive close-and-reopen in a second process"
    );
    eprintln!("[golden-fixture][child] reopen check passed");
}

/// Delete everything under `dir` that is not a `*.db` file.
///
/// SQLite recreates `-shm` / `-wal` siblings whenever a DB is opened, even
/// read-only. They are process-local state, so the fixture must not carry them.
fn prune_non_db_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            prune_non_db_files(&path);
        } else if path.extension().and_then(|e| e.to_str()) != Some("db") {
            eprintln!("[golden-fixture][regen] pruning {}", path.display());
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Regenerate the committed fixture and its manifest from the **current**
/// build.
///
/// `#[ignore]` so it never runs in CI — running it is the deliberate act that
/// re-baselines the gate. Invoke via `scripts/regen-memory-golden-fixture.sh`.
#[tokio::test]
#[ignore = "regenerates the committed golden fixture; run via scripts/regen-memory-golden-fixture.sh"]
async fn regenerate_golden_fixture() {
    let _lock = env_lock();
    let tmp = tempdir().expect("tempdir");
    let _home = EnvVarGuard::set_to_path("HOME", tmp.path());
    let staging = tmp.path().join("workspace");
    std::fs::create_dir_all(&staging).expect("create staging workspace");
    let _ws = EnvVarGuard::set_to_path("OPENHUMAN_WORKSPACE", &staging);
    ensure_memory_seams(&staging);

    tinymemory_core::global::init(staging.clone())
        .expect("bind global memory client to the staging workspace");
    golden::seed(&staging).await.expect("seed golden workspace");

    // Fold the WAL back into the main DB and compact, so the committed blob is
    // a single self-contained file with no `-wal` / `-shm` siblings.
    for db in golden::db_files(&staging) {
        let conn = rusqlite::Connection::open(&db).expect("open seeded db for compaction");
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .expect("wal_checkpoint(TRUNCATE)");
        conn.execute_batch("VACUUM;").expect("VACUUM");
        drop(conn);
        eprintln!("[golden-fixture][regen] compacted {}", db.display());
    }

    // Publish: only the `.db` files, so stray `-wal` / `-shm` / markdown
    // sidecars never enter the fixture.
    let target = fixture_workspace();
    if target.exists() {
        std::fs::remove_dir_all(&target).expect("clear previous fixture workspace");
    }
    let mut total = 0u64;
    for db in golden::db_files(&staging) {
        let relative = db.strip_prefix(&staging).expect("db under staging");
        let dest = target.join(relative);
        std::fs::create_dir_all(dest.parent().expect("db has a parent"))
            .expect("create fixture dir");
        std::fs::copy(&db, &dest).expect("publish db into the fixture");
        total += std::fs::metadata(&dest).expect("stat published db").len();
    }

    let manifest = golden::schema_manifest(&target).expect("derive manifest from the fixture");
    let header = format!(
        "# GENERATED — do not hand-edit.\n\
         # Derived from tests/fixtures/memory_golden/workspace/**.db by\n\
         # memory::store_golden::schema_manifest. Regenerate both together with\n\
         # scripts/regen-memory-golden-fixture.sh.\n\
         # {} schema objects across {} db file(s).\n",
        manifest.len(),
        golden::db_files(&target).len()
    );
    std::fs::write(
        manifest_path(),
        format!("{header}{}", golden::render_manifest(&manifest)),
    )
    .expect("write manifest");

    // Dumping the manifest opened each DB, which recreates `-shm` / `-wal`
    // siblings. Those are transient SQLite state, not fixture content, and
    // committing them would make the fixture non-reproducible.
    prune_non_db_files(&target);

    eprintln!(
        "[golden-fixture][regen] wrote {} bytes of fixture and {} manifest objects to {}",
        total,
        manifest.len(),
        fixture_root().display()
    );
}
