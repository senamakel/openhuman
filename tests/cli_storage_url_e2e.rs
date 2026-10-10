//! One-shot `openhuman-core` subcommands honor the configured storage URL
//! (`OPENHUMAN_STORAGE_URL`) the way the server does, instead of reading the
//! classic on-disk layout. The `storage` domain's URL rule is exercised
//! through the real binary.

use std::process::Command;

fn core(workspace: &std::path::Path, url: Option<&str>, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_openhuman-core"));
    cmd.args(args)
        .env("HOME", workspace)
        .env("USERPROFILE", workspace)
        .env("OPENHUMAN_WORKSPACE", workspace)
        .env_remove("OPENHUMAN_STORAGE_URL");
    if let Some(url) = url {
        cmd.env("OPENHUMAN_STORAGE_URL", url);
    }
    cmd.output().expect("run openhuman-core")
}

#[test]
fn a_one_shot_command_refuses_a_storage_url_it_cannot_open() {
    let tmp = tempfile::tempdir().unwrap();
    let output = core(tmp.path(), Some("nonsense://nowhere"), &["cron", "list"]);
    assert!(!output.status.success(), "a bad URL must fail the command");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("storage"), "{stderr}");
}

#[test]
fn a_one_shot_command_opens_a_valid_storage_url_and_help_does_not_need_one() {
    let tmp = tempfile::tempdir().unwrap();
    let output = core(tmp.path(), Some("memory"), &["cron", "list"]);
    assert!(output.status.success(), "a valid URL must open the backend");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("opening the configured storage backend"),
        "{stderr}"
    );
    // Help never opens the backend, even a bad one.
    let help = core(tmp.path(), Some("nonsense://nowhere"), &["--help"]);
    assert!(help.status.success());
    let nested = core(
        tmp.path(),
        Some("nonsense://nowhere"),
        &["cron", "list", "--help"],
    );
    assert!(nested.status.success(), "nested help never opens storage");
}

/// Runs `args` as a one-shot in its own workspace (so the classic on-disk
/// layout cannot be what two runs share) against `url`.
fn one_shot(url: &str, args: &[&str]) -> (std::process::Output, tempfile::TempDir) {
    let workspace = tempfile::tempdir().unwrap();
    let output = core(workspace.path(), Some(url), args);
    (output, workspace)
}

#[test]
fn a_second_process_reads_what_a_one_shot_wrote_to_the_backend() {
    let data = tempfile::tempdir().unwrap();
    // (url, whether this build carries the driver for it)
    let candidates = [
        (
            format!("sqlite:{}", data.path().join("shared.db").display()),
            cfg!(feature = "storage-sqlite"),
        ),
        (
            format!("file:{}", data.path().join("shared-files").display()),
            cfg!(feature = "storage-file"),
        ),
    ];
    let mut tested = 0;
    for (url, driver_built) in &candidates {
        if !driver_built {
            continue;
        }
        let (added, _first) = one_shot(
            url,
            &[
                "cron",
                "add",
                "--name",
                "storage-probe",
                "--schedule",
                r#"{"kind":"every","every_ms":3600000}"#,
                "--command",
                "echo probe",
            ],
        );
        let stderr = String::from_utf8_lossy(&added.stderr);
        assert!(added.status.success(), "cron add against {url}: {stderr}");

        // A different workspace: only the backend can hold the job.
        let (listed, _second) = one_shot(url, &["cron", "list"]);
        assert!(
            listed.status.success(),
            "{}",
            String::from_utf8_lossy(&listed.stderr)
        );
        let stdout = String::from_utf8_lossy(&listed.stdout);
        assert!(
            stdout.contains("storage-probe"),
            "second process sees the job: {stdout}"
        );

        // And without the URL the same fresh workspace has nothing.
        let workspace = tempfile::tempdir().unwrap();
        let classic = core(workspace.path(), None, &["cron", "list"]);
        assert!(
            classic.status.success(),
            "{}",
            String::from_utf8_lossy(&classic.stderr)
        );
        assert!(
            !String::from_utf8_lossy(&classic.stdout).contains("storage-probe"),
            "the job lives in the backend, not in files"
        );
        tested += 1;
    }
    if tested == 0 {
        eprintln!("skipped: no storage driver compiled in (enable storage-sqlite or storage-file)");
    }
}
