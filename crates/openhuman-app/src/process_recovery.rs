//! Startup recovery for OpenHuman processes left behind by hard exits.

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub argv0: String,
    pub command: String,
}

#[cfg(target_os = "macos")]
mod imp {
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::core_process;
    use crate::process_kill::{kill_pid_force, kill_pid_term};

    pub(crate) use super::ProcessInfo;

    const TERM_GRACE: Duration = Duration::from_millis(500);

    #[derive(Debug, Default, PartialEq, Eq)]
    struct ReapSummary {
        term: usize,
        kill: usize,
        total: usize,
    }

    trait ProcessKiller {
        fn term(&mut self, pid: u32) -> Result<(), String>;
        fn force(&mut self, pid: u32) -> Result<(), String>;
    }

    struct SystemKiller;

    impl ProcessKiller for SystemKiller {
        fn term(&mut self, pid: u32) -> Result<(), String> {
            kill_pid_term(pid)
        }

        fn force(&mut self, pid: u32) -> Result<(), String> {
            kill_pid_force(pid)
        }
    }

    pub(crate) fn reap_stale_openhuman_processes() {
        if core_process::reuse_existing_listener_enabled() {
            log::info!(
                "[startup-recovery] OPENHUMAN_CORE_REUSE_EXISTING=1; skipping stale process reap"
            );
            return;
        }

        let initial = match enumerate_openhuman_processes() {
            Ok(processes) => processes,
            Err(err) => {
                log::warn!("[startup-recovery] failed to enumerate OpenHuman processes: {err}");
                return;
            }
        };
        let stale = filter_self_pid(&initial, std::process::id());
        if stale.is_empty() {
            log::info!("[startup-recovery] no stale OpenHuman processes found");
            return;
        }

        let mut killer = SystemKiller;
        for process in &stale {
            match killer.term(process.pid) {
                Ok(()) => log::warn!(
                    "[startup-recovery] SIGTERM stale OpenHuman pid={} argv0={}",
                    process.pid,
                    process.argv0
                ),
                Err(err) => log::warn!(
                    "[startup-recovery] failed to SIGTERM stale OpenHuman pid={}: {err}",
                    process.pid
                ),
            }
        }

        std::thread::sleep(TERM_GRACE);

        let after_term = match enumerate_openhuman_processes() {
            Ok(processes) => processes,
            Err(err) => {
                log::warn!(
                    "[startup-recovery] failed to re-enumerate after SIGTERM; skipping SIGKILL escalation: {err}"
                );
                return;
            }
        };
        let summary =
            reap_from_snapshots(&stale, &after_term, std::process::id(), &mut killer, false);
        if summary.kill > 0 {
            log::warn!(
                "[startup-recovery] reap complete term={} kill={} total={}",
                stale.len(),
                summary.kill,
                stale.len()
            );
        } else {
            log::info!(
                "[startup-recovery] reap complete term={} kill=0 total={}",
                stale.len(),
                stale.len()
            );
        }
    }

    pub(crate) fn enumerate_openhuman_processes() -> Result<Vec<ProcessInfo>, String> {
        let Some((contents_dir, main_exe)) = current_bundle_contents_dir() else {
            log::debug!("[startup-recovery] current executable is not inside a .app bundle");
            return Ok(Vec::new());
        };
        let output = std::process::Command::new("ps")
            .args(["-ax", "-o", "pid=,ppid=,command="])
            .output()
            .map_err(|err| format!("spawn ps: {err}"))?;
        if !output.status.success() {
            return Err(format!("ps exited with {}", output.status));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_ps_output(&stdout, &contents_dir, Some(&main_exe)))
    }

    fn reap_from_snapshots(
        initial_stale: &[ProcessInfo],
        after_term: &[ProcessInfo],
        self_pid: u32,
        killer: &mut impl ProcessKiller,
        send_term: bool,
    ) -> ReapSummary {
        let initial_stale = filter_self_pid(initial_stale, self_pid);
        let mut summary = ReapSummary {
            total: initial_stale.len(),
            ..ReapSummary::default()
        };

        if send_term {
            for process in &initial_stale {
                if killer.term(process.pid).is_ok() {
                    summary.term += 1;
                }
            }
        } else {
            summary.term = initial_stale.len();
        }

        let expected: HashMap<u32, &str> = initial_stale
            .iter()
            .map(|process| (process.pid, process.command.as_str()))
            .collect();
        let still_running: Vec<&ProcessInfo> = after_term
            .iter()
            .filter(|process| process.pid != self_pid)
            .filter(|process| {
                expected
                    .get(&process.pid)
                    .is_some_and(|command| *command == process.command)
            })
            .collect();

        for process in still_running {
            match killer.force(process.pid) {
                Ok(()) => {
                    summary.kill += 1;
                    log::warn!(
                        "[startup-recovery] SIGKILL stale OpenHuman pid={} argv0={}",
                        process.pid,
                        process.argv0
                    );
                }
                Err(err) => log::warn!(
                    "[startup-recovery] failed to SIGKILL stale OpenHuman pid={}: {err}",
                    process.pid
                ),
            }
        }

        summary
    }

    fn filter_self_pid(processes: &[ProcessInfo], self_pid: u32) -> Vec<ProcessInfo> {
        let mut seen = HashSet::new();
        processes
            .iter()
            .filter(|process| process.pid != self_pid)
            .filter(|process| seen.insert(process.pid))
            .cloned()
            .collect()
    }

    fn parse_ps_output(
        stdout: &str,
        contents_dir: &Path,
        main_exe: Option<&Path>,
    ) -> Vec<ProcessInfo> {
        stdout
            .lines()
            .filter_map(|line| parse_ps_line(line, contents_dir, main_exe))
            .collect()
    }

    fn parse_ps_line(
        line: &str,
        contents_dir: &Path,
        main_exe: Option<&Path>,
    ) -> Option<ProcessInfo> {
        let line = line.trim_start();
        let (pid_raw, rest) = split_once_whitespace(line)?;
        let (ppid_raw, command) = split_once_whitespace(rest.trim_start())?;
        let command = command.trim().to_string();
        let argv0 = extract_bundle_argv0(&command, contents_dir, main_exe)?;
        Some(ProcessInfo {
            pid: pid_raw.parse().ok()?,
            ppid: ppid_raw.parse().ok()?,
            argv0,
            command,
        })
    }

    fn split_once_whitespace(s: &str) -> Option<(&str, &str)> {
        let idx = s.find(char::is_whitespace)?;
        Some((&s[..idx], &s[idx..]))
    }

    fn extract_bundle_argv0(
        command: &str,
        contents_dir: &Path,
        main_exe: Option<&Path>,
    ) -> Option<String> {
        let command = command.trim_start();
        let contents = contents_dir.to_string_lossy();
        if !command.starts_with(contents.as_ref()) {
            return None;
        }

        if let Some(main_exe) = main_exe {
            let main = main_exe.to_string_lossy();
            if command == main || command.starts_with(&format!("{main} ")) {
                return Some(main.into_owned());
            }
        }

        let frameworks_prefix = format!("{}/Frameworks/", contents);
        if command.starts_with(&frameworks_prefix) {
            let marker = ".app/Contents/MacOS/";
            let marker_idx = command.find(marker)?;
            let bundle_name = Path::new(&command[..marker_idx])
                .file_name()?
                .to_string_lossy();
            let argv0 = format!("{}{}{}", &command[..marker_idx], marker, bundle_name);
            if command == argv0 || command.starts_with(&format!("{argv0} ")) {
                return Some(argv0);
            }
        }

        let first = command.split_whitespace().next()?;
        if Path::new(first).starts_with(contents_dir) {
            Some(first.to_string())
        } else {
            None
        }
    }

    fn current_bundle_contents_dir() -> Option<(PathBuf, PathBuf)> {
        let exe = std::env::current_exe().ok()?;
        let mut cursor = exe.parent();
        while let Some(path) = cursor {
            if path.file_name().is_some_and(|name| name == "Contents")
                && path
                    .parent()
                    .and_then(Path::extension)
                    .is_some_and(|ext| ext == "app")
            {
                return Some((path.to_path_buf(), exe));
            }
            cursor = path.parent();
        }
        None
    }

    #[cfg(test)]
    #[path = "process_recovery_macos_tests.rs"]
    mod tests;
}

/// Linux implementation: use /proc/<pid>/cmdline to enumerate openhuman-core processes.
#[cfg(target_os = "linux")]
mod linux_imp {
    use crate::core_process;
    use crate::process_kill::{kill_pid_force, kill_pid_term};
    use std::time::Duration;

    pub(crate) use super::ProcessInfo;

    const TERM_GRACE: Duration = Duration::from_millis(500);

    pub(crate) fn reap_stale_openhuman_processes() {
        if core_process::reuse_existing_listener_enabled() {
            log::info!(
                "[startup-recovery] OPENHUMAN_CORE_REUSE_EXISTING=1; skipping stale process reap"
            );
            return;
        }

        let self_pid = std::process::id();
        log::debug!("[startup-recovery] linux: scanning /proc for stale OpenHuman processes (self_pid={self_pid})");

        let stale = match enumerate_openhuman_processes() {
            Ok(procs) => procs,
            Err(err) => {
                log::warn!("[startup-recovery] linux: failed to enumerate processes: {err}");
                return;
            }
        };

        if stale.is_empty() {
            log::info!("[startup-recovery] linux: no stale OpenHuman processes found");
            return;
        }

        log::info!(
            "[startup-recovery] linux: found {} stale OpenHuman process(es), sending SIGTERM",
            stale.len()
        );
        for proc in &stale {
            match kill_pid_term(proc.pid) {
                Ok(()) => log::warn!(
                    "[startup-recovery] linux: SIGTERM stale OpenHuman pid={} cmd={}",
                    proc.pid,
                    proc.argv0
                ),
                Err(err) => log::warn!(
                    "[startup-recovery] linux: failed to SIGTERM pid={}: {err}",
                    proc.pid
                ),
            }
        }

        std::thread::sleep(TERM_GRACE);

        let after_term = match enumerate_openhuman_processes() {
            Ok(procs) => procs,
            Err(err) => {
                log::warn!("[startup-recovery] linux: failed to re-enumerate after SIGTERM: {err}");
                return;
            }
        };

        let stale_pids: std::collections::HashSet<u32> = stale.iter().map(|p| p.pid).collect();
        let mut kill_count = 0usize;
        for proc in &after_term {
            if stale_pids.contains(&proc.pid) {
                match kill_pid_force(proc.pid) {
                    Ok(()) => {
                        kill_count += 1;
                        log::warn!(
                            "[startup-recovery] linux: SIGKILL stale OpenHuman pid={} cmd={}",
                            proc.pid,
                            proc.argv0
                        );
                    }
                    Err(err) => log::warn!(
                        "[startup-recovery] linux: failed to SIGKILL pid={}: {err}",
                        proc.pid
                    ),
                }
            }
        }

        log::info!(
            "[startup-recovery] linux: reap complete term={} kill={} total={}",
            stale.len(),
            kill_count,
            stale.len()
        );
    }

    pub(crate) fn enumerate_openhuman_processes() -> Result<Vec<ProcessInfo>, String> {
        let self_pid = std::process::id();
        let mut results = Vec::new();

        let proc_dir = std::fs::read_dir("/proc").map_err(|e| format!("read_dir /proc: {e}"))?;

        for entry in proc_dir.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let pid: u32 = match name_str.parse() {
                Ok(p) => p,
                Err(_) => continue,
            };
            if pid == self_pid {
                continue;
            }

            let cmdline_path = format!("/proc/{pid}/cmdline");
            let cmdline_bytes = match std::fs::read(&cmdline_path) {
                Ok(b) => b,
                Err(_) => continue,
            };

            // /proc/<pid>/cmdline uses NUL bytes as argument separators.
            let cmdline = cmdline_bytes
                .split(|&b| b == 0)
                .filter(|seg| !seg.is_empty())
                .map(|seg| String::from_utf8_lossy(seg).into_owned())
                .collect::<Vec<_>>();

            let argv0 = match cmdline.first() {
                Some(a) => a.clone(),
                None => continue,
            };

            if !is_openhuman_executable(&argv0) {
                continue;
            }

            let ppid = read_ppid(pid).unwrap_or(0);
            let command = cmdline.join(" ");

            log::debug!(
                "[startup-recovery] linux: found OpenHuman process pid={pid} argv0={argv0}"
            );
            results.push(ProcessInfo {
                pid,
                ppid,
                argv0,
                command,
            });
        }

        Ok(results)
    }

    fn read_ppid(pid: u32) -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // /proc/<pid>/stat: "pid (comm) state ppid ..."
        // The comm field can contain spaces and parens, find the closing ')' first.
        let after_comm = stat.rfind(')')?;
        let rest = stat[after_comm + 1..].trim_start();
        // rest: "state ppid ..."
        let mut parts = rest.split_whitespace();
        let _state = parts.next()?;
        parts.next()?.parse().ok()
    }

    fn is_openhuman_executable(argv0: &str) -> bool {
        let filename = std::path::Path::new(argv0)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(argv0);
        let lower = filename.to_ascii_lowercase();
        lower == "openhuman-core" || lower == "openhuman"
    }

    #[cfg(test)]
    #[path = "process_recovery_linux_tests.rs"]
    mod tests;
}

/// Windows implementation: enumerate processes via WMIC (command line included)
/// and reap only a wedged GUI CEF-lock-holder — never a legitimate `core`/`mcp`
/// CLI session, a CEF helper subprocess, or an ancestor of the current process
/// (issue #3900, hardening the #3605 pre-CEF reap).
#[cfg(target_os = "windows")]
mod windows_imp {
    use std::collections::{HashMap, HashSet};
    use std::path::Path;
    use std::time::Duration;

    use crate::core_process;
    use crate::process_kill::{kill_pid_force_no_tree, kill_pid_term};

    pub(crate) use super::ProcessInfo;

    const TERM_GRACE: Duration = Duration::from_millis(500);

    pub(crate) fn reap_stale_openhuman_processes() {
        if core_process::reuse_existing_listener_enabled() {
            log::info!(
                "[startup-recovery] OPENHUMAN_CORE_REUSE_EXISTING=1; skipping stale process reap"
            );
            return;
        }

        let self_pid = std::process::id();
        log::debug!(
            "[startup-recovery] windows: scanning for a wedged GUI CEF-lock-holder (self_pid={self_pid})"
        );

        let all = match enumerate_all_processes() {
            Ok(procs) => procs,
            Err(err) => {
                log::warn!("[startup-recovery] windows: failed to enumerate processes: {err}");
                return;
            }
        };

        let stale = select_reapable_gui_instances(&all, self_pid);
        if stale.is_empty() {
            log::info!("[startup-recovery] windows: no stale OpenHuman GUI instance to reap");
            return;
        }

        log::info!(
            "[startup-recovery] windows: found {} stale OpenHuman GUI instance(s), sending terminate",
            stale.len()
        );
        for proc in &stale {
            match kill_pid_term(proc.pid) {
                Ok(()) => log::warn!(
                    "[startup-recovery] windows: TERM stale OpenHuman GUI pid={} cmd={}",
                    proc.pid,
                    proc.command
                ),
                Err(err) => log::warn!(
                    "[startup-recovery] windows: failed to terminate pid={}: {err}",
                    proc.pid
                ),
            }
        }

        std::thread::sleep(TERM_GRACE);

        // Re-validate before force-kill: only SIGKILL pids that are STILL present
        // AND still classify as a reapable GUI instance. A pid that already
        // exited — or whose number was reused by an unrelated process during the
        // grace — must not be killed. This closes the PID-reuse window and honors
        // `kill_pid_force`'s "revalidate ownership" contract.
        let after = match enumerate_all_processes() {
            Ok(procs) => procs,
            Err(err) => {
                log::warn!(
                    "[startup-recovery] windows: failed to re-enumerate after terminate; skipping SIGKILL escalation: {err}"
                );
                return;
            }
        };
        let still_reapable: HashSet<u32> = select_reapable_gui_instances(&after, self_pid)
            .into_iter()
            .map(|p| p.pid)
            .collect();

        let mut kill_count = 0usize;
        for proc in &stale {
            if !still_reapable.contains(&proc.pid) {
                continue;
            }
            // Non-tree kill (`/F` without `/T`): the CEF helper children of the
            // wedged browser process are reaped by the OS job object when it
            // exits, so `/T` is unnecessary — and a tree kill could reach the
            // freshly launched app during an update relaunch (issue #3900 P1).
            match kill_pid_force_no_tree(proc.pid) {
                Ok(()) => {
                    kill_count += 1;
                    log::warn!(
                        "[startup-recovery] windows: force-killed stale OpenHuman GUI pid={} cmd={}",
                        proc.pid,
                        proc.command
                    );
                }
                Err(err) => log::warn!(
                    "[startup-recovery] windows: failed to force-kill pid={}: {err}",
                    proc.pid
                ),
            }
        }

        log::info!(
            "[startup-recovery] windows: reap complete term={} kill={} total={}",
            stale.len(),
            kill_count,
            stale.len()
        );
    }

    /// Diagnostics listing of OpenHuman-owned processes (GUI, CLI `core`/`mcp`,
    /// standalone core, and CEF helpers), excluding the current process. Backs
    /// the `process_diagnostics_list_owned` command — this is a *listing*, not a
    /// kill list; the reap uses [`select_reapable_gui_instances`].
    pub(crate) fn enumerate_openhuman_processes() -> Result<Vec<ProcessInfo>, String> {
        let self_pid = std::process::id();
        Ok(enumerate_all_processes()?
            .into_iter()
            .filter(|p| p.pid != self_pid)
            .filter(|p| is_openhuman_process(&p.argv0))
            .collect())
    }

    /// Enumerate every running process with its parent pid, executable path, and
    /// full command line. The command line (unavailable from the older
    /// `Caption,ProcessId,…` query) is what lets the reap tell a wedged GUI
    /// instance apart from a `core`/`mcp` CLI session or a CEF `--type=` helper.
    /// Includes the current process so its ancestor chain can be walked.
    fn enumerate_all_processes() -> Result<Vec<ProcessInfo>, String> {
        select_enumeration(enumerate_via_wmic, enumerate_via_cim)
    }

    /// Choose between the two enumerators.
    ///
    /// wmic is absent from Windows 11 24H2 and later, where the spawn fails and
    /// recovery loses every process. Try it first for older builds, then fall
    /// back to CIM, which returns the same fields.
    ///
    /// Takes both as arguments so the choice can be tested without spawning
    /// anything: the branch that matters is the one that only happens on a
    /// machine where wmic is missing.
    fn select_enumeration<W, C>(wmic: W, cim: C) -> Result<Vec<ProcessInfo>, String>
    where
        W: FnOnce() -> Result<Vec<ProcessInfo>, String>,
        C: FnOnce() -> Result<Vec<ProcessInfo>, String>,
    {
        match wmic() {
            Ok(processes) if !processes.is_empty() => {
                log::debug!(
                    "[startup-recovery] wmic enumerated {} processes",
                    processes.len()
                );
                Ok(processes)
            }
            Ok(_) => {
                log::debug!("[startup-recovery] wmic returned no processes, trying CIM");
                cim()
            }
            Err(err) => {
                log::debug!("[startup-recovery] wmic unavailable ({err}), trying CIM");
                cim()
            }
        }
    }

    fn enumerate_via_wmic() -> Result<Vec<ProcessInfo>, String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        // `/format:list` (Key=Value blocks) instead of CSV: command lines and
        // paths routinely contain commas, which corrupts CSV field splitting.
        let output = std::process::Command::new("wmic")
            .args([
                "process",
                "get",
                "Caption,CommandLine,ExecutablePath,ParentProcessId,ProcessId",
                "/format:list",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("spawn wmic: {e}"))?;

        if !output.status.success() {
            return Err(format!("wmic exited with {}", output.status));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_wmic_list_output(&stdout))
    }

    /// Emits the same `Key=Value` blocks wmic produced, so the existing parser
    /// handles both sources unchanged.
    ///
    /// OutputEncoding is pinned because PowerShell otherwise writes stdout in the
    /// console's OEM code page. On a non-English install a path outside that page
    /// is replaced before it ever reaches us, so no decode on this side can
    /// recover it.
    const CIM_SCRIPT: &str = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
         Get-CimInstance Win32_Process | ForEach-Object { \
         \"Caption=$($_.Caption)\"; \
         \"CommandLine=$($_.CommandLine)\"; \
         \"ExecutablePath=$($_.ExecutablePath)\"; \
         \"ParentProcessId=$($_.ParentProcessId)\"; \
         \"ProcessId=$($_.ProcessId)\"; \
         \"\" }";

    fn enumerate_via_cim() -> Result<Vec<ProcessInfo>, String> {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", CIM_SCRIPT])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("spawn powershell: {e}"))?;

        if !output.status.success() {
            log::warn!(
                "[startup-recovery] CIM enumeration failed: powershell exited with {}",
                output.status
            );
            return Err(format!("powershell exited with {}", output.status));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let processes = parse_wmic_list_output(&stdout);
        log::debug!(
            "[startup-recovery] CIM enumerated {} processes",
            processes.len()
        );
        Ok(processes)
    }

    /// Parse `wmic ... /format:list` output: records are blocks of `Key=Value`
    /// lines separated by a blank line. Robust to commas inside `CommandLine` /
    /// `ExecutablePath` (the reason we dropped the CSV format).
    fn parse_wmic_list_output(stdout: &str) -> Vec<ProcessInfo> {
        // Normalize CR (wmic list emits `\r\r\n`) then split records on blanks.
        stdout
            .replace('\r', "")
            .split("\n\n")
            .filter_map(parse_wmic_list_record)
            .collect()
    }

    fn parse_wmic_list_record(record: &str) -> Option<ProcessInfo> {
        let mut caption = "";
        let mut command_line = "";
        let mut exe_path = "";
        let mut ppid: u32 = 0;
        let mut pid: Option<u32> = None;
        for line in record.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "Caption" => caption = value,
                "CommandLine" => command_line = value,
                "ExecutablePath" => exe_path = value,
                "ParentProcessId" => ppid = value.parse().unwrap_or(0),
                "ProcessId" => pid = value.parse().ok(),
                _ => {}
            }
        }
        let pid = pid?;
        let argv0 = if !exe_path.is_empty() {
            exe_path.to_string()
        } else {
            caption.to_string()
        };
        let command = if !command_line.is_empty() {
            command_line.to_string()
        } else {
            argv0.clone()
        };
        Some(ProcessInfo {
            pid,
            ppid,
            argv0,
            command,
        })
    }

    /// True for OpenHuman-owned processes of any role (GUI, CLI, standalone
    /// core, CEF helper). Used only for the diagnostics listing.
    fn is_openhuman_process(argv0: &str) -> bool {
        let name = exe_file_name(argv0);
        name == "openhuman.exe" || name == "openhuman-core.exe"
    }

    /// True only for a wedged GUI browser process that is safe to reap: the
    /// desktop app binary (`OpenHuman.exe`, never the standalone
    /// `openhuman-core.exe`), NOT a CEF helper re-exec (`--type=`), and NOT a
    /// `core` / `mcp` / `mcp-server` CLI/MCP session — those never take the CEF
    /// mutex and may be an active user session, e.g. a Claude MCP client
    /// (issue #3900 P2; see `main.rs` for the subcommand routing).
    fn is_reapable_gui_instance(argv0: &str, command_line: &str) -> bool {
        if exe_file_name(argv0) != "openhuman.exe" {
            return false;
        }
        if command_line_has_type_flag(command_line) {
            return false;
        }
        !matches!(
            first_subcommand(command_line).as_deref(),
            Some("core") | Some("mcp") | Some("mcp-server")
        )
    }

    /// Lowercased file name of an executable path or caption.
    fn exe_file_name(argv0: &str) -> String {
        Path::new(argv0)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(argv0)
            .to_ascii_lowercase()
    }

    /// Whether the command line carries a CEF helper role flag (`--type=…`).
    fn command_line_has_type_flag(command_line: &str) -> bool {
        command_line
            .split_whitespace()
            .any(|tok| tok.trim_matches('"').starts_with("--type="))
    }

    /// Extract argv[1] (the subcommand) from a Windows command line, lowercased.
    /// Handles a quoted or unquoted program path in argv[0]. `None` when there
    /// is no second argument.
    fn first_subcommand(command_line: &str) -> Option<String> {
        let s = command_line.trim_start();
        let rest = if let Some(after_quote) = s.strip_prefix('"') {
            // Quoted argv0: skip to the closing quote.
            let end = after_quote.find('"')?;
            &after_quote[end + 1..]
        } else {
            // Unquoted argv0: skip to the first whitespace.
            match s.find(char::is_whitespace) {
                Some(i) => &s[i..],
                None => "",
            }
        };
        let rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let token = rest[..end].trim_matches('"');
        (!token.is_empty()).then(|| token.to_ascii_lowercase())
    }

    /// PIDs of the current process's ancestor chain (parent, grandparent, …).
    /// During an auto-update relaunch the *old* app spawns the new one, so the
    /// old (possibly still-wedged) instance is our ancestor; excluding ancestors
    /// stops the reap from killing the process that launched us — and, with the
    /// non-tree force-kill, closes the #3900 P1 self-termination hole entirely.
    fn collect_ancestor_pids(all: &[ProcessInfo], self_pid: u32) -> HashSet<u32> {
        let ppid_of: HashMap<u32, u32> = all.iter().map(|p| (p.pid, p.ppid)).collect();
        let mut ancestors = HashSet::new();
        let mut cursor = ppid_of.get(&self_pid).copied();
        while let Some(pid) = cursor {
            // pid 0 = no-parent sentinel; stop. `insert` returning false means a
            // cycle (possible under pid reuse) — break to avoid an infinite loop.
            if pid == 0 || !ancestors.insert(pid) {
                break;
            }
            cursor = ppid_of.get(&pid).copied();
        }
        ancestors
    }

    /// The wedged GUI instances that are safe to reap: reapable GUI processes
    /// (see [`is_reapable_gui_instance`]) minus the current process and its
    /// ancestors, deduplicated by pid.
    fn select_reapable_gui_instances(all: &[ProcessInfo], self_pid: u32) -> Vec<ProcessInfo> {
        let ancestors = collect_ancestor_pids(all, self_pid);
        let mut seen = HashSet::new();
        all.iter()
            .filter(|p| p.pid != self_pid)
            .filter(|p| !ancestors.contains(&p.pid))
            .filter(|p| is_reapable_gui_instance(&p.argv0, &p.command))
            .filter(|p| seen.insert(p.pid))
            .cloned()
            .collect()
    }

    #[cfg(test)]
    #[path = "process_recovery_windows_tests.rs"]
    mod tests;
}

#[cfg(target_os = "macos")]
pub(crate) use imp::{enumerate_openhuman_processes, reap_stale_openhuman_processes};

#[cfg(target_os = "linux")]
pub(crate) use linux_imp::{enumerate_openhuman_processes, reap_stale_openhuman_processes};

#[cfg(target_os = "windows")]
pub(crate) use windows_imp::{enumerate_openhuman_processes, reap_stale_openhuman_processes};
