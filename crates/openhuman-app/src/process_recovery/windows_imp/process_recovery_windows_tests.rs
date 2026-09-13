use super::*;

fn proc(pid: u32, ppid: u32, argv0: &str, command: &str) -> ProcessInfo {
    ProcessInfo {
        pid,
        ppid,
        argv0: argv0.to_string(),
        command: command.to_string(),
    }
}

#[test]
fn parse_wmic_list_output_reads_command_line_with_commas() {
    // A command line containing commas would corrupt CSV parsing; the
    // list format must preserve it intact.
    let list = "\r\n\
Caption=OpenHuman.exe\r\r\n\
CommandLine=\"C:\\Program Files\\OpenHuman\\OpenHuman.exe\" --flag=a,b,c\r\r\n\
ExecutablePath=C:\\Program Files\\OpenHuman\\OpenHuman.exe\r\r\n\
ParentProcessId=1234\r\r\n\
ProcessId=5678\r\r\n\
\r\r\n\
Caption=chrome.exe\r\r\n\
CommandLine=chrome.exe\r\r\n\
ExecutablePath=C:\\chrome.exe\r\r\n\
ParentProcessId=1\r\r\n\
ProcessId=9000\r\r\n";
    let results = parse_wmic_list_output(list);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].pid, 5678);
    assert_eq!(results[0].ppid, 1234);
    assert!(results[0].command.ends_with("--flag=a,b,c"));
    assert_eq!(results[1].pid, 9000);
}

#[test]
fn first_subcommand_handles_quoted_and_unquoted_argv0() {
    assert_eq!(
        first_subcommand("\"C:\\p\\OpenHuman.exe\" core --port 7788").as_deref(),
        Some("core")
    );
    assert_eq!(
        first_subcommand("C:\\p\\OpenHuman.exe mcp").as_deref(),
        Some("mcp")
    );
    assert_eq!(
        first_subcommand("\"C:\\p\\OpenHuman.exe\"").as_deref(),
        None
    );
    assert_eq!(first_subcommand("OpenHuman.exe").as_deref(), None);
}

#[test]
fn is_reapable_gui_instance_only_matches_the_gui_browser_process() {
    // GUI browser process (no subcommand, no --type=) → reapable.
    assert!(is_reapable_gui_instance(
        "C:\\p\\OpenHuman.exe",
        "\"C:\\p\\OpenHuman.exe\""
    ));
    // #3900 P2: CLI core / MCP sessions must never be reaped.
    assert!(!is_reapable_gui_instance(
        "C:\\p\\OpenHuman.exe",
        "\"C:\\p\\OpenHuman.exe\" core --port 7788"
    ));
    assert!(!is_reapable_gui_instance(
        "C:\\p\\OpenHuman.exe",
        "\"C:\\p\\OpenHuman.exe\" mcp"
    ));
    assert!(!is_reapable_gui_instance(
        "C:\\p\\OpenHuman.exe",
        "\"C:\\p\\OpenHuman.exe\" mcp-server"
    ));
    // CEF helper re-execs carry --type= → not the browser process.
    assert!(!is_reapable_gui_instance(
        "C:\\p\\OpenHuman.exe",
        "\"C:\\p\\OpenHuman.exe\" --type=renderer --enable-features=x"
    ));
    // The standalone core binary is never a GUI CEF-lock-holder.
    assert!(!is_reapable_gui_instance(
        "C:\\p\\openhuman-core.exe",
        "openhuman-core.exe run"
    ));
    // Unrelated processes.
    assert!(!is_reapable_gui_instance("C:\\chrome.exe", "chrome.exe"));
}

#[test]
fn collect_ancestor_pids_walks_the_parent_chain() {
    // self(500) → parent 400 (old OpenHuman) → grandparent 300 (explorer)
    let all = vec![
        proc(300, 1, "explorer.exe", "explorer.exe"),
        proc(400, 300, "OpenHuman.exe", "\"OpenHuman.exe\""),
        proc(500, 400, "OpenHuman.exe", "\"OpenHuman.exe\""),
    ];
    let ancestors = collect_ancestor_pids(&all, 500);
    assert!(ancestors.contains(&400));
    assert!(ancestors.contains(&300));
    assert!(!ancestors.contains(&500));
}

#[test]
fn select_reapable_excludes_self_ancestors_and_non_gui() {
    // 500 = self (new app), 400 = update-relaunch parent (old app,
    // still holds the CEF lock), 900 = an unrelated wedged GUI instance
    // to reap, 700 = a legit `core` CLI session, 800 = a CEF helper.
    let all = vec![
        proc(300, 1, "explorer.exe", "explorer.exe"),
        proc(400, 300, "OpenHuman.exe", "\"OpenHuman.exe\""),
        proc(500, 400, "OpenHuman.exe", "\"OpenHuman.exe\""),
        proc(
            700,
            1,
            "OpenHuman.exe",
            "\"OpenHuman.exe\" core --port 7788",
        ),
        proc(
            800,
            900,
            "OpenHuman.exe",
            "\"OpenHuman.exe\" --type=gpu-process",
        ),
        proc(900, 1, "OpenHuman.exe", "\"OpenHuman.exe\""),
    ];
    let reapable: Vec<u32> = select_reapable_gui_instances(&all, 500)
        .into_iter()
        .map(|p| p.pid)
        .collect();
    assert_eq!(
        reapable,
        vec![900],
        "only the unrelated wedged GUI instance is reaped; self (500), \
         ancestor (400), CLI core (700) and CEF helper (800) are spared"
    );
}

#[test]
fn is_openhuman_process_matches_gui_and_core_only() {
    assert!(is_openhuman_process("C:\\p\\OpenHuman.exe"));
    assert!(is_openhuman_process("C:\\p\\openhuman-core.exe"));
    assert!(is_openhuman_process("OpenHuman.exe"));
    assert!(!is_openhuman_process("C:\\Chrome\\chrome.exe"));
    assert!(!is_openhuman_process("python.exe"));
}

// The fallback only fires on a machine where wmic is missing, so the
// branch that matters is the one CI never reaches. select_enumeration
// takes both enumerators as arguments so it can be driven directly.

#[test]
fn wmic_results_are_used_and_cim_is_not_consulted() {
    let mut cim_called = false;
    let out = select_enumeration(
        || Ok(vec![proc(1, 0, "a", "a")]),
        || {
            cim_called = true;
            Ok(vec![])
        },
    )
    .expect("wmic path");
    assert_eq!(out.len(), 1);
    assert!(!cim_called, "CIM ran even though wmic returned processes");
}

#[test]
fn cim_runs_when_wmic_is_unavailable() {
    let out = select_enumeration(
        || Err("spawn wmic: not found".to_string()),
        || Ok(vec![proc(7, 0, "b", "b")]),
    )
    .expect("cim path");
    assert_eq!(out[0].pid, 7);
}

#[test]
fn cim_runs_when_wmic_returns_nothing() {
    // 24H2 can leave a wmic shim that exits cleanly with no output,
    // which is not an error and still means no processes.
    let out =
        select_enumeration(|| Ok(vec![]), || Ok(vec![proc(9, 0, "c", "c")])).expect("cim path");
    assert_eq!(out[0].pid, 9);
}

#[test]
fn cim_failure_propagates_rather_than_returning_empty() {
    // An empty list would read as "no OpenHuman processes running" and
    // recovery would silently skip every stale one.
    let err = select_enumeration(
        || Err("wmic gone".to_string()),
        || Err("powershell exited with 1".to_string()),
    )
    .expect_err("both failed");
    assert!(err.contains("powershell"), "lost the CIM error: {err}");
}

#[test]
fn cim_script_pins_utf8_and_requests_every_parsed_field() {
    assert!(
        CIM_SCRIPT.contains("[Console]::OutputEncoding"),
        "without the pin PowerShell writes the OEM code page and non-ASCII paths are lost"
    );
    for field in [
        "Caption",
        "CommandLine",
        "ExecutablePath",
        "ParentProcessId",
        "ProcessId",
    ] {
        assert!(
            CIM_SCRIPT.contains(field),
            "CIM script stopped emitting {field}"
        );
    }
}

#[test]
fn parse_reads_cim_blocks_with_non_ascii_and_empty_fields() {
    // What the CIM script emits: LF separated, blank line between
    // records, empty values for a process with no command line.
    // Müller is representable in cp1252, 用户 is not. Both are here so a
    // regression that drops the encoding pin fails on one or the other,
    // whichever code page the runner happens to use.
    let cim = "Caption=System Idle Process\n\
CommandLine=\n\
ExecutablePath=\n\
ParentProcessId=0\n\
ProcessId=0\n\
\n\
Caption=OpenHuman.exe\n\
CommandLine=\"C:\\Users\\Müller\\用户\\OpenHuman.exe\" --flag=a,b\n\
ExecutablePath=C:\\Users\\Müller\\用户\\OpenHuman.exe\n\
ParentProcessId=4\n\
ProcessId=42\n";
    let results = parse_wmic_list_output(cim);
    assert_eq!(results.len(), 2);
    // No executable path, so argv0 falls back to the caption.
    assert_eq!(results[0].argv0, "System Idle Process");
    assert_eq!(results[1].pid, 42);
    assert_eq!(
        results[1].argv0, "C:\\Users\\Müller\\用户\\OpenHuman.exe",
        "non-ASCII path was mangled"
    );
    assert!(results[1].command.ends_with("--flag=a,b"));
}
