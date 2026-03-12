pub struct Buffer {
    pub name: String,
    /// Text contents as lines. Empty buffer starts as vec![""]
    pub lines: Vec<String>,
    /// Cursor position (row, col), 0-indexed. Col can be 0..=line.len().
    pub cursor: (usize, usize),
    pub dirty: bool,
    /// First visible line index (for scrolling).
    pub scroll_top: usize,
}

impl Buffer {
    pub fn new(name: impl Into<String>) -> Self {
        Buffer {
            name: name.into(),
            lines: vec![String::new()],
            cursor: (0, 0),
            dirty: false,
            scroll_top: 0,
        }
    }

    /// Adjust scroll_top so the cursor stays within the visible viewport.
    ///
    /// TODO(human): implement this function.
    ///
    /// Hints:
    ///   - If viewport_height is 0, return early (nothing to display).
    ///   - If the cursor row is above scroll_top, scroll up: set scroll_top = cursor row.
    ///   - If the cursor row is at or below scroll_top + viewport_height, scroll down.
    ///   - The last visible row is scroll_top + viewport_height - 1, so the new
    ///     scroll_top when scrolling down is cursor.0 - viewport_height + 1.
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
}
