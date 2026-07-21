//! Ratatui rendering for the terminal chat UI — pure view over
//! [`TranscriptState`] + [`UiState`]. No state mutation happens here.
//!
//! Layout (top → bottom):
//!   * transcript viewport (fills remaining height, wraps + scrolls)
//!   * single-line input box (bordered)
//!   * status bar (thread id, turn state, key hints)

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use super::state::{EntryKind, TranscriptState};

/// Ocean accent from the design tokens (`#4A83DD`), kept terminal-native.
const OCEAN: Color = Color::Rgb(0x4A, 0x83, 0xDD);

const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// View-only UI state owned by the event loop and read by [`draw`].
pub struct UiState {
    /// Current input line contents.
    pub input: String,
    /// Lines scrolled up from the tail. `0` follows the newest content.
    pub scroll_from_bottom: u16,
    /// Monotonic tick used to animate the streaming spinner.
    pub spinner_tick: usize,
    /// The thread id shown in the status bar.
    pub thread_id: String,
    /// The client stream id (for the status bar, abbreviated).
    pub client_id: String,
}

impl UiState {
    pub fn new(thread_id: String, client_id: String) -> Self {
        Self {
            input: String::new(),
            scroll_from_bottom: 0,
            spinner_tick: 0,
            thread_id,
            client_id,
        }
    }
}

/// Draw one frame.
pub fn draw(frame: &mut Frame, state: &TranscriptState, ui: &UiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // transcript
            Constraint::Length(3), // input box
            Constraint::Length(1), // status bar
        ])
        .split(frame.area());

    draw_transcript(frame, chunks[0], state, ui);
    draw_input(frame, chunks[1], ui);
    draw_status(frame, chunks[2], state, ui);
}

fn draw_transcript(frame: &mut Frame, area: Rect, state: &TranscriptState, ui: &UiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" OpenHuman chat ")
        .border_style(Style::default().fg(OCEAN));
    let inner = block.inner(area);

    let text = transcript_text(state);
    // Inner width available for wrapping (borders eat 2 cols).
    let wrap_width = inner.width.max(1);
    let total_lines: u16 = text
        .lines
        .iter()
        .map(|line| wrapped_line_count(line, wrap_width))
        .sum::<u16>();
    let viewport = inner.height.max(1);
    let max_scroll = total_lines.saturating_sub(viewport);
    let scroll_from_bottom = ui.scroll_from_bottom.min(max_scroll);
    let top = max_scroll.saturating_sub(scroll_from_bottom);

    let paragraph = Paragraph::new(text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((top, 0));
    frame.render_widget(paragraph, area);
}

fn draw_input(frame: &mut Frame, area: Rect, ui: &UiState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Message ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner_width = block.inner(area).width.max(1) as usize;

    // Keep the caret end of a long input visible.
    let display = tail_to_width(&ui.input, inner_width.saturating_sub(1));
    let line = Line::from(vec![
        Span::styled(display, Style::default().fg(Color::White)),
        Span::styled("▏", Style::default().fg(OCEAN)),
    ]);
    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_status(frame: &mut Frame, area: Rect, state: &TranscriptState, ui: &UiState) {
    let turn = if state.is_streaming() {
        let frame_ch = SPINNER_FRAMES[ui.spinner_tick % SPINNER_FRAMES.len()];
        format!("{frame_ch} streaming")
    } else {
        "idle".to_string()
    };
    let thread_short = abbreviate(&ui.thread_id, 24);

    let left = Span::styled(
        format!(" thread {thread_short} · {turn} "),
        Style::default().fg(Color::Black).bg(OCEAN),
    );
    let hints = Span::styled(
        "  Enter send · Esc cancel · Ctrl+N new · PgUp/PgDn scroll · Ctrl+C quit",
        Style::default().fg(Color::DarkGray),
    );
    let paragraph = Paragraph::new(Line::from(vec![left, hints]));
    frame.render_widget(paragraph, area);
}

/// Build the styled transcript body from the reducer state.
fn transcript_text(state: &TranscriptState) -> Text<'static> {
    let mut lines: Vec<Line> = Vec::new();
    for entry in state.entries() {
        let (prefix, style) = match entry.kind {
            EntryKind::User => (
                "You  ",
                Style::default().fg(OCEAN).add_modifier(Modifier::BOLD),
            ),
            EntryKind::Assistant => ("AI   ", Style::default().fg(Color::White)),
            EntryKind::Thinking => (
                "···  ",
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            ),
            EntryKind::Tool => ("tool ", Style::default().fg(Color::Yellow)),
            EntryKind::Error => (
                "err  ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            EntryKind::System => ("     ", Style::default().fg(Color::DarkGray)),
        };

        let mut first = true;
        for raw in entry.text.split('\n') {
            let gutter = if first { prefix } else { "     " };
            lines.push(Line::from(vec![
                Span::styled(gutter.to_string(), style.add_modifier(Modifier::DIM)),
                Span::styled(raw.to_string(), style),
            ]));
            first = false;
        }
        // Blank spacer between entries for readability.
        lines.push(Line::from(""));
    }
    Text::from(lines)
}

/// Approximate the number of visual rows a wrapped line occupies at `width`.
/// ratatui wraps on word boundaries; this display-width estimate is close
/// enough for scroll bookkeeping (off-by-one at most, harmless).
fn wrapped_line_count(line: &Line, width: u16) -> u16 {
    let w = width.max(1) as usize;
    let content_width: usize = line
        .spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if content_width == 0 {
        1
    } else {
        content_width.div_ceil(w).max(1) as u16
    }
}

/// Return the trailing slice of `s` that fits within `width` display columns.
fn tail_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut out: Vec<char> = Vec::new();
    let mut used = 0usize;
    for ch in s.chars().rev() {
        let cw = UnicodeWidthStr::width(ch.to_string().as_str()).max(1);
        if used + cw > width {
            break;
        }
        used += cw;
        out.push(ch);
    }
    out.into_iter().rev().collect()
}

/// Middle-truncate an id to `max` columns (`abc…xyz`).
fn abbreviate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s
        .chars()
        .rev()
        .take(keep)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_to_width_keeps_the_end() {
        assert_eq!(tail_to_width("hello world", 5), "world");
        assert_eq!(tail_to_width("hi", 10), "hi");
        assert_eq!(tail_to_width("anything", 0), "");
    }

    #[test]
    fn abbreviate_middle_truncates_long_ids() {
        let out = abbreviate("thread-0123456789abcdef", 11);
        assert!(out.contains('…'));
        assert!(out.chars().count() <= 11);
        assert_eq!(abbreviate("short", 24), "short");
    }

    #[test]
    fn wrapped_line_count_divides_by_width() {
        let line = Line::from("a".repeat(25));
        assert_eq!(wrapped_line_count(&line, 10), 3);
        let empty = Line::from("");
        assert_eq!(wrapped_line_count(&empty, 10), 1);
    }
}
