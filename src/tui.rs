use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::Clear,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use crate::editor::Editor;
use crate::mode::TokenClass;
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
    let region_range = editor.active_region_range();
    let sexp_range = editor.active_sexp_range();
    let eval_flash_range = editor.active_eval_flash_range();

    let buf = editor.active_buffer();
    let highlight_spans = editor.active_highlight_spans();

    // ── Text area ──────────────────────────────────────────────────────────
    let scroll_top = buf.scroll_top;
    let match_pos = matching_paren(&buf.lines, buf.cursor);

    let match_style  = Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD);
    let region_bg = Color::Rgb(78, 78, 108);
    let active_sexp_bg = Color::Rgb(46, 44, 78);
    let eval_flash_bg = Color::Rgb(106, 64, 28);

    let visible = scroll_top..(scroll_top + viewport_height).min(buf.lines.len());
    let text_lines: Vec<Line> = buf.lines[visible]
        .iter()
        .enumerate()
        .map(|(i, content)| {
            let row = scroll_top + i;
            let match_col  = match_pos.and_then(|(mr, mc)| if mr == row { Some(mc) } else { None });
            let mut styles = vec![Style::default(); content.len().saturating_add(1)];
            for span in highlight_spans.get(row).into_iter().flatten() {
                let style = token_style(span.class);
                for col in span.start.min(content.len())..span.end.min(content.len()) {
                    styles[col] = style;
                }
            }
            if let Some(((start_row, start_col), (end_row, end_col))) = region_range {
                if row >= start_row && row <= end_row {
                    let highlight_start = if row == start_row { start_col.min(content.len()) } else { 0 };
                    let highlight_end = if row == end_row {
                        end_col.min(content.len())
                    } else {
                        content.len()
                    };
                    for col in highlight_start..highlight_end {
                        styles[col] = styles[col].bg(region_bg);
                    }
                }
            }
            if let Some(((start_row, start_col), (end_row, end_col))) = sexp_range {
                if row >= start_row && row <= end_row {
                    let highlight_start = if row == start_row { start_col.min(content.len()) } else { 0 };
                    let highlight_end = if row == end_row {
                        end_col.saturating_add(1).min(content.len())
                    } else {
                        content.len()
                    };
                    for col in highlight_start..highlight_end {
                        styles[col] = styles[col].bg(active_sexp_bg);
                    }
                }
            }
            if let Some(((start_row, start_col), (end_row, end_col))) = eval_flash_range {
                if row >= start_row && row <= end_row {
                    let highlight_start = if row == start_row { start_col.min(content.len()) } else { 0 };
                    let highlight_end = if row == end_row {
                        end_col.saturating_add(1).min(content.len())
                    } else {
                        content.len()
                    };
                    for col in highlight_start..highlight_end {
                        styles[col] = styles[col].bg(eval_flash_bg);
                    }
                }
            }
            if let Some(col) = match_col {
                if col < styles.len() {
                    styles[col] = match_style;
                }
            }

            if styles.iter().all(|style| *style == Style::default()) {
                return Line::from(content.as_str());
            }

            let mut spans: Vec<Span> = vec![];
            let mut pos = 0usize;
            while pos < content.len() {
                let style = styles[pos];
                let mut end = pos + 1;
                while end < content.len() && styles[end] == style {
                    end += 1;
                }
                let text = &content[pos..end];
                if style == Style::default() {
                    spans.push(Span::raw(text.to_string()));
                } else {
                    spans.push(Span::styled(text.to_string(), style));
                }
                pos = end;
            }
            Line::from(spans)
        })
        .collect();

    let text_widget = Paragraph::new(text_lines)
        .block(Block::default().borders(Borders::ALL).title(buf.name.as_str()));
    frame.render_widget(text_widget, chunks[0]);
    let inner_left = chunks[0].x.saturating_add(1);
    let inner_top = chunks[0].y.saturating_add(1);
    let inner_right = chunks[0].right().saturating_sub(2);
    let inner_bottom = chunks[0].bottom().saturating_sub(2);
    let cursor_x = inner_left
        .saturating_add(buf.cursor.1 as u16)
        .min(inner_right);
    let cursor_y = inner_top
        .saturating_add(buf.cursor.0.saturating_sub(scroll_top) as u16)
        .min(inner_bottom);
    frame.set_cursor_position((cursor_x, cursor_y));

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

    if let Some(completion) = editor.completion_state() {
        let visible_count = completion.items.len().min(8);
        let start = completion.scroll.min(completion.items.len().saturating_sub(visible_count));
        let list_width = completion
            .items
            .iter()
            .map(|item| item.label.len())
            .max()
            .unwrap_or(0)
            .max(12)
            .min((chunks[0].width / 2).max(12) as usize);
        let preferred_list_height = visible_count as u16;
        let popup_x = (chunks[0].x + buf.cursor.1 as u16)
            .min(chunks[0].right().saturating_sub(list_width as u16 + 1));
        let popup_y = (chunks[0].y + 1 + buf.cursor.0.saturating_sub(scroll_top) as u16 + 1)
            .min(chunks[0].bottom().saturating_sub(preferred_list_height.max(1)));
        let list_height = preferred_list_height
            .min(chunks[0].bottom().saturating_sub(popup_y))
            .max(1);
        let list_area = Rect::new(popup_x, popup_y, list_width as u16, list_height);
        let items = completion
            .items
            .iter()
            .skip(start)
            .take(list_height as usize)
            .enumerate()
            .map(|(idx, item)| {
                if start + idx == completion.selected {
                    Line::from(Span::styled(
                        pad_right(&item.label, list_width),
                        Style::default().bg(Color::Rgb(84, 78, 150)).fg(Color::White),
                    ))
                } else {
                    Line::from(Span::styled(
                        pad_right(&item.label, list_width),
                        Style::default().bg(Color::Rgb(38, 38, 56)).fg(Color::White),
                    ))
                }
            })
            .collect::<Vec<_>>();
        frame.render_widget(Clear, list_area);
        frame.render_widget(Paragraph::new(items), list_area);

        if let Some(selected) = completion.items.get(completion.selected) {
            let doc_title = selected
                .signature
                .clone()
                .unwrap_or_else(|| selected.label.clone());
            let doc_body = selected
                .docs
                .clone()
                .unwrap_or_else(|| "No documentation.".to_string());
            let doc_lines = std::iter::once(Line::from(Span::styled(
                doc_title,
                Style::default()
                    .fg(Color::Rgb(187, 129, 255))
                    .add_modifier(Modifier::BOLD),
            )))
            .chain(std::iter::once(Line::from("")))
            .chain(doc_body.lines().map(|line| {
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::White),
                ))
            }))
            .collect::<Vec<_>>();
            let available_doc_width = chunks[0].right().saturating_sub(list_area.right() + 1);
            if available_doc_width > 0 {
                let doc_width = available_doc_width.min(48);
                let available_height = chunks[0].bottom().saturating_sub(popup_y).max(1);
                let desired_height = doc_lines.len().max(6).min(12) as u16;
                let doc_height = desired_height.min(available_height);
                let doc_area = Rect::new(
                    list_area.right() + 1,
                    popup_y,
                    doc_width,
                    doc_height,
                );
                frame.render_widget(Clear, doc_area);
                frame.render_widget(
                    Paragraph::new(doc_lines)
                        .style(Style::default().bg(Color::Black))
                        .wrap(Wrap { trim: false }),
                    doc_area,
                );
            }
        }
    }
}

fn pad_right(text: &str, width: usize) -> String {
    let mut out = text.to_string();
    while out.len() < width {
        out.push(' ');
    }
    out
}

fn token_style(class: TokenClass) -> Style {
    match class {
        TokenClass::Comment => Style::default().fg(Color::DarkGray),
        TokenClass::String => Style::default().fg(Color::Green),
        TokenClass::Number => Style::default().fg(Color::Cyan),
        TokenClass::Keyword => Style::default().fg(Color::Magenta),
        TokenClass::Builtin => Style::default().fg(Color::Yellow),
        TokenClass::Special => Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD),
        TokenClass::Delimiter => Style::default().fg(Color::Gray),
    }
}
