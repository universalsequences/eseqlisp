pub enum Direction {
    Forward,
    Backward,
}

pub struct SExpParser<'a> {
    lines: &'a [String],
    cursor: (usize, usize),
    reached_start: bool,
    reached_end: bool,
    direction: Direction,
}

impl<'a> SExpParser<'a> {
    pub fn new(lines: &'a [String], cursor: (usize, usize)) -> Self {
        SExpParser {
            lines,
            cursor,
            reached_start: false,
            reached_end: false,
            direction: Direction::Backward,
        }
    }

    pub fn set_direction(&mut self, direction: Direction) {
        self.direction = direction;
    }

    pub fn position(&self) -> (usize, usize) {
        self.cursor
    }

    pub fn peek(&self) -> Option<char> {
        if self.reached_start || self.reached_end {
            return None;
        }
        if let Some(line) = self.lines.get(self.cursor.0) {
            return line.chars().nth(self.cursor.1);
        }
        None
    }

    pub fn next(&mut self) -> Option<char> {
        if self.reached_start || self.reached_end {
            return None;
        }
        if let Some(line) = self.lines.get(self.cursor.0) {
            let next = line.chars().nth(self.cursor.1);
            match self.direction {
                Direction::Backward => {
                    if self.cursor.1 > 0 {
                        self.cursor.1 = self.cursor.1.saturating_sub(1);
                    } else if self.cursor.0 == 0 {
                        self.reached_start = true;
                    } else {
                        self.cursor.0 = self.cursor.0.saturating_sub(1);
                        if let Some(prev_line) = self.lines.get(self.cursor.0) {
                            if prev_line.is_empty() {
                                self.cursor.1 = 0;
                            } else {
                                self.cursor.1 = prev_line.len() - 1;
                            }
                        }
                    }
                }
                Direction::Forward => {
                    if line.is_empty() || self.cursor.1 == line.len() - 1 {
                        // next line
                        if self.cursor.0 < self.lines.len() - 1 {
                            self.cursor.0 = self.cursor.0.saturating_add(1);
                            self.cursor.1 = 0;
                        } else {
                            self.reached_end = true;
                        }
                    } else {
                        // move cursor right
                        self.cursor.1 = self.cursor.1.saturating_add(1);
                    }
                }
            }
            return next;
        }
        None
    }
}

/// Find the matching paren for the paren under the cursor.
///
/// Returns the (row, col) of the matching paren, or None if the cursor is
/// not on a paren or no match exists.
pub fn matching_paren(lines: &[String], cursor: (usize, usize)) -> Option<(usize, usize)> {
    let mut cursor = cursor;
    if let Some(line) = lines.get(cursor.0)
        && !line.is_empty()
        && cursor.1 >= line.len()
    {
        cursor.1 = line.len() - 1;
    }

    let mut parser = SExpParser::new(lines, cursor);
    if let Some(ch) = parser.peek()
        && (ch == '(' || ch == ')')
    {
        parser.set_direction(match ch {
            '(' => Direction::Forward,
            _ => Direction::Backward,
        });
        follow_parens(&mut parser);
        return Some(parser.position());
    }
    None
}

pub fn follow_parens(parser: &mut SExpParser) -> String {
    let mut p = 0;
    let mut s = "".to_string();
    while let Some(ch) = parser.peek() {
        match ch {
            ')' => match parser.direction {
                Direction::Forward => {
                    p -= 1;
                    if p <= 0 {
                        s += &ch.to_string();
                        break;
                    }
                }
                Direction::Backward => {
                    p += 1;
                }
            },
            '(' => match parser.direction {
                Direction::Backward => {
                    p -= 1;
                    if p <= 0 {
                        s += &ch.to_string();
                        break;
                    }
                }
                Direction::Forward => {
                    p += 1;
                }
            },
            _ => {}
        }
        s += &ch.to_string();
        parser.next();
    }
    s
}

/// Find the outermost s-expression at or enclosing the cursor.
///
/// Returns the text of the sexp as a String, or None if the cursor
/// is not inside any parenthesized expression.
///
///
/// Hints:
///   - Flatten `lines` into one string (joining with '\n'), then find the
///     cursor's offset in that flat string.
///   - Scan backwards from the cursor to find the opening '(', tracking
///     nesting depth so you find the *outermost* enclosing paren.
///   - From that '(', scan forwards to the matching ')'.
///   - Return the substring from '(' to ')' inclusive.
pub fn sexp_at_cursor(lines: &[String], cursor: (usize, usize)) -> Option<String> {
    let mut cursor = cursor;
    if let Some(line) = lines.get(cursor.0)
        && !line.is_empty()
        && cursor.1 >= line.len()
    {
        cursor.1 = line.len() - 1;
    }
    let mut parser = SExpParser::new(lines, cursor);
    _ = follow_parens(&mut parser);
    parser.set_direction(Direction::Forward);
    let s = follow_parens(&mut parser);
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::sexp_at_cursor;

    #[test]
    fn test_sexp_at_cursor() {
        let lines = [
            "alec".to_string(),
            "hello".to_string(),
            "(+ a ( a ) ".to_string(),
            " a ) a".to_string(),
        ];
        let cursor = (3, 3);
        let s = sexp_at_cursor(&lines, cursor);
        assert_eq!(s.unwrap(), "(+ a ( a )  a )", "s exp match");
    }

    #[test]
    fn test_sexp_before_cursor() {
        let lines = ["(+ 5 4)".to_string(), "hello".to_string()];
        let cursor = (1, 5);
        let s = sexp_at_cursor(&lines, cursor);
        assert_eq!(s.unwrap(), "(+ 5 4)", "sexp match");
    }
}
