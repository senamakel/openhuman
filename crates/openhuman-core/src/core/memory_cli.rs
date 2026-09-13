//! `openhuman memory` — CLI for memory ingestion, graph inspection, and debugging.
//!
//! Provides direct access to the memory system from the command line, including
//! document ingestion with heuristic entity/relation extraction, graph querying,
//! and document listing.
//!
//! Usage:
//!   openhuman memory ingest  <file|->  [--namespace <ns>] [--key <key>] [--title <title>] [-v]
//!   openhuman memory docs    [--namespace <ns>]
//!   openhuman memory graph   [--namespace <ns>] [--subject <s>] [--predicate <p>]
//!   openhuman memory query   --namespace <ns> --query <text> [--limit <n>]
//!   openhuman memory namespaces

use anyhow::Result;
use std::io::Read;
use std::path::PathBuf;

use crate::memory::api::types::NamespaceDocumentInput;

/// Entry point for `openhuman memory <subcommand>`.
pub fn run_memory_command(args: &[String]) -> Result<()> {
    if args.is_empty() || is_help(&args[0]) {
        print_memory_help();
        return Ok(());
    }

    match args[0].as_str() {
        "ingest" => run_ingest(&args[1..]),
        "docs" | "list" => run_docs(&args[1..]),
        "graph" | "graph-query" => run_graph_query(&args[1..]),
        "query" => run_query(&args[1..]),
        "namespaces" | "ns" => run_namespaces(&args[1..]),
        "clear" => run_clear(&args[1..]),
        other => Err(anyhow::anyhow!(
            "unknown memory subcommand '{other}'. Run `openhuman memory --help`."
        )),
    }
}

/// Each `openhuman memory <sub>` subcommand and the registered RPC controller
/// whose surface it duplicates.
///
/// The CAPABILITY is deliberately NOT written here — it is read from the
/// controller registry via [`crate::core::all::capability_for_parts`], so the
/// single decision recorded at the `push_cap` site in `crates/openhuman-core/src/core/all.rs` governs
/// both the RPC surface and this CLI. A second hand-maintained table would
/// drift the first time a family tag moves.
const SUBCOMMAND_CONTROLLER: &[(&str, &str)] = &[
    // Full synchronous ingestion — the driver owns chunking and embedding.
    ("ingest", "doc_ingest"),
    // Mandatory core/recall surface: ungated, listed so the table is total.
    ("docs", "list_documents"),
    ("list", "list_documents"),
    ("graph", "graph_query"),
    ("graph-query", "graph_query"),
    ("query", "query_namespace"),
    ("namespaces", "list_namespaces"),
    ("ns", "list_namespaces"),
    ("clear", "clear_namespace"),
];

/// The capability `openhuman memory <sub>` needs, if any. Resolved from the
/// controller registry, never from a local table.
fn required_capability(subcommand: &str) -> Option<tinymemory_api::capabilities::Capability> {
    let function = SUBCOMMAND_CONTROLLER
        .iter()
        .find(|(sub, _)| *sub == subcommand)
        .map(|(_, function)| *function)?;
    crate::core::all::capability_for_parts("memory", function).flatten()
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

/// `openhuman memory ingest <file|-> [options]`
///
/// Reads a file (or stdin with `-`) and performs full synchronous ingestion
/// including heuristic entity/relation extraction. Outputs the ingestion result
/// as JSON for debugging.
fn run_ingest(args: &[String]) -> Result<()> {
    let mut file_path: Option<String> = None;
    let mut namespace = "cli".to_string();
    let mut key: Option<String> = None;
    let mut title: Option<String> = None;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--namespace" | "-n" => {
                namespace = next_arg(args, &mut i, "--namespace")?;
            }
            "--key" | "-k" => {
                key = Some(next_arg(args, &mut i, "--key")?);
            }
            "--title" | "-t" => {
                title = Some(next_arg(args, &mut i, "--title")?);
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                println!("Usage: openhuman memory ingest <file|-> [options]");
                println!();
                println!("  <file>               Path to file to ingest (use '-' for stdin)");
                println!("  -n, --namespace <ns>  Target namespace (default: 'cli')");
                println!("  -k, --key <key>       Document key for dedup (default: filename)");
                println!("  -t, --title <title>   Document title (default: filename)");
                println!("  -v, --verbose         Enable debug logging");
                return Ok(());
            }
            other if file_path.is_none() && (!other.starts_with('-') || other == "-") => {
                file_path = Some(other.to_string());
                i += 1;
            }
            other => return Err(anyhow::anyhow!("unknown ingest arg: {other}")),
        }
    }

    let file_path = file_path.ok_or_else(|| {
        anyhow::anyhow!("missing file argument. Use a file path or '-' for stdin.")
    })?;

    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);

    let content = read_input(&file_path)?;
    let doc_key = key.unwrap_or_else(|| file_path.clone());
    let doc_title = title.unwrap_or_else(|| {
        if file_path == "-" {
            "stdin-input".to_string()
        } else {
            PathBuf::from(&file_path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| file_path.clone())
        }
    });

    eprintln!(
        "[memory:cli] ingesting document: namespace={namespace}, key={doc_key}, title={doc_title}, \
         content_len={}",
        content.len()
    );

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let document_id = rt.block_on(async {
        let binding = create_memory_binding("ingest").await?;
        let documents = binding.provider().as_documents().ok_or_else(|| {
            anyhow::anyhow!(
                "the bound memory driver '{}' does not serve documents",
                binding.driver_id()
            )
        })?;

        let document = NamespaceDocumentInput {
            namespace: namespace.clone(),
            key: doc_key,
            title: doc_title,
            content,
            source_type: "doc".to_string(),
            priority: "medium".to_string(),
            tags: Vec::new(),
            metadata: serde_json::json!({}),
            category: "core".to_string(),
            session_id: None,
            document_id: None,
            taint: crate::memory::MemoryTaint::Internal,
        };

        documents
            .put_document(document)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })?;

    eprintln!();
    eprintln!("=== Ingestion Result ===");
    eprintln!("  document_id:  {document_id}");
    eprintln!("  namespace:    {namespace}");
    // Narrower than this command used to print, and the reason is worth stating
    // rather than leaving as an apparent regression. It used to call the
    // engine's `ingest_doc` directly and report the extraction tally it
    // returned — chunk, entity, relation, preference and decision counts, the
    // model and the extraction mode. The module contract's `put_document`
    // answers a document id, and `IngestOutcome` carries only written/skipped
    // counts and ids, so there is nothing on the wire to print those from.
    // The extraction still happens; only the report is smaller. Widening it
    // means a new contract member and a release, not a host change.

    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "document_id": document_id,
            "namespace": namespace,
        }))?
    );

    Ok(())
}

/// `openhuman memory docs [--namespace <ns>]`
fn run_docs(args: &[String]) -> Result<()> {
    let mut namespace: Option<String> = None;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--namespace" | "-n" => {
                namespace = Some(next_arg(args, &mut i, "--namespace")?);
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                println!("Usage: openhuman memory docs [--namespace <ns>] [-v]");
                return Ok(());
            }
            other => return Err(anyhow::anyhow!("unknown docs arg: {other}")),
        }
    }

    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let result = rt.block_on(async {
        let binding = create_memory_binding("docs").await?;
        documents_family(&binding)?
            .list_documents(namespace.as_deref())
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))
    })?;

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// `openhuman memory graph [--namespace <ns>] [--subject <s>] [--predicate <p>]`
fn run_graph_query(args: &[String]) -> Result<()> {
    let mut namespace: Option<String> = None;
    let mut subject: Option<String> = None;
    let mut predicate: Option<String> = None;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--namespace" | "-n" => {
                namespace = Some(next_arg(args, &mut i, "--namespace")?);
            }
            "--subject" | "-s" => {
                subject = Some(next_arg(args, &mut i, "--subject")?);
            }
            "--predicate" | "-p" => {
                predicate = Some(next_arg(args, &mut i, "--predicate")?);
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                println!(
                    "Usage: openhuman memory graph [--namespace <ns>] [--subject <s>] [--predicate <p>] [-v]"
                );
                return Ok(());
            }
            other => return Err(anyhow::anyhow!("unknown graph arg: {other}")),
        }
    }

    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let result = rt.block_on(async {
        let binding = create_memory_binding("graph").await?;
        let graph = binding.provider().as_graph().ok_or_else(|| {
            anyhow::anyhow!(
                "memory driver `{}` does not support the graph family",
                binding.driver_id()
            )
        })?;
        // `usize::MAX` is how this tree spells "no limit" on the contract's
        // `relations` — the same value `memory.graph_query` passes over RPC —
        // because the engine call this replaced took no limit argument at all.
        let records = graph
            .relations(
                namespace.as_deref(),
                subject.as_deref(),
                predicate.as_deref(),
                usize::MAX,
            )
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        let rows = records
            .into_iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, anyhow::Error>(rows)
    })?;

    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// `openhuman memory query --namespace <ns> --query <text> [--limit <n>]`
fn run_query(args: &[String]) -> Result<()> {
    let mut namespace: Option<String> = None;
    let mut query: Option<String> = None;
    let mut limit: u32 = 10;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--namespace" | "-n" => {
                namespace = Some(next_arg(args, &mut i, "--namespace")?);
            }
            "--query" | "-q" => {
                query = Some(next_arg(args, &mut i, "--query")?);
            }
            "--limit" | "-l" => {
                let raw = next_arg(args, &mut i, "--limit")?;
                limit = raw
                    .parse::<u32>()
                    .map_err(|e| anyhow::anyhow!("invalid --limit: {e}"))?;
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                println!(
                    "Usage: openhuman memory query --namespace <ns> --query <text> [--limit <n>] [-v]"
                );
                return Ok(());
            }
            other => return Err(anyhow::anyhow!("unknown query arg: {other}")),
        }
    }

    let namespace =
        namespace.ok_or_else(|| anyhow::anyhow!("--namespace is required for query"))?;
    let query = query.ok_or_else(|| anyhow::anyhow!("--query is required"))?;

    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let result = rt.block_on(async {
        let binding = create_memory_binding("query").await?;
        let documents = binding.provider().as_documents().ok_or_else(|| {
            anyhow::anyhow!(
                "the bound memory driver '{}' does not serve documents",
                binding.driver_id()
            )
        })?;
        documents
            .query_documents(&namespace, &query, limit as usize)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })?;

    // The contract answers a structured context where the engine handed back a
    // rendered string, so the CLI does the rendering. JSON rather than a guess
    // at the engine's old prose layout: a scripted caller can read it, and it
    // cannot silently diverge from what the driver actually returned.
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// `openhuman memory namespaces`
fn run_namespaces(args: &[String]) -> Result<()> {
    let mut verbose = false;
    for arg in args {
        match arg.as_str() {
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" => {
                println!("Usage: openhuman memory namespaces [-v]");
                return Ok(());
            }
            other => return Err(anyhow::anyhow!("unknown namespaces arg: {other}")),
        }
    }

    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let result = rt.block_on(async {
        let binding = create_memory_binding("namespaces").await?;
        // The documents family, not `MemoryCore::namespaces`: the engine call
        // this replaced listed the namespaces that hold *documents* and
        // answered bare names, where the mandatory family enumerates entry
        // namespaces with aggregate counts.
        documents_family(&binding)?
            .list_namespaces()
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))
    })?;

    for ns in &result {
        println!("{ns}");
    }
    Ok(())
}

/// `openhuman memory clear --namespace <ns>`
fn run_clear(args: &[String]) -> Result<()> {
    let mut namespace: Option<String> = None;
    let mut verbose = false;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--namespace" | "-n" => {
                namespace = Some(next_arg(args, &mut i, "--namespace")?);
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                println!("Usage: openhuman memory clear --namespace <ns> [-v]");
                return Ok(());
            }
            other => return Err(anyhow::anyhow!("unknown clear arg: {other}")),
        }
    }

    let namespace =
        namespace.ok_or_else(|| anyhow::anyhow!("--namespace is required for clear"))?;

    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async {
        let binding = create_memory_binding("clear").await?;
        documents_family(&binding)?
            .clear_namespace(&namespace)
            .await
            .map_err(|error| anyhow::anyhow!("{error}"))?;
        eprintln!("[memory:cli] namespace '{namespace}' cleared");
        Ok::<_, anyhow::Error>(())
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_help(s: &str) -> bool {
    matches!(s, "-h" | "--help" | "help")
}

fn next_arg(args: &[String], i: &mut usize, flag: &str) -> Result<String> {
    let value = args
        .get(*i + 1)
        .ok_or_else(|| anyhow::anyhow!("missing value for {flag}"))?
        .clone();
    *i += 2;
    Ok(value)
}

fn read_input(path: &str) -> Result<String> {
    if path == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf)
    } else {
        let path = PathBuf::from(path);
        if !path.exists() {
            return Err(anyhow::anyhow!("file not found: {}", path.display()));
        }
        Ok(std::fs::read_to_string(&path)?)
    }
}

/// Resolve the bound memory driver for a subcommand, refusing first when it
/// does not advertise the family the subcommand needs.
///
/// Returns the whole [`MemoryBinding`](crate::memory::binding::MemoryBinding)
/// rather than the provider alone, so a missing family can be refused by name
/// *and* by driver id — the same shape as the capability diagnostic, and the
/// only way an operator can tell "this driver has no documents tier" from "this
/// namespace is empty".
///
/// Only the capability gate runs here. The legacy-client gate
/// [`create_memory_client`] additionally applies is specifically about the
/// embedded engine client: its refusal says the command "operates on the local
/// embedded store directly, and this configuration bound a different driver",
/// which is exactly what routing through the contract stops doing. Applying it
/// to a contract call would refuse every non-embedded driver for serving a
/// request it is able to serve — and since `binding::admit` now rejects
/// `DriverClass::Embedded` outright, it would refuse all of them.
///
/// The gate is default-OPEN when the binding cannot be resolved, mirroring
/// [`crate::core::all::capability_allowed`]: denying is only ever correct after
/// a driver has actually answered `capabilities()`.
async fn create_memory_binding(
    subcommand: &str,
) -> Result<std::sync::Arc<crate::memory::binding::MemoryBinding>> {
    let config = crate::config::Config::load_or_init()
        .await
        .unwrap_or_default();

    // Resolved through `cli_capability` for the same reason `create_memory_client`
    // does: the memory-guard bypass ratchet carries one allowlisted line for the
    // whole CLI layer. The binding is cached per workspace, so asking twice
    // costs one map lookup.
    let invocation = format!("openhuman memory {subcommand}");
    if let Some((driver_id, _class, advertised)) =
        crate::core::cli_capability::bound_memory_driver_for(
            &config.workspace_dir,
            &config.subsystems.memory,
        )
    {
        if let Some(required) = required_capability(subcommand) {
            crate::core::cli_capability::capability_verdict(
                &driver_id,
                advertised,
                Some(required),
                &invocation,
            )?;
        }
    }

    // The contract event sink, plus the `[modules]` policy a module-backed
    // driver needs published before it can load. Both idempotent.
    //
    // The seven engine seams that used to be installed here went with
    // `tinymemory-core` (#5560) — this process embeds no engine, and every
    // `memory` subcommand routes through `binding::for_workspace` below. The
    // two that did not, `ingest` and `query`, resolved an in-process client
    // and no longer exist. The sink is a `tinymemory-api` seam, not an engine
    // one, and stays for the reason spelled out in `runtime::context`.
    crate::memory::host::install_memory_event_sink();
    #[cfg(feature = "modules")]
    crate::modules::memory::set_modules_policy(std::sync::Arc::new(config.clone()));

    crate::memory::binding::for_workspace(&config.workspace_dir, &config.subsystems.memory)
        .map_err(|error| anyhow::anyhow!(error))
}

/// The documents family on a bound driver, or a refusal naming the driver.
fn documents_family(
    binding: &crate::memory::binding::MemoryBinding,
) -> Result<&dyn crate::memory::api::provider::MemoryDocuments> {
    binding.provider().as_documents().ok_or_else(|| {
        anyhow::anyhow!(
            "memory driver `{}` does not support the documents family",
            binding.driver_id()
        )
    })
}

/// Resolve the in-process engine client for the two subcommands whose shapes
/// the contract cannot yet carry (#5560):
///
/// - `ingest` drives the engine's `MemoryIngestionRequest` pipeline — a
///   caller-supplied `MemoryIngestionConfig` and the extraction counts it
///   reports back. `MemoryIngest::ingest_document` takes neither.
/// - `query` prints the engine's rendered context string; the contract's
///   `query_documents` answers a `NamespaceRetrievalContext`, so the rendering
///   would move into this file and stop matching what the RPC surface returns.
///
/// Every other subcommand routes through [`create_memory_binding`]. Draining
/// these two is upstream work — a `tinymemory` release and a `modules::registry`
/// re-pin — not a routing change here.
///
/// Two gates run first, both *config facts* naming the driver rather than silent
/// absence (`docs/specs/kernel.md` §3.3 makes the CLI its one exception, because
/// a human reads silence as a typo — same reasoning as the retained `mcp` /
/// `tui` arms in `crates/openhuman-core/src/core/cli.rs`):
///
/// 1. **Capability gate** — when the subcommand maps to a gated controller, the
///    bound driver must advertise that family.
/// 2. **Legacy-client gate** — both subcommands below operate on the embedded
///    store directly (via `memory::global::init`), so the bound driver must be
///    the embedded engine. This is what makes `driver = "null"` (or a fallback)
///    actually disable `openhuman memory query`, which has no gated capability
///    to refuse on beyond the one every driver advertises.
///
/// Both gates are default-OPEN when the binding cannot be resolved, mirroring
/// [`crate::core::all::capability_allowed`]: denying is only ever correct after
/// a driver has actually answered `capabilities()`.
fn print_memory_help() {
    println!("Usage: openhuman memory <subcommand> [options]");
    println!();
    println!("Subcommands:");
    println!("  ingest <file|->     Ingest a document with heuristic extraction");
    println!("  docs                List stored documents");
    println!("  graph               Query the knowledge graph");
    println!("  query               Semantic query against a namespace");
    println!("  namespaces          List all namespaces");
    println!("  clear               Clear all data in a namespace");
    println!();
    println!("Some subcommands need capability families the bound memory driver may not");
    println!("advertise. Run `openhuman subsystems` to see what is bound.");
    println!();
    println!("Examples:");
    println!("  openhuman memory ingest notes.md -n my-project -v");
    println!("  echo 'Alice works on ProjectX' | openhuman memory ingest - -n test -v");
    println!("  openhuman memory graph -n my-project");
    println!("  openhuman memory docs -n my-project");
    println!("  openhuman memory query -n my-project -q 'who works on what?'");
}

#[cfg(test)]
#[path = "memory_cli_tests.rs"]
mod tests;
