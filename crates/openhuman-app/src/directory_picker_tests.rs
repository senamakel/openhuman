use super::absolute_path_string;
use std::path::Path;

#[cfg(not(target_os = "windows"))]
const ABSOLUTE: &str = "/Users/you/notes";
#[cfg(target_os = "windows")]
const ABSOLUTE: &str = r"C:\Users\you\notes";

#[test]
fn passes_an_absolute_path_through_unchanged() {
    assert_eq!(
        absolute_path_string(Path::new(ABSOLUTE)),
        Ok(ABSOLUTE.to_string())
    );
}

#[test]
fn refuses_a_bare_directory_name() {
    // `docs` is exactly what the old `webkitRelativePath.split('/')[0]`
    // fallback stored, and what made the source unsyncable (#5831).
    let err = absolute_path_string(Path::new("docs")).unwrap_err();
    assert!(err.contains("non-absolute"), "unexpected message: {err}");
    assert!(err.contains("docs"), "message should name the value: {err}");
}

#[test]
fn refuses_a_relative_path_with_separators() {
    assert!(absolute_path_string(Path::new("notes/inner")).is_err());
}

/// A Unix directory name is bytes, not text. `Path::display()` would have
/// substituted U+FFFD here and returned the corrupted string as a
/// success, which is the #5831 failure mode reached by another route.
#[cfg(unix)]
#[test]
fn refuses_an_absolute_path_that_is_not_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let raw = OsStr::from_bytes(b"/Users/you/\xff\xfenotes");
    let path = Path::new(raw);
    assert!(path.is_absolute(), "fixture must clear the absolute check");

    let err = absolute_path_string(path).unwrap_err();
    assert!(err.contains("not valid UTF-8"), "unexpected message: {err}");
    // The mangled rendering must not be echoed back — it is both useless
    // and carries the user's login name.
    assert!(
        !err.contains('\u{FFFD}'),
        "lossy path leaked into the error"
    );
}
