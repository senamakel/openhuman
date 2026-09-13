use super::*;

fn contents_dir() -> PathBuf {
    PathBuf::from("/Applications/OpenHuman.app/Contents")
}

fn main_exe() -> PathBuf {
    contents_dir().join("MacOS/OpenHuman")
}

#[test]
fn parse_ps_matches_main_and_helper_bundle_argv0() {
    let stdout = "\
  123   1 /Applications/OpenHuman.app/Contents/MacOS/OpenHuman
  124 123 /Applications/OpenHuman.app/Contents/Frameworks/OpenHuman Helper (Renderer).app/Contents/MacOS/OpenHuman Helper (Renderer) --type=renderer
  999   1 /Applications/Other.app/Contents/MacOS/OpenHuman
";
    let processes = parse_ps_output(stdout, &contents_dir(), Some(&main_exe()));
    assert_eq!(processes.len(), 2);
    assert_eq!(processes[0].pid, 123);
    assert_eq!(processes[0].argv0, main_exe().to_string_lossy());
    assert_eq!(processes[1].pid, 124);
    assert_eq!(
        processes[1].argv0,
        "/Applications/OpenHuman.app/Contents/Frameworks/OpenHuman Helper (Renderer).app/Contents/MacOS/OpenHuman Helper (Renderer)"
    );
}

#[test]
fn filter_self_pid_drops_current_process() {
    let processes = vec![
        ProcessInfo {
            pid: 10,
            ppid: 1,
            argv0: "self".into(),
            command: "self".into(),
        },
        ProcessInfo {
            pid: 11,
            ppid: 1,
            argv0: "other".into(),
            command: "other".into(),
        },
    ];
    let filtered = filter_self_pid(&processes, 10);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].pid, 11);
}

#[test]
fn reap_from_snapshots_escalates_sigkill_for_term_holdouts() {
    #[derive(Default)]
    struct MockKiller {
        term: Vec<u32>,
        force: Vec<u32>,
    }

    impl ProcessKiller for MockKiller {
        fn term(&mut self, pid: u32) -> Result<(), String> {
            self.term.push(pid);
            Ok(())
        }

        fn force(&mut self, pid: u32) -> Result<(), String> {
            self.force.push(pid);
            Ok(())
        }
    }

    let stale = ProcessInfo {
        pid: 42,
        ppid: 1,
        argv0: main_exe().to_string_lossy().into_owned(),
        command: format!("{}", main_exe().display()),
    };
    let still_running = stale.clone();
    let mut killer = MockKiller::default();
    let summary = reap_from_snapshots(
        std::slice::from_ref(&stale),
        &[still_running],
        99,
        &mut killer,
        true,
    );

    assert_eq!(killer.term, vec![42]);
    assert_eq!(killer.force, vec![42]);
    assert_eq!(
        summary,
        ReapSummary {
            term: 1,
            kill: 1,
            total: 1
        }
    );
}
