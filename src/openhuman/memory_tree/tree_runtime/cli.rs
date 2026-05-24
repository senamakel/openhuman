//! `openhuman tree-summarizer` — CLI for the hierarchical summary tree.
//!
//! Ingest content, run summarization jobs, query the tree, and inspect
//! status from the terminal without starting the full app.
//!
//! Usage:
//!   openhuman tree-summarizer ingest  <namespace> [--content <text> | --file <path>] [-v]
//!   openhuman tree-summarizer run     <namespace> [-v]
//!   openhuman tree-summarizer query   <namespace> [<node_id>] [-v]
//!   openhuman tree-summarizer status  <namespace> [-v]
//!   openhuman tree-summarizer rebuild <namespace> [-v]

use anyhow::Result;

/// Entry point for `openhuman tree-summarizer <subcommand>`.
pub(crate) fn run_tree_summarizer_command(args: &[String]) -> Result<()> {
    if args.is_empty() || is_help(&args[0]) {
        print_help();
        return Ok(());
    }

    match args[0].as_str() {
        "ingest" => run_ingest(&args[1..]),
        "run" => run_summarize(&args[1..]),
        "query" => run_query(&args[1..]),
        "status" => run_status(&args[1..]),
        "rebuild" => run_rebuild(&args[1..]),
        other => Err(anyhow::anyhow!(
            "unknown tree-summarizer subcommand '{other}'. Run `openhuman tree-summarizer --help`."
        )),
    }
}

// ---------------------------------------------------------------------------
// Option parsing
// ---------------------------------------------------------------------------

struct CliOpts {
    verbose: bool,
    content: Option<String>,
    file: Option<String>,
    node_id: Option<String>,
}

fn parse_opts(args: &[String]) -> Result<(CliOpts, Vec<String>)> {
    let mut verbose = false;
    let mut content: Option<String> = None;
    let mut file: Option<String> = None;
    let mut node_id: Option<String> = None;
    let mut rest = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--content" | "-c" => {
                let val = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("missing value for --content"))?;
                content = Some(val.clone());
                i += 2;
            }
            "--file" | "-f" => {
                let val = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("missing value for --file"))?;
                file = Some(val.clone());
                i += 2;
            }
            "--node-id" | "--node" => {
                let val = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("missing value for --node-id"))?;
                node_id = Some(val.clone());
                i += 2;
            }
            "-v" | "--verbose" => {
                verbose = true;
                i += 1;
            }
            "-h" | "--help" => {
                rest.push(args[i].clone());
                i += 1;
            }
            _ => {
                rest.push(args[i].clone());
                i += 1;
            }
        }
    }

    Ok((
        CliOpts {
            verbose,
            content,
            file,
            node_id,
        },
        rest,
    ))
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

/// `openhuman tree-summarizer ingest <namespace> --content <text>` or `--file <path>`
fn run_ingest(args: &[String]) -> Result<()> {
    let (opts, rest) = parse_opts(args)?;

    if rest.iter().any(|a| is_help(a)) || rest.is_empty() {
        println!(
            "Usage: openhuman tree-summarizer ingest <namespace> [--content <text>] [--file <path>] [-v]"
        );
        println!();
        println!("Append content to the summarization buffer for a namespace.");
        println!();
        println!("  <namespace>          Target namespace for the summary tree");
        println!("  --content, -c <text> Raw text content to ingest");
        println!("  --file, -f <path>    Read content from a file (use - for stdin)");
        println!("  -v, --verbose        Enable debug logging");
        println!();
        println!("Either --content or --file is required. If both are given, --file wins.");
        return Ok(());
    }

    let namespace = &rest[0];

    let content = if let Some(ref path) = opts.file {
        if path == "-" {
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| anyhow::anyhow!("failed to read stdin: {e}"))?;
            buf
        } else {
            std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("failed to read '{}': {e}", path))?
        }
    } else if let Some(ref text) = opts.content {
        text.clone()
    } else {
        return Err(anyhow::anyhow!(
            "either --content or --file is required. Run `openhuman tree-summarizer ingest --help`."
        ));
    };

    if content.trim().is_empty() {
        return Err(anyhow::anyhow!("content is empty"));
    }

    init_logging(opts.verbose);

    let rt = build_runtime()?;
    rt.block_on(async {
        let config = load_config().await?;
        let outcome = crate::openhuman::memory_tree::tree_runtime::rpc::tree_summarizer_ingest(
            &config, namespace, &content, None, None,
        )
        .await
        .map_err(anyhow::Error::msg)?;

        println!(
            "{}",
            serde_json::to_string_pretty(&outcome.value)
                .unwrap_or_else(|_| format!("{:?}", outcome.value))
        );
        Ok(())
    })
}

/// `openhuman tree-summarizer run <namespace>`
fn run_summarize(args: &[String]) -> Result<()> {
    let (opts, rest) = parse_opts(args)?;

    if rest.iter().any(|a| is_help(a)) || rest.is_empty() {
        println!("Usage: openhuman tree-summarizer run <namespace> [-v]");
        println!();
        println!("Trigger the summarization job for a namespace.");
        println!("Drains the buffer, creates the hour leaf, and propagates upward.");
        println!();
        println!("  <namespace>      Target namespace");
        println!("  -v, --verbose    Enable debug logging");
        return Ok(());
    }

    let namespace = &rest[0];
    init_logging(opts.verbose);

    let rt = build_runtime()?;
    rt.block_on(async {
        let config = load_config().await?;
        let outcome = crate::openhuman::memory_tree::tree_runtime::rpc::tree_summarizer_run(
            &config, namespace,
        )
        .await
        .map_err(anyhow::Error::msg)?;

        println!(
            "{}",
            serde_json::to_string_pretty(&outcome.value)
                .unwrap_or_else(|_| format!("{:?}", outcome.value))
        );
        Ok(())
    })
}

/// `openhuman tree-summarizer query <namespace> [<node_id>]`
fn run_query(args: &[String]) -> Result<()> {
    let (opts, rest) = parse_opts(args)?;

    if rest.iter().any(|a| is_help(a)) || rest.is_empty() {
        println!(
            "Usage: openhuman tree-summarizer query <namespace> [<node_id>] [--node-id <id>] [-v]"
        );
        println!();
        println!("Read a summary tree node and its direct children.");
        println!();
        println!("  <namespace>          Target namespace");
        println!("  <node_id>            Node ID to query (default: root)");
        println!("  --node-id, --node    Alternative way to specify the node ID");
        println!("  -v, --verbose        Enable debug logging");
        println!();
        println!("Node ID examples:");
        println!("  root              All-time summary");
        println!("  2024              Year summary");
        println!("  2024/03           Month summary");
        println!("  2024/03/15        Day summary");
        println!("  2024/03/15/14     Hour leaf (2pm)");
        return Ok(());
    }

    let namespace = &rest[0];
    let node_id = opts
        .node_id
        .as_deref()
        .or_else(|| rest.get(1).map(|s| s.as_str()));

    init_logging(opts.verbose);

    let rt = build_runtime()?;
    rt.block_on(async {
        let config = load_config().await?;
        let outcome = crate::openhuman::memory_tree::tree_runtime::rpc::tree_summarizer_query(
            &config, namespace, node_id,
        )
        .await
        .map_err(anyhow::Error::msg)?;

        println!(
            "{}",
            serde_json::to_string_pretty(&outcome.value)
                .unwrap_or_else(|_| format!("{:?}", outcome.value))
        );
        Ok(())
    })
}

/// `openhuman tree-summarizer status <namespace>`
fn run_status(args: &[String]) -> Result<()> {
    let (opts, rest) = parse_opts(args)?;

    if rest.iter().any(|a| is_help(a)) || rest.is_empty() {
        println!("Usage: openhuman tree-summarizer status <namespace> [-v]");
        println!();
        println!("Show tree metadata: node count, depth, date range.");
        println!();
        println!("  <namespace>      Target namespace");
        println!("  -v, --verbose    Enable debug logging");
        return Ok(());
    }

    let namespace = &rest[0];
    init_logging(opts.verbose);

    let rt = build_runtime()?;
    rt.block_on(async {
        let config = load_config().await?;
        let outcome = crate::openhuman::memory_tree::tree_runtime::rpc::tree_summarizer_status(
            &config, namespace,
        )
        .await
        .map_err(anyhow::Error::msg)?;

        println!(
            "{}",
            serde_json::to_string_pretty(&outcome.value)
                .unwrap_or_else(|_| format!("{:?}", outcome.value))
        );
        Ok(())
    })
}

/// `openhuman tree-summarizer rebuild <namespace>`
fn run_rebuild(args: &[String]) -> Result<()> {
    let (opts, rest) = parse_opts(args)?;

    if rest.iter().any(|a| is_help(a)) || rest.is_empty() {
        println!("Usage: openhuman tree-summarizer rebuild <namespace> [-v]");
        println!();
        println!("Rebuild the entire summary tree from hour leaves upward.");
        println!("This re-summarizes all intermediate levels (day, month, year, root).");
        println!();
        println!("  <namespace>      Target namespace");
        println!("  -v, --verbose    Enable debug logging");
        return Ok(());
    }

    let namespace = &rest[0];
    init_logging(opts.verbose);

    eprintln!("  Rebuilding tree for namespace '{namespace}'... this may take a while.");

    let rt = build_runtime()?;
    rt.block_on(async {
        let config = load_config().await?;
        let outcome = crate::openhuman::memory_tree::tree_runtime::rpc::tree_summarizer_rebuild(
            &config, namespace,
        )
        .await
        .map_err(anyhow::Error::msg)?;

        println!(
            "{}",
            serde_json::to_string_pretty(&outcome.value)
                .unwrap_or_else(|_| format!("{:?}", outcome.value))
        );
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build tokio runtime: {e}"))
}

async fn load_config() -> Result<crate::openhuman::config::Config> {
    let mut config = crate::openhuman::config::Config::load_or_init()
        .await
        .unwrap_or_default();
    config.apply_env_overrides();
    Ok(config)
}

fn init_logging(verbose: bool) {
    if !verbose && std::env::var_os("RUST_LOG").is_none() {
        unsafe { std::env::set_var("RUST_LOG", "warn") };
    }
    crate::core::logging::init_for_cli_run(verbose, crate::core::logging::CliLogDefault::Global);
}

fn is_help(value: &str) -> bool {
    matches!(value, "-h" | "--help" | "help")
}

fn print_help() {
    println!("openhuman tree-summarizer — hierarchical summary tree\n");
    println!("Usage:");
    println!(
        "  openhuman tree-summarizer ingest  <namespace> [--content <text>] [--file <path>] [-v]"
    );
    println!("  openhuman tree-summarizer run     <namespace> [-v]");
    println!("  openhuman tree-summarizer query   <namespace> [<node_id>] [-v]");
    println!("  openhuman tree-summarizer status  <namespace> [-v]");
    println!("  openhuman tree-summarizer rebuild <namespace> [-v]");
    println!();
    println!("Subcommands:");
    println!("  ingest    Buffer raw content for the next summarization run");
    println!("  run       Drain buffer → create hour leaf → propagate summaries upward");
    println!("  query     Read a node and its children (default: root)");
    println!("  status    Show tree metadata (node count, depth, date range)");
    println!("  rebuild   Rebuild entire tree from hour leaves (re-summarizes all levels)");
    println!();
    println!("Common options:");
    println!("  -v, --verbose    Enable debug logging");
    println!();
    println!("Examples:");
    println!("  openhuman tree-summarizer ingest my-ns --content 'Some raw data to summarize'");
    println!("  openhuman tree-summarizer ingest my-ns --file notes.txt");
    println!("  cat journal.md | openhuman tree-summarizer ingest my-ns --file -");
    println!("  openhuman tree-summarizer run my-ns");
    println!("  openhuman tree-summarizer query my-ns root");
    println!("  openhuman tree-summarizer query my-ns 2024/03/15");
    println!("  openhuman tree-summarizer status my-ns");
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use tempfile::TempDir;

    use crate::openhuman::config::TEST_ENV_LOCK;

    use super::*;

    struct WorkspaceEnvGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<OsString>,
    }

    impl WorkspaceEnvGuard {
        fn set(path: &std::path::Path) -> Self {
            let lock = TEST_ENV_LOCK.lock().unwrap();
            let previous = std::env::var_os("OPENHUMAN_WORKSPACE");
            std::env::set_var("OPENHUMAN_WORKSPACE", path);
            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for WorkspaceEnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var("OPENHUMAN_WORKSPACE", previous);
            } else {
                std::env::remove_var("OPENHUMAN_WORKSPACE");
            }
        }
    }

    #[test]
    fn is_help_matches_supported_aliases() {
        assert!(is_help("-h"));
        assert!(is_help("--help"));
        assert!(is_help("help"));
        assert!(!is_help("run"));
    }

    #[test]
    fn parse_opts_collects_known_flags_and_rest_args() {
        let args = vec![
            "--content".to_string(),
            "hello".to_string(),
            "--file".to_string(),
            "notes.md".to_string(),
            "--node-id".to_string(),
            "2024/03/15".to_string(),
            "--verbose".to_string(),
            "namespace".to_string(),
        ];
        let (opts, rest) = parse_opts(&args).unwrap();
        assert!(opts.verbose);
        assert_eq!(opts.content.as_deref(), Some("hello"));
        assert_eq!(opts.file.as_deref(), Some("notes.md"));
        assert_eq!(opts.node_id.as_deref(), Some("2024/03/15"));
        assert_eq!(rest, vec!["namespace".to_string()]);
    }

    #[test]
    fn parse_opts_errors_when_flag_value_is_missing() {
        let err = match parse_opts(&["--content".to_string()]) {
            Ok(_) => panic!("missing --content value should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("missing value for --content"));
    }

    #[test]
    fn top_level_command_help_and_unknown_subcommand_behave() {
        assert!(run_tree_summarizer_command(&[]).is_ok());
        assert!(run_tree_summarizer_command(&["--help".to_string()]).is_ok());

        let err = run_tree_summarizer_command(&["bogus".to_string()])
            .expect_err("unknown subcommand should fail");
        assert!(
            err.to_string()
                .contains("unknown tree-summarizer subcommand")
        );
    }

    #[test]
    fn subcommand_argument_validation_errors_without_running_runtime() {
        let err = run_ingest(&["ns".to_string()])
            .expect_err("ingest without content or file should fail");
        assert!(
            err.to_string()
                .contains("either --content or --file is required")
        );

        let err = run_ingest(&["ns".to_string(), "--content".to_string(), "   ".to_string()])
            .expect_err("blank content should fail");
        assert!(err.to_string().contains("content is empty"));
    }

    #[test]
    fn help_paths_for_subcommands_return_ok() {
        assert!(run_ingest(&["--help".to_string()]).is_ok());
        assert!(run_summarize(&["--help".to_string()]).is_ok());
        assert!(run_query(&["--help".to_string()]).is_ok());
        assert!(run_status(&["--help".to_string()]).is_ok());
        assert!(run_rebuild(&["--help".to_string()]).is_ok());
    }

    #[test]
    fn ingest_status_and_query_run_against_isolated_workspace() {
        let tmp = TempDir::new().unwrap();
        let _workspace = WorkspaceEnvGuard::set(tmp.path());

        assert!(run_ingest(&["ns".to_string(), "--content".to_string(), "hello world".to_string()]).is_ok());
        assert!(run_status(&["ns".to_string()]).is_ok());
        let err = run_query(&["ns".to_string(), "root".to_string()])
            .expect_err("root query should fail before a summarization run creates nodes");
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn ingest_reads_from_file_path() {
        let tmp = TempDir::new().unwrap();
        let _workspace = WorkspaceEnvGuard::set(tmp.path());
        let input = tmp.path().join("input.txt");
        std::fs::write(&input, "from file").unwrap();

        let args = vec![
            "ns".to_string(),
            "--file".to_string(),
            input.display().to_string(),
        ];
        assert!(run_ingest(&args).is_ok());
    }

    #[test]
    fn run_and_rebuild_surface_local_ai_runtime_requirement() {
        let tmp = TempDir::new().unwrap();
        let _workspace = WorkspaceEnvGuard::set(tmp.path());

        // Seed a namespace so the commands go through the runtime path
        // rather than failing argument validation.
        assert!(run_ingest(&["ns".to_string(), "--content".to_string(), "seed".to_string()]).is_ok());

        let run_err = run_summarize(&["ns".to_string()]).expect_err("run should require local ai");
        assert!(run_err.to_string().contains("requires local_ai to be enabled"));

        let rebuild_err = run_rebuild(&["ns".to_string()]).expect_err("rebuild should require local ai");
        assert!(rebuild_err.to_string().contains("requires local_ai to be enabled"));
    }
}
