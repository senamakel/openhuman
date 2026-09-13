use super::*;
use std::io::{self, Write};

struct FailingWriter {
    remaining: usize,
}

impl Write for FailingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "test broken pipe",
            ));
        }

        let written = buf.len().min(self.remaining);
        self.remaining -= written;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn socket_path_uses_xdg_runtime_dir() {
    std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1234");
    let path = socket_path();
    assert_eq!(
        path,
        PathBuf::from("/run/user/1234/com.openhuman.app-deeplink.sock")
    );
}

#[test]
fn socket_path_fallback_has_uid() {
    std::env::remove_var("XDG_RUNTIME_DIR");
    let path = socket_path();
    let name = path.file_name().unwrap().to_string_lossy();
    assert!(
        name.contains("com_openhuman_app_deeplink"),
        "path {path:?} should contain identifier"
    );
    // Should NOT be inside /run/user since XDG_RUNTIME_DIR is unset.
    assert!(
        !path.starts_with("/run/user"),
        "path should use temp_dir fallback"
    );
}

#[test]
fn extract_deep_link_urls_filters_correctly() {
    // Exercise the REAL production filter through the args-slice seam
    // (mirrors the Windows sibling test) instead of re-implementing the
    // predicate inline — so a regression in the filter actually fails here.
    let urls = collect_deep_link_urls_from_args([
        "OpenHuman",
        "openhuman://auth?token=abc",
        "--some-flag",
        "openhuman://other",
        "https://example.com",
    ]);
    assert_eq!(
        urls,
        vec!["openhuman://auth?token=abc", "openhuman://other"]
    );
}

#[test]
fn write_urls_reports_broken_pipe_after_partial_write() {
    let urls = vec!["openhuman://auth?token=test".to_string()];
    let mut writer = FailingWriter { remaining: 5 };

    let result = write_urls(&mut writer, &urls);

    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn write_urls_writes_all_urls_with_newlines() {
    let urls = vec![
        "openhuman://auth?token=first".to_string(),
        "openhuman://auth?token=second".to_string(),
    ];
    let mut writer = Vec::new();

    write_urls(&mut writer, &urls).unwrap();

    assert_eq!(
        writer,
        b"openhuman://auth?token=first\nopenhuman://auth?token=second\n"
    );
}

#[test]
fn failed_forward_is_not_reported_as_forwarded() {
    let urls = vec!["openhuman://auth?token=test".to_string()];
    let mut writer = FailingWriter { remaining: 5 };

    let result = forward_connected_stream(&mut writer, &urls);

    assert!(matches!(result, ForwardResult::ForwardFailed));
}

#[test]
fn no_primary_returns_no_primary() {
    let tmp = tempfile::TempDir::new().unwrap();
    let sock_path = tmp.path().join("missing-deeplink.sock");
    let urls = vec!["openhuman://auth?token=test".to_string()];

    let result = try_forward_urls(&urls, &sock_path);

    assert!(matches!(result, ForwardResult::NoPrimary));
}

#[test]
fn round_trip_bind_connect_forward() {
    use std::io::BufRead;

    // Use a temp path for this test to avoid collisions.
    let tmp = tempfile::TempDir::new().unwrap();
    let sock_path = tmp.path().join("test-deeplink.sock");

    let listener = UnixListener::bind(&sock_path).unwrap();
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let received_clone = Arc::clone(&received);

    std::thread::spawn(move || {
        if let Ok(stream) = listener.accept().map(|(s, _)| s) {
            stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
            let reader = BufReader::new(stream);
            for line in reader.lines().flatten() {
                if line.starts_with("openhuman://") {
                    received_clone.lock().unwrap().push(line);
                }
            }
        }
    });

    let urls = vec!["openhuman://auth?token=testtoken123".to_string()];
    let result = try_forward_urls(&urls, &sock_path);
    assert!(matches!(result, ForwardResult::Forwarded));

    std::thread::sleep(Duration::from_millis(100));
    let got = received.lock().unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0], "openhuman://auth?token=testtoken123");
}
