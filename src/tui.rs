use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use crate::editor::Editor;
use crate::text::matching_paren;

/// Render the full editor UI. Called every time needs_redraw is true.
/// Lisp never calls this — it only mutates Editor state.
pub fn render(frame: &mut Frame, editor: &mut Editor) {
    // Compute text area height (minus 2 for borders) and sync scroll before borrowing.
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(frame.area());

    let viewport_height = chunks[0].height.saturating_sub(2) as usize;
    editor.active_buffer_mut().adjust_scroll(viewport_height);

    let buf = editor.active_buffer();

    // ── Text area ──────────────────────────────────────────────────────────
    let scroll_top = buf.scroll_top;
    let match_pos = matching_paren(&buf.lines, buf.cursor);

    let cursor_style = Style::default().bg(Color::White).fg(Color::Black).add_modifier(Modifier::BOLD);
    let match_style  = Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD);

    let visible = scroll_top..(scroll_top + viewport_height).min(buf.lines.len());
    let text_lines: Vec<Line> = buf.lines[visible]
        .iter()
        .enumerate()
        .map(|(i, content)| {
            let row = scroll_top + i;
            let cursor_col = if row == buf.cursor.0 { Some(buf.cursor.1.min(content.len())) } else { None };
            let match_col  = match_pos.and_then(|(mr, mc)| if mr == row { Some(mc) } else { None });

            // Fast path: no highlights on this row
            if cursor_col.is_none() && match_col.is_none() {
                return Line::from(content.as_str());
            }

            // Build spans by inserting styled chars at highlight positions
            let highlights: Vec<(usize, Style)> = {
                let mut h = vec![];
                if let Some(col) = cursor_col { h.push((col, cursor_style)); }
                if let Some(col) = match_col  { h.push((col, match_style)); }
                h.sort_by_key(|(col, _)| *col);
                h.dedup_by_key(|(col, _)| *col); // cursor == match (rare)
                h
            };

            let mut spans: Vec<Span> = vec![];
            let mut pos = 0usize;
            for (col, style) in highlights {
                if pos < col {
                    spans.push(Span::raw(content[pos..col].to_string()));
                }
                let end = content[col..].char_indices().nth(1).map(|(i, _)| col + i).unwrap_or(content.len());
                let ch = if col >= content.len() { " ".to_string() } else { content[col..end].to_string() };
                spans.push(Span::styled(ch, style));
                pos = end.max(col + 1).min(content.len());
            }
            if pos <= content.len() {
                spans.push(Span::raw(content[pos..].to_string()));
            }
            Line::from(spans)
        })
        .collect();

    let text_widget = Paragraph::new(text_lines)
        .block(Block::default().borders(Borders::ALL).title(buf.name.as_str()));
    frame.render_widget(text_widget, chunks[0]);

    // ── Status bar / minibuffer ────────────────────────────────────────────
    let (row, col) = buf.cursor;
    let status = if let Some(prompt) = editor.prompt_text() {
        prompt
    } else if let Some(msg) = &editor.minibuffer {
        format!(" {msg}")
    } else {
        let dirty = if buf.dirty { "**" } else { "  " };
        format!(" {dirty} {}  L{} C{}   C-q: quit  C-x C-e: eval sexp", buf.name, row + 1, col + 1)
    };
    let status_widget = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));
    frame.render_widget(status_widget, chunks[1]);
}
