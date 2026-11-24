use std::collections::VecDeque;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Ink {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub fg: Ink,
    pub bg: Ink,
    pub bright: bool,
    pub reverse: bool,
    pub underline: bool,
}

#[derive(Clone)]
pub struct Cell {
    pub ch: char,
    pub width: u8,
    pub marks: String,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            width: 1,
            marks: String::new(),
            style: Style::default(),
        }
    }
}

enum Parse {
    Text,
    Escape,
    Charset,
    Csi(Vec<u8>),
    Ignore(bool),
}
type Line = Vec<Cell>;

struct Grid {
    lines: Vec<Line>,
    row: usize,
    col: usize,
    saved: (usize, usize, Style),
    top: usize,
    bottom: usize,
    wrap_pending: bool,
}

impl Grid {
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            lines: vec![vec![Cell::default(); cols]; rows],
            row: 0,
            col: 0,
            saved: (0, 0, Style::default()),
            top: 0,
            bottom: rows - 1,
            wrap_pending: false,
        }
    }
    fn resize(&mut self, cols: usize, rows: usize) -> Vec<Line> {
        let remove = self.row.saturating_add(1).saturating_sub(rows);
        let lost = self.lines.drain(..remove).collect();
        self.row -= remove;
        self.lines.resize_with(rows, || vec![Cell::default(); cols]);
        for line in &mut self.lines {
            line.resize(cols, Cell::default());
            if line[cols - 1].width == 2 {
                line[cols - 1] = Cell::default();
            }
        }
        self.col = self.col.min(cols - 1);
        self.top = 0;
        self.bottom = rows - 1;
        self.wrap_pending = false;
        lost
    }
}

pub struct Screen {
    grid: Grid,
    normal: Option<Grid>,
    history: VecDeque<Line>,
    parser: Parse,
    utf8: Vec<u8>,
    cols: usize,
    rows: usize,
    style: Style,
    wrap: bool,
    origin: bool,
    pub cursor: bool,
    pub application_cursor: bool,
    pub bracketed_paste: bool,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            grid: Grid::new(cols, rows),
            normal: None,
            history: VecDeque::new(),
            parser: Parse::Text,
            utf8: Vec::with_capacity(4),
            cols,
            rows,
            style: Style::default(),
            wrap: true,
            origin: false,
            cursor: true,
            application_cursor: false,
            bracketed_paste: false,
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        if self.shape() == (cols, rows) {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        let lost = self.grid.resize(cols, rows);
        let lost = if let Some(normal) = &mut self.normal {
            normal.resize(cols, rows)
        } else {
            lost
        };
        for line in lost {
            self.remember(line);
        }
    }

    pub fn shape(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }
    pub fn history(&self) -> usize {
        if self.normal.is_none() {
            self.history.len()
        } else {
            0
        }
    }
    pub fn line(&self, row: usize) -> Option<&[Cell]> {
        let history = self.history();
        if row < history {
            self.history.get(row).map(Vec::as_slice)
        } else {
            self.grid.lines.get(row - history).map(Vec::as_slice)
        }
    }
    pub fn position(&self) -> (usize, usize) {
        (self.grid.row, self.grid.col)
    }

    pub fn note(&mut self, message: &str) {
        self.parser = Parse::Text;
        self.utf8.clear();
        if let Some(normal) = self.normal.take() {
            self.grid = normal;
        }
        self.style = Style::default();
        self.grid.col = 0;
        self.line_feed();
        for ch in message.chars().filter(|ch| !ch.is_control()) {
            self.print(ch);
        }
    }

    fn remember(&mut self, line: Line) {
        if self.history.len() >= 1000 {
            self.history.pop_front();
        }
        self.history.push_back(line);
    }
    fn blank(&self) -> Cell {
        Cell {
            style: Style {
                bg: self.style.bg,
                ..Style::default()
            },
            ..Cell::default()
        }
    }
    fn blank_line(&self) -> Line {
        vec![self.blank(); self.cols]
    }
    fn scroll(&mut self, first: usize, last: usize, count: usize, up: bool, remember: bool) {
        for _ in 0..count.min(last - first + 1) {
            let removed = self.grid.lines.remove(if up { first } else { last });
            if up && remember && first == 0 && last + 1 == self.rows && self.normal.is_none() {
                self.remember(removed);
            }
            self.grid
                .lines
                .insert(if up { last } else { first }, self.blank_line());
        }
    }
    fn line_feed(&mut self) {
        self.grid.wrap_pending = false;
        if self.grid.row == self.grid.bottom {
            self.scroll(self.grid.top, self.grid.bottom, 1, true, true);
        } else {
            self.grid.row = (self.grid.row + 1).min(self.rows - 1);
        }
    }

    fn print(&mut self, ch: char) {
        let width = char_width(ch);
        if width == 0 {
            let col = if self.grid.wrap_pending {
                self.grid.col
            } else {
                self.grid.col.saturating_sub(1)
            };
            let line = &mut self.grid.lines[self.grid.row];
            let col = if line[col].width == 0 {
                col.saturating_sub(1)
            } else {
                col
            };
            if line[col].marks.len() < 24 {
                line[col].marks.push(ch);
            }
            return;
        }
        if self.grid.wrap_pending || self.grid.col + width > self.cols {
            if self.wrap {
                self.grid.col = 0;
                self.line_feed();
            } else if width > 1 {
                return;
            }
        }
        let col = self.grid.col;
        let line = &mut self.grid.lines[self.grid.row];
        if line[col].width == 0 && col > 0 {
            line[col - 1] = Cell::default();
        }
        if width == 2 {
            if line[col + 1].width == 2 && col + 2 < self.cols {
                line[col + 2] = Cell::default();
            }
            line[col + 1] = Cell {
                width: 0,
                style: self.style,
                ..Cell::default()
            };
        } else if line[col].width == 2 && col + 1 < self.cols {
            line[col + 1] = Cell::default();
        }
        line[col] = Cell {
            ch,
            width: width as u8,
            marks: String::new(),
            style: self.style,
        };
        self.grid.col = (col + width).min(self.cols - 1);
        self.grid.wrap_pending = self.wrap && col + width == self.cols;
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut response = Vec::new();
        for &byte in bytes {
            match std::mem::replace(&mut self.parser, Parse::Text) {
                Parse::Text => self.input(byte),
                Parse::Escape => match byte {
                    b'[' => self.parser = Parse::Csi(Vec::new()),
                    b']' | b'P' | b'_' | b'^' | b'X' => self.parser = Parse::Ignore(false),
                    b'(' | b')' | b'*' | b'+' | b'#' => self.parser = Parse::Charset,
                    b'7' => self.save(),
                    b'8' => self.restore(),
                    b'D' => self.line_feed(),
                    b'E' => {
                        self.grid.col = 0;
                        self.line_feed();
                    }
                    b'M' if self.grid.row == self.grid.top => {
                        self.scroll(self.grid.top, self.grid.bottom, 1, false, false)
                    }
                    b'M' => self.grid.row = self.grid.row.saturating_sub(1),
                    b'c' => *self = Self::new(self.cols, self.rows),
                    0x1b => self.parser = Parse::Escape,
                    _ => {}
                },
                Parse::Charset => {}
                Parse::Ignore(escaped) => {
                    if byte != 7 && !(escaped && byte == b'\\') {
                        self.parser = Parse::Ignore(byte == 0x1b);
                    }
                }
                Parse::Csi(mut seq) => match byte {
                    0x40..=0x7e => self.control(&seq, byte, &mut response),
                    0x20..=0x3f if seq.len() < 128 => {
                        seq.push(byte);
                        self.parser = Parse::Csi(seq);
                    }
                    0x1b => self.parser = Parse::Escape,
                    _ => {}
                },
            }
        }
        response
    }

    fn input(&mut self, byte: u8) {
        if !self.utf8.is_empty() || byte >= 0x80 {
            self.utf8.push(byte);
            match std::str::from_utf8(&self.utf8) {
                Ok(text) => {
                    let ch = text.chars().next().unwrap_or('�');
                    self.utf8.clear();
                    self.print(ch);
                }
                Err(error) if error.error_len().is_none() => {}
                Err(error) => {
                    let retry = self.utf8.len() > error.error_len().unwrap_or_default();
                    self.utf8.clear();
                    self.print('�');
                    if retry {
                        self.input(byte);
                    }
                }
            }
            return;
        }
        match byte {
            0x1b => self.parser = Parse::Escape,
            b'\r' => {
                self.grid.col = 0;
                self.grid.wrap_pending = false;
            }
            b'\n' | 0x0b | 0x0c => self.line_feed(),
            8 => {
                self.grid.col = self.grid.col.saturating_sub(1);
                self.grid.wrap_pending = false;
            }
            b'\t' => {
                self.grid.col = ((self.grid.col / 8 + 1) * 8).min(self.cols - 1);
                self.grid.wrap_pending = false;
            }
            0x20..=0x7e => self.print(char::from(byte)),
            _ => {}
        }
    }

    fn control(&mut self, seq: &[u8], tag: u8, response: &mut Vec<u8>) {
        let marker = seq
            .first()
            .copied()
            .filter(|c| matches!(c, b'?' | b'>' | b'='));
        let seq = if marker.is_some() { &seq[1..] } else { seq };
        let len = seq
            .iter()
            .take_while(|c| matches!(c, b'0'..=b'9' | b';' | b':'))
            .count();
        let params: Vec<usize> = seq[..len]
            .split(|c| matches!(c, b';' | b':'))
            .map(|v| {
                std::str::from_utf8(v)
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0)
            })
            .collect();
        let value = |i| params.get(i).copied().unwrap_or(0);
        let n: usize = value(0).max(1);
        let min_row = if self.origin { self.grid.top } else { 0 };
        let max_row = if self.origin {
            self.grid.bottom
        } else {
            self.rows - 1
        };
        if matches!(
            tag,
            b'A'..=b'H' | b'J' | b'K' | 0x60 | b'a' | b'd' | b'e' | b'f' | b'r'
        ) {
            self.grid.wrap_pending = false;
        }
        match tag {
            b'A' | b'F' => {
                self.grid.row = self.grid.row.saturating_sub(n).max(min_row);
                if tag == b'F' {
                    self.grid.col = 0;
                }
            }
            b'B' | b'E' | b'e' => {
                self.grid.row = self.grid.row.saturating_add(n).min(max_row);
                if tag == b'E' {
                    self.grid.col = 0;
                }
            }
            b'C' | b'a' => self.grid.col = self.grid.col.saturating_add(n).min(self.cols - 1),
            b'D' => self.grid.col = self.grid.col.saturating_sub(n),
            b'G' | 0x60 => self.grid.col = (n - 1).min(self.cols - 1),
            b'd' => self.grid.row = min_row.saturating_add(n - 1).min(max_row),
            b'H' | b'f' => {
                self.grid.row = min_row.saturating_add(n - 1).min(max_row);
                self.grid.col = (value(1).max(1) - 1).min(self.cols - 1);
            }
            b'J' => self.erase_display(value(0)),
            b'K' => self.erase_line(value(0)),
            b'X' => self.erase(
                self.grid.row,
                self.grid.col,
                self.grid.col.saturating_add(n).min(self.cols),
            ),
            b'@' | b'P' => {
                let blank = self.blank();
                let col = self.grid.col;
                let n = n.min(self.cols - col);
                let line = &mut self.grid.lines[self.grid.row];
                if tag == b'@' {
                    line[col..].rotate_right(n);
                    line[col..col + n].fill(blank);
                } else {
                    line[col..].rotate_left(n);
                    line[self.cols - n..].fill(blank);
                }
            }
            b'L' | b'M' if (self.grid.top..=self.grid.bottom).contains(&self.grid.row) => {
                self.scroll(self.grid.row, self.grid.bottom, n, tag == b'M', false)
            }
            b'S' | b'T' => self.scroll(self.grid.top, self.grid.bottom, n, tag == b'S', true),
            b'r' if marker.is_none() => {
                let bottom = if value(1) == 0 {
                    self.rows
                } else {
                    value(1).min(self.rows)
                };
                if n < bottom {
                    self.grid.top = n - 1;
                    self.grid.bottom = bottom - 1;
                    self.grid.row = if self.origin { n - 1 } else { 0 };
                    self.grid.col = 0;
                }
            }
            b'm' => self.colors(&params),
            b's' => self.save(),
            b'u' => self.restore(),
            b'h' | b'l' if marker == Some(b'?') => {
                let enabled = tag == b'h';
                for mode in params {
                    match mode {
                        1 => self.application_cursor = enabled,
                        6 => {
                            self.origin = enabled;
                            self.grid.row = if enabled { self.grid.top } else { 0 };
                            self.grid.col = 0;
                        }
                        7 => {
                            self.wrap = enabled;
                            self.grid.wrap_pending = false;
                        }
                        25 => self.cursor = enabled,
                        47 | 1047 | 1049 if enabled && self.normal.is_none() => {
                            self.normal = Some(std::mem::replace(
                                &mut self.grid,
                                Grid::new(self.cols, self.rows),
                            ));
                        }
                        47 | 1047 | 1049 if !enabled => {
                            if let Some(normal) = self.normal.take() {
                                self.grid = normal;
                            }
                        }
                        2004 => self.bracketed_paste = enabled,
                        _ => {}
                    }
                }
            }
            b'n' if value(0) == 5 => response.extend(b"\x1b[0n"),
            b'n' if value(0) == 6 => response
                .extend(format!("\x1b[{};{}R", self.grid.row + 1, self.grid.col + 1).bytes()),
            b'c' if marker == Some(b'>') => response.extend(b"\x1b[>0;1;0c"),
            b'c' => response.extend(b"\x1b[?1;2c"),
            _ => {}
        }
    }

    fn save(&mut self) {
        self.grid.saved = (self.grid.row, self.grid.col, self.style);
    }
    fn restore(&mut self) {
        let (row, col, style) = self.grid.saved;
        self.grid.row = row.min(self.rows - 1);
        self.grid.col = col.min(self.cols - 1);
        self.grid.wrap_pending = false;
        self.style = style;
    }
    fn erase(&mut self, row: usize, mut start: usize, mut end: usize) {
        if start >= end {
            return;
        }
        let blank = self.blank();
        let line = &mut self.grid.lines[row];
        if line[start].width == 0 {
            start = start.saturating_sub(1);
        }
        if end < self.cols && line[end].width == 0 {
            end += 1;
        }
        line[start..end].fill(blank);
    }
    fn erase_line(&mut self, mode: usize) {
        let (start, end) = match mode {
            0 => (self.grid.col, self.cols),
            1 => (0, self.grid.col + 1),
            2 => (0, self.cols),
            _ => return,
        };
        self.erase(self.grid.row, start, end);
    }
    fn erase_display(&mut self, mode: usize) {
        match mode {
            0 => {
                self.erase_line(0);
                for row in self.grid.row + 1..self.rows {
                    self.grid.lines[row] = self.blank_line();
                }
            }
            1 => {
                self.erase_line(1);
                for row in 0..self.grid.row {
                    self.grid.lines[row] = self.blank_line();
                }
            }
            2 => {
                for row in 0..self.rows {
                    self.grid.lines[row] = self.blank_line();
                }
            }
            3 if self.normal.is_none() => self.history.clear(),
            _ => {}
        }
    }
    fn colors(&mut self, params: &[usize]) {
        let mut params = params.iter().copied();
        while let Some(n) = params.next() {
            match n {
                0 => self.style = Style::default(),
                1 => self.style.bright = true,
                22 => self.style.bright = false,
                4 => self.style.underline = true,
                24 => self.style.underline = false,
                7 => self.style.reverse = true,
                27 => self.style.reverse = false,
                30..=37 => self.style.fg = Ink::Indexed((n - 30) as u8),
                40..=47 => self.style.bg = Ink::Indexed((n - 40) as u8),
                90..=97 => self.style.fg = Ink::Indexed((n - 82) as u8),
                100..=107 => self.style.bg = Ink::Indexed((n - 92) as u8),
                39 => self.style.fg = Ink::Default,
                49 => self.style.bg = Ink::Default,
                38 | 48 => {
                    let color = match params.next() {
                        Some(5) => Ink::Indexed(params.next().unwrap_or(0).min(255) as u8),
                        Some(2) => {
                            let mut component = || params.next().unwrap_or(0).min(255) as u8;
                            Ink::Rgb(component(), component(), component())
                        }
                        _ => continue,
                    };
                    if n == 38 {
                        self.style.fg = color;
                    } else {
                        self.style.bg = color;
                    }
                }
                _ => {}
            }
        }
    }
}

fn char_width(ch: char) -> usize {
    match ch as u32 {
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x200b..=0x200f | 0xfe00..=0xfe0f => {
            0
        }
        0x1100..=0x115f
        | 0x2329..=0x232a
        | 0x2e80..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe10..=0xfe6f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6
        | 0x1f300..=0x1faff
        | 0x20000..=0x3fffd => 2,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragmented_utf8_cursor_and_alternate_buffer() {
        let mut screen = Screen::new(16, 4);
        let text = "한A".as_bytes();
        screen.feed(&text[..2]);
        screen.feed(&text[2..]);
        assert_eq!(screen.line(0).unwrap()[0].ch, '한');
        assert_eq!(screen.line(0).unwrap()[1].width, 0);
        assert_eq!(screen.line(0).unwrap()[2].ch, 'A');
        screen.feed(b"\x1b[?1049hother");
        assert_eq!(screen.line(0).unwrap()[0].ch, 'o');
        screen.feed(b"\x1b[?1049l\x1b[5;2H");
        assert_eq!(screen.line(0).unwrap()[0].ch, '한');
        assert_eq!(screen.feed(b"\x1b[6n"), b"\x1b[4;2R");
    }
    #[test]
    fn scrollback_and_erase() {
        let mut screen = Screen::new(16, 3);
        screen.feed(b"zero\r\none\r\ntwo\r\nthree\x1b[2K");
        assert_eq!(screen.history(), 1);
        assert_eq!(screen.line(0).unwrap()[0].ch, 'z');
        assert_eq!(screen.line(3).unwrap()[0].ch, ' ');
        screen.feed(b"\x1b[3J");
        assert_eq!(screen.history(), 0);
    }
}
