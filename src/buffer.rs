use std::path::PathBuf;

use crate::host::BufferId;

pub struct Buffer {
    pub id: BufferId,
    pub name: String,
    pub path: Option<PathBuf>,
    /// Text contents as lines. Empty buffer starts as vec![""]
    pub lines: Vec<String>,
    /// Cursor position (row, col), 0-indexed. Col can be 0..=line.len().
    pub cursor: (usize, usize),
    pub dirty: bool,
    /// First visible line index (for scrolling).
    pub scroll_top: usize,
}

impl Buffer {
    pub fn new(id: BufferId, name: impl Into<String>) -> Self {
        Buffer {
            id,
            name: name.into(),
            path: None,
            lines: vec![String::new()],
            cursor: (0, 0),
            dirty: false,
            scroll_top: 0,
        }
    }

    pub fn from_text(id: BufferId, name: impl Into<String>, text: &str) -> Self {
        let mut buffer = Self::new(id, name);
        buffer.set_text(text);
        buffer.dirty = false;
        buffer
    }

    pub fn from_file(id: BufferId, path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let text = std::fs::read_to_string(&path)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let mut buffer = Self::from_text(id, name, &text);
        buffer.path = Some(path);
        Ok(buffer)
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = if text.is_empty() {
            vec![String::new()]
        } else {
            text.lines().map(|line| line.to_string()).collect()
        };
        if text.ends_with('\n') {
            self.lines.push(String::new());
        }
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor = (0, 0);
        self.scroll_top = 0;
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn set_path(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        self.path = Some(path);
    }

    pub fn save(&mut self) -> std::io::Result<PathBuf> {
        let path = self
            .path
            .clone()
            .ok_or_else(|| std::io::Error::other("buffer is not file-backed"))?;
        std::fs::write(&path, self.text())?;
        self.dirty = false;
        Ok(path)
    }

    pub fn save_as(&mut self, path: impl Into<PathBuf>) -> std::io::Result<PathBuf> {
        self.set_path(path);
        self.save()
    }

    /// Adjust scroll_top so the cursor stays within the visible viewport.
    ///
    pub fn adjust_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.cursor.0 < self.scroll_top {
            self.scroll_top = self.cursor.0;
        }

        if self.cursor.0 >= self.scroll_top + viewport_height {
            self.scroll_top = self.cursor.0 - viewport_height + 1;
        }
    }

    pub fn insert_char(&mut self, c: char) {
        self.dirty = true;
        match c {
            '\n' => {
                let new_line = self.lines[self.cursor.0].split_off(self.cursor.1);
                self.lines.insert(self.cursor.0 + 1, new_line);
                self.cursor = (self.cursor.0 + 1, 0);
            }
            _ => {
                self.lines[self.cursor.0].insert(self.cursor.1, c);
                self.cursor.1 += 1;
            }
        }
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor.1 > 0 {
            self.lines[self.cursor.0].remove(self.cursor.1 - 1);
            self.cursor.1 -= 1;
            self.dirty = true;
        } else if self.cursor.0 > 0 {
            let line = self.lines.remove(self.cursor.0);
            let prev_len = self.lines[self.cursor.0 - 1].len();
            self.lines[self.cursor.0 - 1].push_str(&line);
            self.cursor = (self.cursor.0 - 1, prev_len);
            self.dirty = true;
        }
    }

    pub fn delete_word_before(&mut self) {
        let original = self.cursor;
        let mut target = original;

        while target.1 == 0 && target.0 > 0 {
            target.0 -= 1;
            target.1 = self.lines[target.0].len();
        }

        loop {
            if target.1 == 0 {
                break;
            }
            let line = &self.lines[target.0];
            let prefix = &line[..target.1];
            let ch = prefix.chars().next_back().unwrap();
            if !ch.is_whitespace() {
                break;
            }
            target.1 -= ch.len_utf8();
        }

        loop {
            if target.1 == 0 {
                if target.0 == 0 {
                    break;
                }
                let prev_row = target.0 - 1;
                if self.lines[prev_row].is_empty() {
                    target.0 = prev_row;
                    continue;
                }
                let ch = self.lines[prev_row].chars().next_back().unwrap();
                if ch.is_whitespace() {
                    break;
                }
                target.0 = prev_row;
                target.1 = self.lines[prev_row].len();
                continue;
            }

            let line = &self.lines[target.0];
            let prefix = &line[..target.1];
            let ch = prefix.chars().next_back().unwrap();
            if ch.is_whitespace() {
                break;
            }
            target.1 -= ch.len_utf8();
        }

        if target == original {
            return;
        }

        if target.0 == original.0 {
            self.lines[target.0].drain(target.1..original.1);
        } else {
            let suffix = self.lines[original.0][original.1..].to_string();
            self.lines[target.0].truncate(target.1);
            self.lines[target.0].push_str(&suffix);
            self.lines.drain(target.0 + 1..=original.0);
        }

        self.cursor = target;
        self.dirty = true;
    }

    pub fn delete_to_line_end(&mut self) {
        let row = self.cursor.0;
        let col = self.cursor.1;
        if col < self.lines[row].len() {
            self.lines[row].truncate(col);
            self.dirty = true;
        } else if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].push_str(&next);
            self.dirty = true;
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor.1 > 0 {
            self.cursor.1 -= 1;
        } else if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.cursor.1 = self.lines[self.cursor.0].len();
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor.1 < self.lines[self.cursor.0].len() {
            self.cursor.1 += 1;
        } else if self.cursor.0 < self.lines.len() - 1 {
            self.cursor.0 += 1;
            self.cursor.1 = 0;
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor.0 > 0 {
            self.cursor.0 -= 1;
            self.cursor.1 = self.cursor.1.min(self.lines[self.cursor.0].len());
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor.0 < self.lines.len() - 1 {
            self.cursor.0 += 1;
            self.cursor.1 = self.cursor.1.min(self.lines[self.cursor.0].len());
        }
    }

    pub fn move_to_line_start(&mut self) {
        self.cursor.1 = 0;
    }

    pub fn move_to_line_end(&mut self) {
        self.cursor.1 = self.lines[self.cursor.0].len();
    }

    pub fn move_word_left(&mut self) {
        let line = &self.lines[self.cursor.0];
        if self.cursor.1 == 0 {
            if self.cursor.0 > 0 {
                self.cursor.0 -= 1;
                self.cursor.1 = self.lines[self.cursor.0].len();
                self.move_word_left();
            }
            return;
        }

        let chars: Vec<char> = line.chars().collect();
        let mut idx = self.cursor.1.min(chars.len());

        while idx > 0 && chars[idx - 1].is_whitespace() {
            idx -= 1;
        }
        while idx > 0 && !chars[idx - 1].is_whitespace() {
            idx -= 1;
        }

        self.cursor.1 = idx;
    }

    pub fn move_word_right(&mut self) {
        let line = &self.lines[self.cursor.0];
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        let mut idx = self.cursor.1.min(len);

        while idx < len && chars[idx].is_whitespace() {
            idx += 1;
        }
        while idx < len && !chars[idx].is_whitespace() {
            idx += 1;
        }

        if idx == len && self.cursor.0 < self.lines.len() - 1 {
            self.cursor.0 += 1;
            self.cursor.1 = 0;
            self.move_word_right();
        } else {
            self.cursor.1 = idx;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Buffer;

    #[test]
    fn move_to_line_start_sets_column_to_zero() {
        let mut buffer = Buffer::from_text(0, "*test*", "abcdef");
        buffer.cursor = (0, 4);
        buffer.move_to_line_start();
        assert_eq!(buffer.cursor, (0, 0));
    }

    #[test]
    fn move_to_line_end_sets_column_to_line_length() {
        let mut buffer = Buffer::from_text(0, "*test*", "abcdef");
        buffer.cursor = (0, 1);
        buffer.move_to_line_end();
        assert_eq!(buffer.cursor, (0, 6));
    }

    #[test]
    fn move_word_left_stops_at_previous_word_boundary() {
        let mut buffer = Buffer::from_text(0, "*test*", "abc def ghi");
        buffer.cursor = (0, 10);
        buffer.move_word_left();
        assert_eq!(buffer.cursor, (0, 8));
    }

    #[test]
    fn move_word_right_stops_at_next_word_boundary() {
        let mut buffer = Buffer::from_text(0, "*test*", "abc def ghi");
        buffer.cursor = (0, 0);
        buffer.move_word_right();
        assert_eq!(buffer.cursor, (0, 3));
    }

    #[test]
    fn delete_word_before_removes_previous_word_and_space() {
        let mut buffer = Buffer::from_text(0, "*test*", "abc def ghi");
        buffer.cursor = (0, 8);
        buffer.delete_word_before();
        assert_eq!(buffer.text(), "abc ghi");
        assert_eq!(buffer.cursor, (0, 4));
    }

    #[test]
    fn delete_word_before_joins_previous_line_when_at_line_start() {
        let mut buffer = Buffer::from_text(0, "*test*", "abc def\nghi");
        buffer.cursor = (1, 0);
        buffer.delete_word_before();
        assert_eq!(buffer.text(), "abc ghi");
        assert_eq!(buffer.cursor, (0, 4));
    }

    #[test]
    fn delete_to_line_end_truncates_current_line() {
        let mut buffer = Buffer::from_text(0, "*test*", "abc def ghi");
        buffer.cursor = (0, 4);
        buffer.delete_to_line_end();
        assert_eq!(buffer.text(), "abc ");
        assert_eq!(buffer.cursor, (0, 4));
    }

    #[test]
    fn delete_to_line_end_at_eol_joins_next_line() {
        let mut buffer = Buffer::from_text(0, "*test*", "abc\ndef");
        buffer.cursor = (0, 3);
        buffer.delete_to_line_end();
        assert_eq!(buffer.text(), "abcdef");
        assert_eq!(buffer.cursor, (0, 3));
    }
}
