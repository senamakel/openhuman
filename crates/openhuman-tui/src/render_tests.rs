use super::*;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

fn rendered(ui: &UiState) -> String {
    let backend = TestBackend::new(180, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let transcript = TranscriptState::new("test-client");
    terminal
        .draw(|frame| draw(frame, &transcript, ui))
        .expect("draw");
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn tab_bar_and_navigation_footer_are_always_rendered() {
    let ui = UiState::new("thread-1".into(), "client-1".into());
    let output = rendered(&ui);
    for title in ["1 Logs", "2 Chat", "3 Config", "4 Settings"] {
        assert!(output.contains(title), "missing tab {title}");
    }
    assert!(output.contains("Ctrl+Tab switch"));
    assert!(output.contains("Shift+Enter newline"));
}

#[test]
fn editing_footer_explains_that_tab_switching_is_paused() {
    let mut ui = UiState::new("thread-1".into(), "client-1".into());
    ui.active_tab = AppTab::Config;
    ui.config_edit = Some("value".to_string());
    let output = rendered(&ui);
    assert!(output.contains("Finish or Esc before switching tabs"));
    assert!(!output.contains("Alt+1-4 tabs"));
}

#[test]
fn tail_to_width_keeps_the_end() {
    assert_eq!(tail_to_width("hello world", 5), "world");
    assert_eq!(tail_to_width("hi", 10), "hi");
    assert_eq!(tail_to_width("anything", 0), "");
}

#[test]
fn wrapped_line_count_divides_by_width() {
    let line = Line::from("a".repeat(25));
    assert_eq!(wrapped_line_count(&line, 10), 3);
    let empty = Line::from("");
    assert_eq!(wrapped_line_count(&empty, 10), 1);
}

#[test]
fn cursor_byte_lookup_handles_wide_unicode() {
    assert_eq!(byte_at_display_column("界a", 2), "界".len());
    assert_eq!(byte_at_display_column("界a", 3), "界a".len());
}

#[test]
fn short_id_truncates_non_ascii_at_a_character_boundary() {
    let id = "12345678901界xyz";
    assert_eq!(short_id(id), "12345678901界");
    assert_eq!(short_id("abcdefghijklmnop"), "abcdefghijkl");
}
