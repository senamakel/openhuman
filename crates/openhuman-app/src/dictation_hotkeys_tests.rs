use super::expand_dictation_shortcuts;

#[test]
fn expand_dictation_shortcuts_cmd_or_ctrl_expansion() {
    #[cfg(target_os = "macos")]
    {
        let result = expand_dictation_shortcuts("CmdOrCtrl+Shift+D");
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"Cmd+Shift+D".to_string()));
        assert!(result.contains(&"Ctrl+Shift+D".to_string()));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let result = expand_dictation_shortcuts("CmdOrCtrl+Shift+D");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Ctrl+Shift+D");
    }
}

#[test]
fn expand_dictation_shortcuts_plain_shortcut() {
    let result = expand_dictation_shortcuts("Ctrl+Alt+T");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "Ctrl+Alt+T");
}

#[test]
fn expand_dictation_shortcuts_empty_input() {
    let result = expand_dictation_shortcuts("");
    assert!(result.is_empty());

    let result = expand_dictation_shortcuts("   ");
    assert!(result.is_empty());
}

#[test]
#[cfg(target_os = "macos")]
fn expand_dictation_shortcuts_macos_cmd_only() {
    let result = expand_dictation_shortcuts("CmdOrCtrl+Space");
    assert!(result.contains(&"Cmd+Space".to_string()));
}

#[test]
#[cfg(not(target_os = "macos"))]
fn expand_dictation_shortcuts_non_macos_ctrl_only() {
    let result = expand_dictation_shortcuts("CmdOrCtrl+Space");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "Ctrl+Space");
}
