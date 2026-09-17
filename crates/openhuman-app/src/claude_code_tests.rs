use super::*;

/// Every launcher must reach the `auth` subcommand. `claude login` is not a
/// command — `claude` takes a positional `[prompt]`, so it silently became a
/// prompt instead of starting the OAuth flow.
fn assert_reaches_auth_login(rendered: &str, what: &str) {
    assert!(
        rendered.contains("claude auth login"),
        "{what} must invoke `claude auth login`, got: {rendered}"
    );
    assert!(
        !rendered.contains("claude login"),
        "{what} still constructs the obsolete `claude login`, got: {rendered}"
    );
}

#[test]
fn login_command_line_is_the_auth_subcommand() {
    assert_eq!(login_command_line(), "claude auth login --claudeai");
}

#[test]
fn argv_keeps_auth_and_login_as_separate_words() {
    // Split-argument terminals pass these straight to execvp, so `auth` and
    // `login` have to be distinct argv entries rather than one "auth login".
    assert_eq!(
        CLAUDE_LOGIN_ARGV,
        &["claude", "auth", "login", "--claudeai"]
    );
}

#[test]
fn windows_launcher_reaches_auth_login() {
    assert_reaches_auth_login(&windows_launch_args().join(" "), "the Windows launcher");
}

#[test]
fn windows_launcher_keeps_the_empty_start_title() {
    // `start` reads a bare first argument as the window title, which would
    // swallow `cmd` and open an empty shell.
    let args = windows_launch_args();
    assert_eq!(args[1], "start");
    assert_eq!(args[2], "", "the empty title placeholder must survive");
}

#[test]
fn macos_launcher_reaches_auth_login() {
    assert_reaches_auth_login(&macos_launch_script(), "the macOS AppleScript");
}

#[test]
fn macos_script_quotes_the_command_for_do_script() {
    assert!(macos_launch_script().contains(r#"do script "claude auth login --claudeai""#));
}

#[test]
fn every_linux_candidate_reaches_auth_login() {
    let candidates = linux_launch_candidates();
    assert_eq!(candidates.len(), 5, "all five emulators stay covered");
    for (term, args) in candidates {
        assert_reaches_auth_login(&args.join(" "), term);
    }
}

#[test]
fn xfce4_terminal_gets_one_string_and_the_others_get_argv() {
    let candidates = linux_launch_candidates();
    for (term, args) in candidates {
        match term {
            // -e plus a single command string
            "xfce4-terminal" => assert_eq!(args.len(), 2, "xfce4-terminal takes one string"),
            // separator plus four argv words
            _ => assert_eq!(args.len(), 5, "{term} takes the command as argv"),
        }
    }
}
