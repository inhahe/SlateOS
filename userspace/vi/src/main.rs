//! vi -- modal terminal text editor for SlateOS.
//!
//! A functional vi/vim-like editor with Normal, Insert, Visual, and Command
//! modes. Uses VT100 escape sequences for full-screen rendering and POSIX
//! termios for raw-mode input.
//!
//! # Usage
//!
//! ```text
//! vi [FILE]
//! ```
//!
//! # Modes
//!
//! - **Normal**: cursor movement, editing commands, mode transitions
//! - **Insert**: text entry; Escape returns to Normal
//! - **Visual**: character/line selection; y/d/>/< operate on selection
//! - **Command**: colon commands (w, q, s, set, e, …)

#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_possible_wrap)]

use std::env;
use std::fs;
use std::io::{self, Read, Write};

// ============================================================================
// Minimal libc bindings for termios
// ============================================================================

#[cfg(unix)]
mod libc {
    //! Minimal termios / ioctl bindings for raw-mode terminal control.

    pub type TcflagT = u32;
    pub type CcT = u8;
    pub type SpeedT = u32;

    pub const NCCS: usize = 32;

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct Termios {
        pub c_iflag: TcflagT,
        pub c_oflag: TcflagT,
        pub c_cflag: TcflagT,
        pub c_lflag: TcflagT,
        pub c_line: CcT,
        pub c_cc: [CcT; NCCS],
        pub c_ispeed: SpeedT,
        pub c_ospeed: SpeedT,
    }

    pub const BRKINT: TcflagT = 0o000002;
    pub const ICRNL: TcflagT = 0o000400;
    pub const INPCK: TcflagT = 0o000020;
    pub const ISTRIP: TcflagT = 0o000040;
    pub const IXON: TcflagT = 0o002000;
    pub const OPOST: TcflagT = 0o000001;
    pub const CS8: TcflagT = 0o000060;
    pub const ECHO: TcflagT = 0o000010;
    pub const ICANON: TcflagT = 0o000002;
    pub const ISIG: TcflagT = 0o000001;
    pub const IEXTEN: TcflagT = 0o100000;
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;
    pub const TCSAFLUSH: i32 = 2;

    #[repr(C)]
    pub struct Winsize {
        pub ws_row: u16,
        pub ws_col: u16,
        pub ws_xpixel: u16,
        pub ws_ypixel: u16,
    }

    pub const TIOCGWINSZ: u64 = 0x5413;

    unsafe extern "C" {
        pub fn tcgetattr(fd: i32, termios_p: *mut Termios) -> i32;
        pub fn tcsetattr(fd: i32, action: i32, termios_p: *const Termios) -> i32;
        pub fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
}

// ============================================================================
// Terminal raw mode
// ============================================================================

fn enable_raw_mode() -> Option<RawModeGuard> {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let mut orig: libc::Termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
            return None;
        }
        let mut raw = orig;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return None;
        }
        Some(RawModeGuard { orig })
    }
    #[cfg(not(unix))]
    {
        Some(RawModeGuard {})
    }
}

struct RawModeGuard {
    #[cfg(unix)]
    orig: libc::Termios,
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = io::stdin().as_raw_fd();
            let _ = unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &self.orig) };
        }
    }
}

fn get_terminal_size() -> (usize, usize) {
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdout().as_raw_fd();
        let mut ws: libc::Winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            return (ws.ws_row as usize, ws.ws_col as usize);
        }
    }
    // The ANSI cursor-position-report fallback (move cursor to the far
    // bottom-right, then ask the terminal where it ended up) requires an
    // interactive terminal: it writes a query and then *blocks* reading the
    // reply from stdin. In a pipe, file redirect, or test harness no reply
    // ever arrives and the read would hang forever, so only attempt the
    // handshake when both stdin and stdout are real terminals.
    use std::io::IsTerminal;
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return (24, 80);
    }
    let _ = io::stdout().write_all(b"\x1b[999;999H\x1b[6n");
    let _ = io::stdout().flush();
    let mut buf = Vec::with_capacity(32);
    let mut b = [0u8; 1];
    while let Ok(1) = io::stdin().lock().read(&mut b) {
        buf.push(b[0]);
        if b[0] == b'R' {
            break;
        }
    }
    if let Some(start) = buf.iter().position(|&c| c == b'[') {
        let s = std::str::from_utf8(&buf[start + 1..]).unwrap_or("");
        let mut parts = s.trim_end_matches('R').split(';');
        let rows = parts.next().and_then(|p| p.parse().ok()).unwrap_or(24);
        let cols = parts.next().and_then(|p| p.parse().ok()).unwrap_or(80);
        return (rows, cols);
    }
    (24, 80)
}

// ============================================================================
// Terminal output helpers
// ============================================================================

fn write_str(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}
fn write_bytes(b: &[u8]) {
    let _ = io::stdout().write_all(b);
}
fn flush() {
    let _ = io::stdout().flush();
}
fn enter_alternate_screen() {
    write_str("\x1b[?1049h");
}
fn leave_alternate_screen() {
    write_str("\x1b[?1049l");
}

// ============================================================================
// Key input
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Char(char),
    Ctrl(char),
    Escape,
    Enter,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F1,
    Unknown(Vec<u8>),
}

fn read_byte() -> Option<u8> {
    let mut b = [0u8; 1];
    match io::stdin().lock().read(&mut b) {
        Ok(1) => Some(b[0]),
        _ => None,
    }
}

fn read_key() -> Key {
    loop {
        let Some(b) = read_byte() else { continue };
        return match b {
            0x1b => parse_escape_sequence(),
            0x0d | 0x0a => Key::Enter,
            0x7f | 0x08 => Key::Backspace,
            0x01..=0x1a => Key::Ctrl((b - 0x01 + b'a') as char),
            0x20..=0x7e => Key::Char(b as char),
            _ => Key::Unknown(vec![b]),
        };
    }
}

fn parse_escape_sequence() -> Key {
    let Some(b2) = read_byte() else {
        return Key::Escape;
    };
    match b2 {
        b'[' => {
            let mut params: Vec<u8> = Vec::new();
            loop {
                let Some(b) = read_byte() else {
                    return Key::Unknown(params);
                };
                if b.is_ascii_uppercase() || b == b'~' {
                    return decode_csi(&params, b);
                }
                params.push(b);
            }
        }
        b'O' => {
            let Some(b) = read_byte() else {
                return Key::Escape;
            };
            match b {
                b'P' => Key::F1,
                b'H' => Key::Home,
                b'F' => Key::End,
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                _ => Key::Unknown(vec![0x1b, b'O', b]),
            }
        }
        0x01..=0x1a => Key::Ctrl((b2 - 0x01 + b'a') as char),
        _ => Key::Escape,
    }
}

fn decode_csi(params: &[u8], final_byte: u8) -> Key {
    let p = std::str::from_utf8(params).unwrap_or("");
    match (p, final_byte) {
        ("", b'A') | ("1", b'A') => Key::Up,
        ("", b'B') | ("1", b'B') => Key::Down,
        ("", b'C') | ("1", b'C') => Key::Right,
        ("", b'D') | ("1", b'D') => Key::Left,
        ("", b'H') | ("1", b'H') => Key::Home,
        ("", b'F') | ("1", b'F') => Key::End,
        ("5", b'~') => Key::PageUp,
        ("6", b'~') => Key::PageDown,
        ("1", b'~') => Key::Home,
        ("4", b'~') => Key::End,
        ("3", b'~') => Key::Delete,
        ("11", b'~') => Key::F1,
        _ => Key::Unknown(params.to_vec()),
    }
}

// ============================================================================
// Undo / redo
// ============================================================================

#[derive(Clone, Debug)]
#[allow(dead_code)] // Insert variant reserved for future fine-grained undo
enum UndoOp {
    Insert {
        row: usize,
        col: usize,
        text: String,
    },
    Delete {
        row: usize,
        col: usize,
        text: String,
    },
    ReplaceLines {
        start_row: usize,
        old_lines: Vec<String>,
        new_lines: Vec<String>,
    },
}

// ============================================================================
// Mode
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Insert,
    VisualChar,
    VisualLine,
    Command,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Insert => "INSERT",
            Mode::VisualChar => "VISUAL",
            Mode::VisualLine => "V-LINE",
            Mode::Command => "COMMAND",
        }
    }
}

// ============================================================================
// Insert motion (for . repeat)
// ============================================================================

#[derive(Clone, Debug, Copy)]
enum InsertMotion {
    InsertBefore,
    InsertAfter,
    InsertBOL,
    InsertEOL,
    OpenBelow,
    OpenAbove,
}

// ============================================================================
// Last change (for . repeat)
// ============================================================================

#[derive(Clone, Debug)]
enum LastChange {
    None,
    InsertText { motion: InsertMotion, text: String },
    DeleteChar,
    DeleteLine,
    YankLine,
    ReplaceChar(char),
    JoinLines,
    ToggleCase,
    IndentRight,
    IndentLeft,
    PasteAfter,
    PasteBefore,
}

// ============================================================================
// Register
// ============================================================================

#[derive(Clone, Debug)]
enum Register {
    Chars(String),
    Lines(Vec<String>),
}

// ============================================================================
// Editor
// ============================================================================

struct Editor {
    lines: Vec<String>,
    mode: Mode,
    row: usize,
    col: usize,
    scroll: usize,
    term_rows: usize,
    term_cols: usize,
    show_numbers: bool,
    filename: Option<String>,
    modified: bool,
    undo_stack: Vec<UndoOp>,
    undo_pos: usize,
    register: Option<Register>,
    message: String,
    visual_anchor: (usize, usize),
    cmd_buf: String,
    search_pat: String,
    search_forward: bool,
    last_change: LastChange,
    count_buf: String,
}

impl Editor {
    fn new() -> Self {
        let (rows, cols) = get_terminal_size();
        Self {
            lines: vec![String::new()],
            mode: Mode::Normal,
            row: 0,
            col: 0,
            scroll: 0,
            term_rows: rows,
            term_cols: cols,
            show_numbers: false,
            filename: None,
            modified: false,
            undo_stack: Vec::new(),
            undo_pos: 0,
            register: None,
            message: String::new(),
            visual_anchor: (0, 0),
            cmd_buf: String::new(),
            search_pat: String::new(),
            search_forward: true,
            last_change: LastChange::None,
            count_buf: String::new(),
        }
    }

    // --- File I/O ---

    fn load_file(&mut self, path: &str) -> Result<(), String> {
        match fs::read_to_string(path) {
            Ok(content) => {
                let mut ls: Vec<String> = content
                    .split('\n')
                    .map(|l| l.trim_end_matches('\r').to_owned())
                    .collect();
                if ls.last().map(|l| l.is_empty()).unwrap_or(false) {
                    ls.pop();
                }
                if ls.is_empty() {
                    ls.push(String::new());
                }
                self.lines = ls;
                self.filename = Some(path.to_owned());
                self.modified = false;
                self.row = 0;
                self.col = 0;
                self.scroll = 0;
                self.undo_stack.clear();
                self.undo_pos = 0;
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.filename = Some(path.to_owned());
                Ok(())
            }
            Err(e) => Err(format!("Cannot open '{}': {}", path, e)),
        }
    }

    fn save_file(&mut self, path: &str) -> Result<(), String> {
        let mut content = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(line);
        }
        content.push('\n');
        fs::write(path, content.as_bytes())
            .map_err(|e| format!("Cannot write '{}': {}", path, e))?;
        self.modified = false;
        self.filename = Some(path.to_owned());
        Ok(())
    }

    // --- Undo/redo ---

    fn push_undo(&mut self, op: UndoOp) {
        self.undo_stack.truncate(self.undo_pos);
        self.undo_stack.push(op);
        self.undo_pos = self.undo_stack.len();
    }

    fn undo(&mut self) {
        if self.undo_pos == 0 {
            self.message = "Already at oldest change".to_owned();
            return;
        }
        self.undo_pos -= 1;
        let op = self.undo_stack[self.undo_pos].clone();
        self.apply_undo_op(op);
        self.modified = true;
    }

    fn apply_undo_op(&mut self, op: UndoOp) {
        match op {
            UndoOp::Insert { row, col, text } => {
                self.remove_text_at(row, col, text.len());
                self.row = row;
                self.col = col;
            }
            UndoOp::Delete { row, col, text } => {
                self.insert_text_at(row, col, &text);
                self.row = row;
                self.col = col;
            }
            UndoOp::ReplaceLines {
                start_row,
                old_lines,
                new_lines,
            } => {
                let end = (start_row + new_lines.len()).min(self.lines.len());
                self.lines.splice(start_row..end, old_lines.iter().cloned());
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                self.row = start_row.min(self.lines.len().saturating_sub(1));
                self.col = 0;
            }
        }
        self.clamp_cursor();
    }

    fn redo(&mut self) {
        if self.undo_pos >= self.undo_stack.len() {
            self.message = "Already at newest change".to_owned();
            return;
        }
        let op = self.undo_stack[self.undo_pos].clone();
        self.undo_pos += 1;
        self.apply_redo_op(op);
        self.modified = true;
    }

    fn apply_redo_op(&mut self, op: UndoOp) {
        match op {
            UndoOp::Insert { row, col, text } => {
                self.insert_text_at(row, col, &text);
                self.row = row;
                self.col = col + text.len().saturating_sub(1);
            }
            UndoOp::Delete { row, col, text } => {
                self.remove_text_at(row, col, text.len());
                self.row = row;
                self.col = col;
            }
            UndoOp::ReplaceLines {
                start_row,
                old_lines,
                new_lines,
            } => {
                let end = (start_row + old_lines.len()).min(self.lines.len());
                self.lines.splice(start_row..end, new_lines.iter().cloned());
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                self.row = start_row.min(self.lines.len().saturating_sub(1));
                self.col = 0;
            }
        }
        self.clamp_cursor();
    }

    // --- Low-level text mutation ---

    fn insert_text_at(&mut self, row: usize, col: usize, text: &str) {
        if row >= self.lines.len() {
            return;
        }
        if text.contains('\n') {
            let line = self.lines[row].clone();
            let byte_col = col.min(line.len());
            let before = &line[..byte_col];
            let after = &line[byte_col..];
            let mut parts: Vec<&str> = text.split('\n').collect();
            let first_part = parts.remove(0);
            let last_part = parts.pop().unwrap_or("");
            let first_line = format!("{}{}", before, first_part);
            let last_line = format!("{}{}", last_part, after);
            let mut new_lines = vec![first_line];
            for p in parts {
                new_lines.push(p.to_owned());
            }
            new_lines.push(last_line);
            self.lines.splice(row..=row, new_lines);
        } else {
            let line = &mut self.lines[row];
            let byte_col = col.min(line.len());
            line.insert_str(byte_col, text);
        }
    }

    fn remove_text_at(&mut self, row: usize, col: usize, count: usize) {
        if row >= self.lines.len() || count == 0 {
            return;
        }
        let line = &mut self.lines[row];
        let start = col.min(line.len());
        let end = (start + count).min(line.len());
        line.drain(start..end);
    }

    // --- Cursor helpers ---

    fn line_len(&self) -> usize {
        self.lines.get(self.row).map(|l| l.len()).unwrap_or(0)
    }

    fn clamp_cursor(&mut self) {
        self.row = self.row.min(self.lines.len().saturating_sub(1));
        let max_col = if self.mode == Mode::Insert {
            self.line_len()
        } else {
            self.line_len().saturating_sub(1)
        };
        self.col = self.col.min(max_col);
    }

    fn line_num_width(&self) -> usize {
        if self.show_numbers {
            format!("{}", self.lines.len()).len() + 1
        } else {
            0
        }
    }

    fn text_cols(&self) -> usize {
        self.term_cols.saturating_sub(self.line_num_width())
    }

    fn scroll_to_cursor(&mut self) {
        let visible = self.term_rows.saturating_sub(2);
        if self.row < self.scroll {
            self.scroll = self.row;
        } else if self.row >= self.scroll + visible {
            self.scroll = self.row + 1 - visible;
        }
    }

    // --- Movement ---

    fn move_left(&mut self, n: usize) {
        self.col = self.col.saturating_sub(n);
    }

    fn move_right(&mut self, n: usize) {
        let max = if self.mode == Mode::Insert {
            self.line_len()
        } else {
            self.line_len().saturating_sub(1)
        };
        self.col = (self.col + n).min(max);
    }

    fn move_up(&mut self, n: usize) {
        self.row = self.row.saturating_sub(n);
        self.clamp_cursor();
    }

    fn move_down(&mut self, n: usize) {
        self.row = (self.row + n).min(self.lines.len().saturating_sub(1));
        self.clamp_cursor();
    }

    fn move_to_bol(&mut self) {
        self.col = 0;
    }

    fn move_to_eol(&mut self) {
        let len = self.line_len();
        self.col = if len == 0 { 0 } else { len - 1 };
    }

    fn move_to_first_nonblank(&mut self) {
        if let Some(line) = self.lines.get(self.row) {
            self.col = line.chars().take_while(|c| c.is_whitespace()).count();
        }
    }

    fn move_word_forward(&mut self, n: usize) {
        for _ in 0..n {
            let line = match self.lines.get(self.row) {
                Some(l) => l.clone(),
                None => return,
            };
            let chars: Vec<char> = line.chars().collect();
            let len = chars.len();
            let mut c = self.col.min(len.saturating_sub(1));
            let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
            if c < len && is_word(chars[c]) {
                while c < len && is_word(chars[c]) {
                    c += 1;
                }
            } else if c < len && !chars[c].is_whitespace() {
                while c < len && !chars[c].is_whitespace() && !is_word(chars[c]) {
                    c += 1;
                }
            }
            while c < len && chars[c].is_whitespace() {
                c += 1;
            }
            if c >= len && self.row + 1 < self.lines.len() {
                self.row += 1;
                self.col = 0;
                self.move_to_first_nonblank();
                return;
            }
            self.col = c.min(len.saturating_sub(1));
        }
    }

    fn move_word_backward(&mut self, n: usize) {
        for _ in 0..n {
            let line = match self.lines.get(self.row) {
                Some(l) => l.clone(),
                None => return,
            };
            let chars: Vec<char> = line.chars().collect();
            let len = chars.len();
            let mut c = self.col.min(len.saturating_sub(1));
            let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
            c = c.saturating_sub(1);
            while c > 0 && chars[c].is_whitespace() {
                c -= 1;
            }
            if c > 0 {
                if is_word(chars[c]) {
                    while c > 0 && is_word(chars[c - 1]) {
                        c -= 1;
                    }
                } else {
                    while c > 0 && !chars[c - 1].is_whitespace() && !is_word(chars[c - 1]) {
                        c -= 1;
                    }
                }
            }
            self.col = c;
        }
    }

    fn move_word_end(&mut self, n: usize) {
        for _ in 0..n {
            let line = match self.lines.get(self.row) {
                Some(l) => l.clone(),
                None => return,
            };
            let chars: Vec<char> = line.chars().collect();
            let len = chars.len();
            let mut c = self.col.min(len.saturating_sub(1));
            let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
            if c + 1 < len {
                c += 1;
            }
            while c + 1 < len && chars[c].is_whitespace() {
                c += 1;
            }
            if is_word(chars[c]) {
                while c + 1 < len && is_word(chars[c + 1]) {
                    c += 1;
                }
            } else {
                while c + 1 < len && !chars[c + 1].is_whitespace() && !is_word(chars[c + 1]) {
                    c += 1;
                }
            }
            self.col = c.min(len.saturating_sub(1));
        }
    }

    fn goto_first_line(&mut self) {
        self.row = 0;
        self.col = 0;
        self.move_to_first_nonblank();
    }

    fn goto_last_line(&mut self) {
        self.row = self.lines.len().saturating_sub(1);
        self.col = 0;
        self.move_to_first_nonblank();
    }

    fn goto_line(&mut self, n: usize) {
        self.row = n.saturating_sub(1).min(self.lines.len().saturating_sub(1));
        self.col = 0;
        self.move_to_first_nonblank();
    }

    fn page_down(&mut self, n: usize) {
        let page = self.term_rows.saturating_sub(2);
        self.move_down(page * n);
    }

    fn page_up(&mut self, n: usize) {
        let page = self.term_rows.saturating_sub(2);
        self.move_up(page * n);
    }

    // --- Normal-mode editing ---

    fn delete_char(&mut self) -> Option<char> {
        let line = self.lines.get_mut(self.row)?;
        if line.is_empty() {
            return None;
        }
        let c = self.col.min(line.len() - 1);
        let ch = line.chars().nth(c)?;
        line.remove(c);
        let new_len = self.lines[self.row].len();
        self.col = self.col.min(new_len.saturating_sub(1));
        self.modified = true;
        Some(ch)
    }

    fn delete_lines(&mut self, n: usize) -> Vec<String> {
        let start = self.row;
        let end = (start + n).min(self.lines.len());
        let deleted: Vec<String> = self.lines.drain(start..end).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = self.row.min(self.lines.len().saturating_sub(1));
        self.col = 0;
        self.move_to_first_nonblank();
        self.modified = true;
        deleted
    }

    fn yank_lines(&mut self, n: usize) {
        let start = self.row;
        let end = (start + n).min(self.lines.len());
        self.register = Some(Register::Lines(self.lines[start..end].to_vec()));
    }

    fn paste_after(&mut self) {
        match self.register.clone() {
            Some(Register::Lines(lines)) => {
                let at = self.row + 1;
                for (i, line) in lines.iter().enumerate() {
                    self.lines.insert(at + i, line.clone());
                }
                self.row = at;
                self.col = 0;
                self.move_to_first_nonblank();
                self.modified = true;
            }
            Some(Register::Chars(text)) => {
                let line = &mut self.lines[self.row];
                let pos = if line.is_empty() {
                    0
                } else {
                    (self.col + 1).min(line.len())
                };
                line.insert_str(pos, &text);
                self.col = pos + text.len().saturating_sub(1);
                self.modified = true;
            }
            None => {
                self.message = "Nothing in register".to_owned();
            }
        }
    }

    fn paste_before(&mut self) {
        match self.register.clone() {
            Some(Register::Lines(lines)) => {
                for (i, line) in lines.iter().enumerate() {
                    self.lines.insert(self.row + i, line.clone());
                }
                self.col = 0;
                self.move_to_first_nonblank();
                self.modified = true;
            }
            Some(Register::Chars(text)) => {
                let line = &mut self.lines[self.row];
                let pos = self.col.min(line.len());
                line.insert_str(pos, &text);
                self.col = pos;
                self.modified = true;
            }
            None => {
                self.message = "Nothing in register".to_owned();
            }
        }
    }

    fn join_lines(&mut self) {
        if self.row + 1 >= self.lines.len() {
            return;
        }
        let old_len = self.lines[self.row].len();
        let next = self.lines.remove(self.row + 1);
        if !self.lines[self.row].is_empty() {
            self.lines[self.row].push(' ');
        }
        self.lines[self.row].push_str(next.trim_start());
        self.col = old_len;
        self.modified = true;
    }

    fn toggle_case(&mut self) {
        let line = match self.lines.get_mut(self.row) {
            Some(l) => l,
            None => return,
        };
        if line.is_empty() {
            return;
        }
        let c = self.col.min(line.len() - 1);
        if let Some(ch) = line[c..].chars().next() {
            let ch_len = ch.len_utf8();
            let toggled: String = if ch.is_uppercase() {
                ch.to_lowercase().collect()
            } else {
                ch.to_uppercase().collect()
            };
            line.replace_range(c..c + ch_len, &toggled);
        }
        self.move_right(1);
        self.modified = true;
    }

    fn replace_char(&mut self, ch: char) {
        let line = match self.lines.get_mut(self.row) {
            Some(l) => l,
            None => return,
        };
        if line.is_empty() {
            line.push(ch);
            return;
        }
        let c = self.col.min(line.len() - 1);
        if let Some(old_ch) = line[c..].chars().next() {
            let old_len = old_ch.len_utf8();
            let s: String = std::iter::once(ch).collect();
            line.replace_range(c..c + old_len, &s);
        }
        self.modified = true;
    }

    fn indent_line(&mut self, right: bool) {
        if right {
            self.lines[self.row].insert_str(0, "    ");
        } else {
            let spaces = self.lines[self.row]
                .chars()
                .take_while(|&c| c == ' ')
                .count();
            let remove = spaces.min(4);
            self.lines[self.row].drain(..remove);
        }
        self.move_to_first_nonblank();
        self.modified = true;
    }

    // --- Open lines ---

    fn open_line_below(&mut self) {
        let indent: String = self.lines[self.row]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        self.lines.insert(self.row + 1, indent.clone());
        self.row += 1;
        self.col = indent.len();
        self.mode = Mode::Insert;
        self.modified = true;
    }

    fn open_line_above(&mut self) {
        let indent: String = self.lines[self.row]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect();
        self.lines.insert(self.row, indent.clone());
        self.col = indent.len();
        self.mode = Mode::Insert;
        self.modified = true;
    }

    // --- Insert mode ---

    fn insert_char(&mut self, ch: char) {
        let line = &mut self.lines[self.row];
        let byte_col = self.col.min(line.len());
        let s: String = std::iter::once(ch).collect();
        line.insert_str(byte_col, &s);
        self.col += ch.len_utf8();
        self.modified = true;
    }

    fn insert_backspace(&mut self) {
        if self.col > 0 {
            let line = &mut self.lines[self.row];
            let byte_col = self.col.min(line.len());
            if let Some((idx, ch)) = line[..byte_col].char_indices().next_back() {
                let ch_len = ch.len_utf8();
                line.drain(idx..idx + ch_len);
                self.col = idx;
            }
        } else if self.row > 0 {
            let cur_line = self.lines.remove(self.row);
            self.row -= 1;
            let prev_len = self.lines[self.row].len();
            self.lines[self.row].push_str(&cur_line);
            self.col = prev_len;
        }
        self.modified = true;
    }

    fn insert_enter(&mut self) {
        let byte_col = self.col.min(self.lines[self.row].len());
        let after = self.lines[self.row][byte_col..].to_owned();
        self.lines[self.row].truncate(byte_col);
        let indent: String = after.chars().take_while(|c| c.is_whitespace()).collect();
        let new_line = format!("{}{}", indent, after.trim_start());
        self.lines.insert(self.row + 1, new_line);
        self.row += 1;
        self.col = indent.len();
        self.modified = true;
    }

    // --- Search ---

    fn search_fwd(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        let nr = self.lines.len();
        if nr == 0 {
            return;
        }
        // Visit nr+1 (line, start_col) pairs in forward order: the current line
        // after the cursor first, then each following line wrapping around, and
        // finally the current line again from its start so a match *before* the
        // cursor on the same line is found last. The cursor's own position is
        // skipped (start_col = col+1 on the first visit); on the final wrap back
        // to the starting line we reject matches at or after the cursor column
        // (those were already considered on the first visit).
        for i in 0..=nr {
            let r = (self.row + i) % nr;
            let start_col = if i == 0 { self.col + 1 } else { 0 };
            if let Some(line) = self.lines.get(r)
                && let Some(pos) = find_after(line, pattern, start_col)
            {
                if i == nr && pos >= self.col {
                    break;
                }
                self.row = r;
                self.col = pos;
                return;
            }
        }
        self.message = format!("Pattern not found: {}", pattern);
    }

    fn search_bwd(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        let nr = self.lines.len();
        if nr == 0 {
            return;
        }
        // Mirror of search_fwd, walking backward. Visit nr+1 (line, before)
        // pairs: the current line before the cursor first, then each preceding
        // line wrapping around, and finally the current line again over its
        // whole length so a match *after* the cursor on the same line is found
        // last (the wrap case). On that final visit we reject matches at or
        // before the cursor column (already considered on the first visit).
        for i in 0..=nr {
            // r = (row - i) mod nr, computed without underflow.
            let r = (self.row + nr - (i % nr)) % nr;
            if let Some(line) = self.lines.get(r) {
                let before = if i == 0 { self.col } else { line.len() };
                if let Some(pos) = find_before(line, pattern, before) {
                    if i == nr && pos <= self.col {
                        break;
                    }
                    self.row = r;
                    self.col = pos;
                    return;
                }
            }
        }
        self.message = format!("Pattern not found: {}", pattern);
    }

    fn search_next(&mut self) {
        let pat = self.search_pat.clone();
        let fwd = self.search_forward;
        if fwd {
            self.search_fwd(&pat);
        } else {
            self.search_bwd(&pat);
        }
    }

    fn search_prev(&mut self) {
        let pat = self.search_pat.clone();
        let fwd = self.search_forward;
        if fwd {
            self.search_bwd(&pat);
        } else {
            self.search_fwd(&pat);
        }
    }

    // --- Substitute ---

    fn substitute_global(&mut self, pat: &str, rep: &str, all_lines: bool) -> usize {
        if pat.is_empty() {
            return 0;
        }
        let mut count = 0usize;
        let (range_start, range_end) = if all_lines {
            (0, self.lines.len())
        } else {
            (self.row, self.row + 1)
        };
        for i in range_start..range_end {
            let mut new_line = String::new();
            let mut search_start = 0;
            let line = self.lines[i].clone();
            while search_start <= line.len() {
                match line[search_start..].find(pat) {
                    Some(pos) => {
                        new_line.push_str(&line[search_start..search_start + pos]);
                        new_line.push_str(rep);
                        search_start += pos + pat.len().max(1);
                        count += 1;
                    }
                    None => {
                        new_line.push_str(&line[search_start..]);
                        break;
                    }
                }
            }
            self.lines[i] = new_line;
        }
        if count > 0 {
            self.modified = true;
        }
        count
    }

    // --- Visual mode ---

    fn visual_range(&self) -> ((usize, usize), (usize, usize)) {
        let (ar, ac) = self.visual_anchor;
        let (cr, cc) = (self.row, self.col);
        if ar < cr || (ar == cr && ac <= cc) {
            ((ar, ac), (cr, cc))
        } else {
            ((cr, cc), (ar, ac))
        }
    }

    fn visual_line_range(&self) -> (usize, usize) {
        let (ar, _) = self.visual_anchor;
        if ar <= self.row {
            (ar, self.row)
        } else {
            (self.row, ar)
        }
    }

    fn yank_visual(&mut self) {
        match self.mode {
            Mode::VisualLine => {
                let (s, e) = self.visual_line_range();
                let lines = self.lines[s..=e.min(self.lines.len() - 1)].to_vec();
                self.register = Some(Register::Lines(lines));
            }
            Mode::VisualChar => {
                let ((sr, sc), (er, ec)) = self.visual_range();
                let text = self.extract_char_range(sr, sc, er, ec);
                self.register = Some(Register::Chars(text));
            }
            _ => {}
        }
        self.mode = Mode::Normal;
        self.message = "Yanked".to_owned();
    }

    fn delete_visual(&mut self) {
        match self.mode {
            Mode::VisualLine => {
                let (start, end) = self.visual_line_range();
                let n = end - start + 1;
                let del = self.delete_lines_range(start, n);
                self.register = Some(Register::Lines(del));
                self.row = start.min(self.lines.len().saturating_sub(1));
            }
            Mode::VisualChar => {
                let ((sr, sc), (er, ec)) = self.visual_range();
                let text = self.extract_char_range(sr, sc, er, ec);
                self.register = Some(Register::Chars(text));
                self.delete_char_range(sr, sc, er, ec);
                self.row = sr;
                self.col = sc;
            }
            _ => {}
        }
        self.mode = Mode::Normal;
        self.modified = true;
    }

    fn indent_visual(&mut self, right: bool) {
        let (start, end) = match self.mode {
            Mode::VisualLine => self.visual_line_range(),
            Mode::VisualChar => {
                let ((sr, _), (er, _)) = self.visual_range();
                (sr, er)
            }
            _ => return,
        };
        for r in start..=end.min(self.lines.len().saturating_sub(1)) {
            if right {
                self.lines[r].insert_str(0, "    ");
            } else {
                let spaces = self.lines[r].chars().take_while(|&c| c == ' ').count();
                self.lines[r].drain(..spaces.min(4));
            }
        }
        self.mode = Mode::Normal;
        self.modified = true;
    }

    fn extract_char_range(&self, sr: usize, sc: usize, er: usize, ec: usize) -> String {
        let mut out = String::new();
        for r in sr..=er.min(self.lines.len().saturating_sub(1)) {
            let line = &self.lines[r];
            let col_start = if r == sr { sc.min(line.len()) } else { 0 };
            let col_end = if r == er {
                (ec + 1).min(line.len())
            } else {
                line.len()
            };
            if r > sr {
                out.push('\n');
            }
            out.push_str(&line[col_start..col_end]);
        }
        out
    }

    fn delete_char_range(&mut self, sr: usize, sc: usize, er: usize, ec: usize) {
        if sr == er {
            let line = &mut self.lines[sr];
            let s = sc.min(line.len());
            let e = (ec + 1).min(line.len());
            if s < e {
                line.drain(s..e);
            }
        } else {
            let prefix = self.lines[sr][..sc.min(self.lines[sr].len())].to_owned();
            let suffix = {
                let l = &self.lines[er];
                l[(ec + 1).min(l.len())..].to_owned()
            };
            self.lines[sr] = format!("{}{}", prefix, suffix);
            let rm_end = er.min(self.lines.len().saturating_sub(1));
            self.lines.drain(sr + 1..=rm_end);
        }
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
    }

    fn delete_lines_range(&mut self, start: usize, n: usize) -> Vec<String> {
        let end = (start + n).min(self.lines.len());
        let deleted: Vec<String> = self.lines.drain(start..end).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        deleted
    }

    // --- Command execution ---

    fn exec_command(&mut self, cmd: &str) -> bool {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return false;
        }
        if cmd.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = cmd.parse::<usize>() {
                self.goto_line(n);
            }
            return false;
        }
        if cmd == "q" {
            if self.modified {
                self.message = "No write since last change (use :q! to override)".to_owned();
                return false;
            }
            return true;
        }
        if cmd == "q!" {
            return true;
        }
        if cmd == "w" {
            match self.filename.clone() {
                Some(f) => match self.save_file(&f) {
                    Ok(()) => self.message = format!("\"{}\" {}L written", f, self.lines.len()),
                    Err(e) => self.message = e,
                },
                None => self.message = "No file name".to_owned(),
            }
            return false;
        }
        if let Some(rest) = cmd.strip_prefix("w ") {
            match self.save_file(rest.trim()) {
                Ok(()) => self.message = format!("\"{}\" written", rest.trim()),
                Err(e) => self.message = e,
            }
            return false;
        }
        if cmd == "wq" || cmd == "x" {
            match self.filename.clone() {
                Some(f) => match self.save_file(&f) {
                    Ok(()) => return true,
                    Err(e) => {
                        self.message = e;
                        return false;
                    }
                },
                None => {
                    self.message = "No file name".to_owned();
                    return false;
                }
            }
        }
        if let Some(rest) = cmd.strip_prefix("e ") {
            let path = rest.trim().to_owned();
            if self.modified {
                self.message = "No write since last change (use :e! to override)".to_owned();
                return false;
            }
            match self.load_file(&path) {
                Ok(()) => self.message = format!("\"{}\" opened", path),
                Err(e) => self.message = e,
            }
            return false;
        }
        if let Some(rest) = cmd.strip_prefix("e! ") {
            let path = rest.trim().to_owned();
            match self.load_file(&path) {
                Ok(()) => self.message = format!("\"{}\" opened", path),
                Err(e) => self.message = e,
            }
            return false;
        }
        if cmd == "set number" || cmd == "set nu" {
            self.show_numbers = true;
            return false;
        }
        if cmd == "set nonumber" || cmd == "set nonu" {
            self.show_numbers = false;
            return false;
        }
        let is_percent_sub = cmd.starts_with("%s");
        let is_local_sub = !is_percent_sub && cmd.starts_with('s');
        if is_percent_sub || is_local_sub {
            let body = if is_percent_sub { &cmd[2..] } else { &cmd[1..] };
            let all_lines = is_percent_sub;
            if let Some(sep) = body.chars().next() {
                let sep_str: String = std::iter::once(sep).collect();
                let parts: Vec<&str> = body[sep.len_utf8()..].split(sep_str.as_str()).collect();
                if parts.len() >= 2 {
                    let (pat, rep) = (parts[0], parts[1]);
                    let count = self.substitute_global(pat, rep, all_lines);
                    self.message = if count == 0 {
                        format!("Pattern not found: {}", pat)
                    } else {
                        format!("{} substitution(s) made", count)
                    };
                } else {
                    self.message = "Usage: :%s/pat/rep/g".to_owned();
                }
            }
            return false;
        }
        self.message = format!("Unknown command: {}", cmd);
        false
    }

    // --- Count prefix ---

    fn take_count(&mut self) -> usize {
        let n: usize = self.count_buf.parse().unwrap_or(1).max(1);
        self.count_buf.clear();
        n
    }

    // --- Repeat last change ---

    fn repeat_last_change(&mut self) {
        match self.last_change.clone() {
            LastChange::None => {}
            LastChange::DeleteChar => {
                if let Some(ch) = self.delete_char() {
                    self.push_undo(UndoOp::Delete {
                        row: self.row,
                        col: self.col,
                        text: ch.to_string(),
                    });
                }
            }
            LastChange::DeleteLine => {
                let old = self.lines[self.row].clone();
                self.delete_lines(1);
                self.push_undo(UndoOp::ReplaceLines {
                    start_row: self.row,
                    old_lines: vec![old],
                    new_lines: vec![],
                });
            }
            LastChange::YankLine => {
                self.yank_lines(1);
            }
            LastChange::ReplaceChar(ch) => {
                self.replace_char(ch);
            }
            LastChange::JoinLines => {
                self.join_lines();
            }
            LastChange::ToggleCase => {
                self.toggle_case();
            }
            LastChange::IndentRight => {
                self.indent_line(true);
            }
            LastChange::IndentLeft => {
                self.indent_line(false);
            }
            LastChange::PasteAfter => {
                self.paste_after();
            }
            LastChange::PasteBefore => {
                self.paste_before();
            }
            LastChange::InsertText { motion, text } => {
                match motion {
                    InsertMotion::InsertBOL => self.col = 0,
                    InsertMotion::InsertEOL => self.col = self.line_len(),
                    InsertMotion::InsertAfter => self.move_right(1),
                    InsertMotion::OpenBelow => {
                        self.open_line_below();
                        self.mode = Mode::Normal;
                    }
                    InsertMotion::OpenAbove => {
                        self.open_line_above();
                        self.mode = Mode::Normal;
                    }
                    InsertMotion::InsertBefore => {}
                }
                for ch in text.chars() {
                    if ch == '\n' {
                        self.insert_enter();
                    } else {
                        self.insert_char(ch);
                    }
                }
            }
        }
    }

    // --- Normal-mode key handler ---

    fn handle_normal(&mut self, key: Key) -> bool {
        if let Key::Char(ch) = &key
            && ch.is_ascii_digit()
            && (*ch != '0' || !self.count_buf.is_empty())
        {
            self.count_buf.push(*ch);
            return false;
        }
        match key {
            Key::Char('i') => {
                self.mode = Mode::Insert;
                self.last_change = LastChange::InsertText {
                    motion: InsertMotion::InsertBefore,
                    text: String::new(),
                };
            }
            Key::Char('I') => {
                self.move_to_first_nonblank();
                self.mode = Mode::Insert;
                self.last_change = LastChange::InsertText {
                    motion: InsertMotion::InsertBOL,
                    text: String::new(),
                };
            }
            Key::Char('a') => {
                self.move_right(1);
                self.mode = Mode::Insert;
                self.last_change = LastChange::InsertText {
                    motion: InsertMotion::InsertAfter,
                    text: String::new(),
                };
            }
            Key::Char('A') => {
                self.col = self.line_len();
                self.mode = Mode::Insert;
                self.last_change = LastChange::InsertText {
                    motion: InsertMotion::InsertEOL,
                    text: String::new(),
                };
            }
            Key::Char('o') => {
                self.count_buf.clear();
                self.open_line_below();
                self.last_change = LastChange::InsertText {
                    motion: InsertMotion::OpenBelow,
                    text: String::new(),
                };
            }
            Key::Char('O') => {
                self.count_buf.clear();
                self.open_line_above();
                self.last_change = LastChange::InsertText {
                    motion: InsertMotion::OpenAbove,
                    text: String::new(),
                };
            }
            Key::Char('v') => {
                self.visual_anchor = (self.row, self.col);
                self.mode = Mode::VisualChar;
                self.count_buf.clear();
            }
            Key::Char('V') => {
                self.visual_anchor = (self.row, self.col);
                self.mode = Mode::VisualLine;
                self.count_buf.clear();
            }
            Key::Char(':') => {
                self.cmd_buf.clear();
                self.mode = Mode::Command;
                self.count_buf.clear();
            }
            Key::Char('h') | Key::Left => {
                let n = self.take_count();
                self.move_left(n);
            }
            Key::Char('j') | Key::Down => {
                let n = self.take_count();
                self.move_down(n);
            }
            Key::Char('k') | Key::Up => {
                let n = self.take_count();
                self.move_up(n);
            }
            Key::Char('l') | Key::Right => {
                let n = self.take_count();
                self.move_right(n);
            }
            Key::Char('w') => {
                let n = self.take_count();
                self.move_word_forward(n);
            }
            Key::Char('b') => {
                let n = self.take_count();
                self.move_word_backward(n);
            }
            Key::Char('e') => {
                let n = self.take_count();
                self.move_word_end(n);
            }
            Key::Char('0') => {
                self.count_buf.clear();
                self.move_to_bol();
            }
            Key::Char('$') => {
                self.count_buf.clear();
                self.move_to_eol();
            }
            Key::Char('^') => {
                self.count_buf.clear();
                self.move_to_first_nonblank();
            }
            Key::Char('g') => {
                let n = self.take_count();
                let next = read_key();
                if let Key::Char('g') = next {
                    if n > 1 {
                        self.goto_line(n);
                    } else {
                        self.goto_first_line();
                    }
                }
            }
            Key::Char('G') => {
                let n_raw = self.count_buf.parse::<usize>().unwrap_or(0);
                self.count_buf.clear();
                if n_raw > 0 {
                    self.goto_line(n_raw);
                } else {
                    self.goto_last_line();
                }
            }
            Key::Ctrl('f') | Key::PageDown => {
                let n = self.take_count();
                self.page_down(n);
            }
            Key::Ctrl('b') | Key::PageUp => {
                let n = self.take_count();
                self.page_up(n);
            }
            Key::Ctrl('d') => {
                let n = self.take_count();
                let h = self.term_rows / 2;
                self.move_down(h * n);
            }
            Key::Ctrl('u') => {
                let n = self.take_count();
                let h = self.term_rows / 2;
                self.move_up(h * n);
            }
            Key::Char('x') => {
                let (row, col) = (self.row, self.col);
                let n = self.take_count();
                for _ in 0..n {
                    self.delete_char();
                }
                self.push_undo(UndoOp::Delete {
                    row,
                    col,
                    text: "x".to_owned(),
                });
                self.last_change = LastChange::DeleteChar;
            }
            Key::Char('d') => {
                let n = self.take_count();
                let next = read_key();
                match next {
                    Key::Char('d') => {
                        let start = self.row;
                        let end = (start + n).min(self.lines.len());
                        let old = self.lines[start..end].to_vec();
                        self.delete_lines(n);
                        self.push_undo(UndoOp::ReplaceLines {
                            start_row: start,
                            old_lines: old,
                            new_lines: vec![],
                        });
                        self.last_change = LastChange::DeleteLine;
                    }
                    Key::Char('w') => {
                        let (col, row) = (self.col, self.row);
                        let old = self.lines[row].clone();
                        self.move_word_forward(n);
                        let end_col = self.col;
                        self.col = col;
                        if end_col > col {
                            let deleted = self.lines[row][col..end_col].to_owned();
                            self.lines[row].drain(col..end_col);
                            self.register = Some(Register::Chars(deleted));
                            self.modified = true;
                        }
                        self.push_undo(UndoOp::ReplaceLines {
                            start_row: row,
                            old_lines: vec![old],
                            new_lines: vec![self.lines[row].clone()],
                        });
                    }
                    _ => {}
                }
            }
            Key::Char('y') => {
                let n = self.take_count();
                let next = read_key();
                if next == Key::Char('y') {
                    self.yank_lines(n);
                    self.message = format!("{} line(s) yanked", n);
                    self.last_change = LastChange::YankLine;
                }
            }
            Key::Char('p') => {
                self.count_buf.clear();
                self.paste_after();
                self.last_change = LastChange::PasteAfter;
            }
            Key::Char('P') => {
                self.count_buf.clear();
                self.paste_before();
                self.last_change = LastChange::PasteBefore;
            }
            Key::Char('u') => {
                self.count_buf.clear();
                self.undo();
            }
            Key::Ctrl('r') => {
                self.count_buf.clear();
                self.redo();
            }
            Key::Char('r') => {
                self.count_buf.clear();
                if let Key::Char(ch) = read_key() {
                    self.replace_char(ch);
                    self.last_change = LastChange::ReplaceChar(ch);
                }
            }
            Key::Char('J') => {
                let n = self.take_count().max(1);
                let (row, end) = (self.row, (self.row + n + 1).min(self.lines.len()));
                let old = self.lines[row..end].to_vec();
                for _ in 0..n {
                    self.join_lines();
                }
                self.push_undo(UndoOp::ReplaceLines {
                    start_row: row,
                    old_lines: old,
                    new_lines: vec![self.lines[row].clone()],
                });
                self.last_change = LastChange::JoinLines;
            }
            Key::Char('~') => {
                let n = self.take_count();
                let row = self.row;
                let old = self.lines[row].clone();
                for _ in 0..n {
                    self.toggle_case();
                }
                self.push_undo(UndoOp::ReplaceLines {
                    start_row: row,
                    old_lines: vec![old],
                    new_lines: vec![self.lines[row].clone()],
                });
                self.last_change = LastChange::ToggleCase;
            }
            Key::Char('>') => {
                if read_key() == Key::Char('>') {
                    let n = self.take_count();
                    let row = self.row;
                    let old = self.lines[row].clone();
                    for _ in 0..n {
                        self.indent_line(true);
                    }
                    self.push_undo(UndoOp::ReplaceLines {
                        start_row: row,
                        old_lines: vec![old],
                        new_lines: vec![self.lines[row].clone()],
                    });
                    self.last_change = LastChange::IndentRight;
                }
            }
            Key::Char('<') => {
                if read_key() == Key::Char('<') {
                    let n = self.take_count();
                    let row = self.row;
                    let old = self.lines[row].clone();
                    for _ in 0..n {
                        self.indent_line(false);
                    }
                    self.push_undo(UndoOp::ReplaceLines {
                        start_row: row,
                        old_lines: vec![old],
                        new_lines: vec![self.lines[row].clone()],
                    });
                    self.last_change = LastChange::IndentLeft;
                }
            }
            Key::Char('/') => {
                self.count_buf.clear();
                self.mode = Mode::Command;
                self.cmd_buf.clear();
                self.cmd_buf.push('/');
            }
            Key::Char('?') => {
                self.count_buf.clear();
                self.mode = Mode::Command;
                self.cmd_buf.clear();
                self.cmd_buf.push('?');
            }
            Key::Char('n') => {
                self.count_buf.clear();
                self.search_next();
            }
            Key::Char('N') => {
                self.count_buf.clear();
                self.search_prev();
            }
            Key::Char('.') => {
                self.count_buf.clear();
                self.repeat_last_change();
            }
            Key::Escape => {
                self.count_buf.clear();
                self.message.clear();
            }
            _ => {
                self.count_buf.clear();
            }
        }
        self.clamp_cursor();
        self.scroll_to_cursor();
        false
    }

    // --- Insert-mode key handler ---

    fn handle_insert(&mut self, key: Key) {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
                if self.col > 0 {
                    self.col -= 1;
                }
            }
            Key::Enter => {
                self.insert_enter();
                if let LastChange::InsertText { text, .. } = &mut self.last_change {
                    text.push('\n');
                }
            }
            Key::Backspace => {
                self.insert_backspace();
                if let LastChange::InsertText { text, .. } = &mut self.last_change {
                    text.pop();
                }
            }
            Key::Char(ch) => {
                self.insert_char(ch);
                if let LastChange::InsertText { text, .. } = &mut self.last_change {
                    text.push(ch);
                }
            }
            Key::Left => {
                self.move_left(1);
            }
            Key::Right => {
                self.move_right(1);
            }
            Key::Up => {
                self.move_up(1);
            }
            Key::Down => {
                self.move_down(1);
            }
            Key::Home => {
                self.move_to_bol();
            }
            Key::End => {
                self.col = self.line_len();
            }
            Key::Delete => {
                let len = self.lines[self.row].len();
                if self.col < len {
                    self.lines[self.row].remove(self.col);
                    self.modified = true;
                } else if self.row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.row + 1);
                    self.lines[self.row].push_str(&next);
                    self.modified = true;
                }
            }
            _ => {}
        }
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    // --- Visual-mode key handler ---

    fn handle_visual(&mut self, key: Key) {
        match key {
            Key::Escape => {
                self.mode = Mode::Normal;
            }
            Key::Char('y') => {
                self.yank_visual();
            }
            Key::Char('d') | Key::Char('x') => {
                self.delete_visual();
            }
            Key::Char('>') => {
                self.indent_visual(true);
            }
            Key::Char('<') => {
                self.indent_visual(false);
            }
            Key::Char('h') | Key::Left => {
                self.move_left(1);
            }
            Key::Char('j') | Key::Down => {
                self.move_down(1);
            }
            Key::Char('k') | Key::Up => {
                self.move_up(1);
            }
            Key::Char('l') | Key::Right => {
                self.move_right(1);
            }
            Key::Char('w') => {
                self.move_word_forward(1);
            }
            Key::Char('b') => {
                self.move_word_backward(1);
            }
            Key::Char('e') => {
                self.move_word_end(1);
            }
            Key::Char('0') => {
                self.move_to_bol();
            }
            Key::Char('$') => {
                self.move_to_eol();
            }
            Key::Char('^') => {
                self.move_to_first_nonblank();
            }
            Key::Char('G') => {
                self.goto_last_line();
            }
            _ => {}
        }
        self.clamp_cursor();
        self.scroll_to_cursor();
    }

    // --- Command-mode key handler ---

    fn handle_command(&mut self, key: Key) -> bool {
        match key {
            Key::Escape => {
                self.cmd_buf.clear();
                self.mode = Mode::Normal;
            }
            Key::Enter => {
                let cmd = self.cmd_buf.clone();
                self.cmd_buf.clear();
                self.mode = Mode::Normal;
                if let Some(pat) = cmd.strip_prefix('/') {
                    if !pat.is_empty() {
                        self.search_pat = pat.to_owned();
                    }
                    self.search_forward = true;
                    let p = self.search_pat.clone();
                    self.search_fwd(&p);
                } else if let Some(pat) = cmd.strip_prefix('?') {
                    if !pat.is_empty() {
                        self.search_pat = pat.to_owned();
                    }
                    self.search_forward = false;
                    let p = self.search_pat.clone();
                    self.search_bwd(&p);
                } else {
                    return self.exec_command(&cmd);
                }
            }
            Key::Backspace => {
                if self.cmd_buf.is_empty() {
                    self.mode = Mode::Normal;
                } else {
                    self.cmd_buf.pop();
                }
            }
            Key::Char(ch) => {
                self.cmd_buf.push(ch);
            }
            _ => {}
        }
        false
    }

    // --- Rendering ---

    fn render(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(b"\x1b[?25l\x1b[H"); // hide cursor, top-left
        let visible_rows = self.term_rows.saturating_sub(1);
        let num_w = self.line_num_width();
        let text_w = self.text_cols();
        for screen_row in 0..visible_rows {
            let buf_row = self.scroll + screen_row;
            out.extend_from_slice(b"\x1b[K");
            if buf_row < self.lines.len() {
                if self.show_numbers {
                    let s = format!("{:>width$} ", buf_row + 1, width = num_w - 1);
                    out.extend_from_slice(b"\x1b[2m");
                    out.extend_from_slice(s.as_bytes());
                    out.extend_from_slice(b"\x1b[22m");
                }
                let line = &self.lines[buf_row];
                let (hl_start, hl_end) = self.visual_highlight_range(buf_row);
                let mut col_idx = 0usize;
                let mut in_hl = false;
                for (byte_idx, ch) in line.char_indices() {
                    if col_idx >= text_w {
                        break;
                    }
                    let should_hl = byte_idx >= hl_start && byte_idx < hl_end;
                    if should_hl && !in_hl {
                        out.extend_from_slice(b"\x1b[7m");
                        in_hl = true;
                    } else if !should_hl && in_hl {
                        out.extend_from_slice(b"\x1b[27m");
                        in_hl = false;
                    }
                    if ch == '\t' {
                        out.extend_from_slice(b"    ");
                        col_idx += 4;
                    } else {
                        let mut buf = [0u8; 4];
                        let encoded = ch.encode_utf8(&mut buf);
                        out.extend_from_slice(encoded.as_bytes());
                        col_idx += 1;
                    }
                }
                if in_hl {
                    out.extend_from_slice(b"\x1b[0m");
                }
            } else {
                out.extend_from_slice(b"\x1b[34m~\x1b[0m");
            }
            out.extend_from_slice(b"\r\n");
        }
        self.render_status(out);
        let col_display = self.line_num_width()
            + self
                .lines
                .get(self.row)
                .map(|l| {
                    l[..self.col.min(l.len())]
                        .chars()
                        .map(|c| if c == '\t' { 4usize } else { 1 })
                        .sum::<usize>()
                })
                .unwrap_or(0);
        let screen_r = self.row.saturating_sub(self.scroll);
        out.extend_from_slice(format!("\x1b[{};{}H", screen_r + 1, col_display + 1).as_bytes());
        out.extend_from_slice(b"\x1b[?25h");
    }

    fn visual_highlight_range(&self, buf_row: usize) -> (usize, usize) {
        match self.mode {
            Mode::VisualLine => {
                let (sr, er) = self.visual_line_range();
                if buf_row >= sr && buf_row <= er {
                    (
                        0,
                        self.lines.get(buf_row).map(|l| l.len()).unwrap_or(0).max(1),
                    )
                } else {
                    (0, 0)
                }
            }
            Mode::VisualChar => {
                let ((sr, sc), (er, ec)) = self.visual_range();
                if buf_row < sr || buf_row > er {
                    return (0, 0);
                }
                let line = match self.lines.get(buf_row) {
                    Some(l) => l,
                    None => return (0, 0),
                };
                let ll = line.len();
                let start = if buf_row == sr { sc.min(ll) } else { 0 };
                let end = if buf_row == er { (ec + 1).min(ll) } else { ll };
                (start, end.max(start))
            }
            _ => (0, 0),
        }
    }

    fn render_status(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[7m");
        let fname = self.filename.as_deref().unwrap_or("[No Name]");
        let modified = if self.modified { " [+]" } else { "" };
        let pos_str = format!("{}:{}", self.row + 1, self.col + 1);
        let left = if self.mode == Mode::Command {
            format!(":{}", self.cmd_buf)
        } else if !self.message.is_empty() {
            self.message.clone()
        } else {
            format!(" -- {} --  {}{}", self.mode.name(), fname, modified)
        };
        let right = format!("  {}  ", pos_str);
        let max_left = self.term_cols.saturating_sub(right.len());
        let left_trunc = if left.len() > max_left {
            &left[..max_left]
        } else {
            &left
        };
        let pad = self
            .term_cols
            .saturating_sub(left_trunc.len() + right.len());
        let mut line = format!("{}{:pad$}{}", left_trunc, " ", right, pad = pad);
        line.truncate(self.term_cols);
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\x1b[0m");
    }
}

// ============================================================================
// String search helpers
// ============================================================================

fn find_after(s: &str, pat: &str, after: usize) -> Option<usize> {
    if after > s.len() {
        return None;
    }
    s[after..].find(pat).map(|pos| after + pos)
}

fn find_before(s: &str, pat: &str, before: usize) -> Option<usize> {
    s[..before.min(s.len())].rfind(pat)
}

// ============================================================================
// Main loop
// ============================================================================

fn run(mut editor: Editor) {
    enter_alternate_screen();
    let _ = io::stdout().write_all(b"\x1b[2J");
    let _ = io::stdout().flush();
    let _raw = enable_raw_mode();
    let mut render_buf: Vec<u8> = Vec::with_capacity(65536);
    loop {
        let (rows, cols) = get_terminal_size();
        editor.term_rows = rows;
        editor.term_cols = cols;
        editor.scroll_to_cursor();
        editor.render(&mut render_buf);
        write_bytes(&render_buf);
        flush();
        let key = read_key();
        let quit = match editor.mode {
            Mode::Normal => editor.handle_normal(key),
            Mode::Insert => {
                editor.handle_insert(key);
                false
            }
            Mode::VisualChar | Mode::VisualLine => {
                editor.handle_visual(key);
                false
            }
            Mode::Command => editor.handle_command(key),
        };
        if quit {
            break;
        }
        if !matches!(editor.mode, Mode::Command) {
            editor.message.clear();
        }
    }
    drop(_raw);
    leave_alternate_screen();
    write_str("Exiting vi\r\n");
    flush();
}

fn print_help() {
    eprintln!("Usage: vi [FILE]");
    eprintln!("Modal text editor for SlateOS.");
    eprintln!();
    eprintln!("NORMAL:  h/j/k/l  w/b/e  0/$/^  gg/G  C-f/b  i/I/a/A/o/O");
    eprintln!("         x  dd/dw  yy  p/P  r  u/C-r  J  ~  >>/<<  /  n/N  .");
    eprintln!("COMMAND: :w  :q/:q!  :wq  :e FILE  :set number  :%s/pat/rep/g  :N");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    let mut editor = Editor::new();
    if args.len() >= 2
        && let Err(e) = editor.load_file(&args[1])
    {
        eprintln!("vi: {}", e);
        std::process::exit(1);
    }
    run(editor);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_editor(lines: &[&str]) -> Editor {
        let mut e = Editor::new();
        e.lines = lines.iter().map(|s| s.to_string()).collect();
        if e.lines.is_empty() {
            e.lines.push(String::new());
        }
        e
    }

    // --- Buffer / file I/O --------------------------------------------------

    #[test]
    fn test_new_editor_one_empty_line() {
        let e = Editor::new();
        assert_eq!(e.lines, vec![""]);
    }

    #[test]
    fn test_load_file_splits_lines() {
        let mut e = Editor::new();
        let tmp = std::env::temp_dir().join("vi_test_load.txt");
        std::fs::write(&tmp, "hello\nworld\n").unwrap();
        e.load_file(tmp.to_str().unwrap()).unwrap();
        assert_eq!(e.lines, vec!["hello", "world"]);
        assert!(!e.modified);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_load_missing_file_ok() {
        let mut e = Editor::new();
        let result = e.load_file("/tmp/vi_not_exist_xyz123.txt");
        assert!(result.is_ok());
        assert_eq!(e.filename, Some("/tmp/vi_not_exist_xyz123.txt".to_owned()));
    }

    #[test]
    fn test_save_and_reload() {
        let tmp = std::env::temp_dir().join("vi_test_save.txt");
        let mut e = make_editor(&["foo", "bar", "baz"]);
        e.save_file(tmp.to_str().unwrap()).unwrap();
        assert!(!e.modified);
        let mut e2 = Editor::new();
        e2.load_file(tmp.to_str().unwrap()).unwrap();
        assert_eq!(e2.lines, vec!["foo", "bar", "baz"]);
        std::fs::remove_file(&tmp).ok();
    }

    // --- Movement -----------------------------------------------------------

    #[test]
    fn test_move_left_clamps() {
        let mut e = make_editor(&["hello"]);
        e.col = 0;
        e.move_left(10);
        assert_eq!(e.col, 0);
    }

    #[test]
    fn test_move_right_clamps_normal() {
        let mut e = make_editor(&["hi"]);
        e.col = 0;
        e.move_right(100);
        assert_eq!(e.col, 1);
    }

    #[test]
    fn test_move_up_clamps() {
        let mut e = make_editor(&["a", "b"]);
        e.row = 0;
        e.move_up(10);
        assert_eq!(e.row, 0);
    }

    #[test]
    fn test_move_down_clamps() {
        let mut e = make_editor(&["a", "b", "c"]);
        e.move_down(100);
        assert_eq!(e.row, 2);
    }

    #[test]
    fn test_move_to_bol() {
        let mut e = make_editor(&["hello"]);
        e.col = 4;
        e.move_to_bol();
        assert_eq!(e.col, 0);
    }

    #[test]
    fn test_move_to_eol_normal() {
        let mut e = make_editor(&["hello"]);
        e.move_to_eol();
        assert_eq!(e.col, 4);
    }

    #[test]
    fn test_first_nonblank() {
        let mut e = make_editor(&["   hello"]);
        e.move_to_first_nonblank();
        assert_eq!(e.col, 3);
    }

    #[test]
    fn test_goto_first_line() {
        let mut e = make_editor(&["a", "b", "c"]);
        e.row = 2;
        e.goto_first_line();
        assert_eq!(e.row, 0);
    }

    #[test]
    fn test_goto_last_line() {
        let mut e = make_editor(&["a", "b", "c"]);
        e.goto_last_line();
        assert_eq!(e.row, 2);
    }

    #[test]
    fn test_goto_line_number() {
        let mut e = make_editor(&["a", "b", "c", "d"]);
        e.goto_line(3);
        assert_eq!(e.row, 2);
    }

    #[test]
    fn test_word_forward() {
        let mut e = make_editor(&["hello world foo"]);
        e.col = 0;
        e.move_word_forward(1);
        assert_eq!(e.col, 6);
    }

    #[test]
    fn test_word_backward() {
        let mut e = make_editor(&["hello world"]);
        e.col = 10;
        e.move_word_backward(1);
        assert_eq!(e.col, 6);
    }

    // --- Editing ------------------------------------------------------------

    #[test]
    fn test_delete_char_basic() {
        let mut e = make_editor(&["hello"]);
        e.col = 0;
        assert_eq!(e.delete_char(), Some('h'));
        assert_eq!(e.lines[0], "ello");
    }

    #[test]
    fn test_delete_char_empty_returns_none() {
        let mut e = make_editor(&[""]);
        assert_eq!(e.delete_char(), None);
    }

    #[test]
    fn test_delete_lines() {
        let mut e = make_editor(&["a", "b", "c"]);
        e.row = 1;
        let del = e.delete_lines(1);
        assert_eq!(del, vec!["b"]);
        assert_eq!(e.lines, vec!["a", "c"]);
    }

    #[test]
    fn test_delete_all_lines_leaves_empty() {
        let mut e = make_editor(&["a", "b"]);
        e.row = 0;
        e.delete_lines(2);
        assert_eq!(e.lines, vec![""]);
    }

    #[test]
    fn test_yank_paste_after() {
        let mut e = make_editor(&["a", "b", "c"]);
        e.row = 0;
        e.yank_lines(1);
        e.row = 1;
        e.paste_after();
        assert_eq!(e.lines, vec!["a", "b", "a", "c"]);
    }

    #[test]
    fn test_paste_before() {
        let mut e = make_editor(&["x", "y"]);
        e.row = 0;
        e.yank_lines(1);
        e.row = 1;
        e.paste_before();
        assert_eq!(e.lines, vec!["x", "x", "y"]);
    }

    #[test]
    fn test_join_lines() {
        let mut e = make_editor(&["hello", "world"]);
        e.row = 0;
        e.join_lines();
        assert_eq!(e.lines[0], "hello world");
        assert_eq!(e.lines.len(), 1);
    }

    #[test]
    fn test_toggle_case_lower_to_upper() {
        let mut e = make_editor(&["hello"]);
        e.col = 0;
        e.toggle_case();
        assert!(e.lines[0].starts_with('H'));
    }

    #[test]
    fn test_toggle_case_upper_to_lower() {
        let mut e = make_editor(&["Hello"]);
        e.col = 0;
        e.toggle_case();
        assert!(e.lines[0].starts_with('h'));
    }

    #[test]
    fn test_replace_char() {
        let mut e = make_editor(&["abc"]);
        e.col = 1;
        e.replace_char('X');
        assert_eq!(e.lines[0], "aXc");
    }

    #[test]
    fn test_indent_right() {
        let mut e = make_editor(&["hello"]);
        e.indent_line(true);
        assert_eq!(e.lines[0], "    hello");
    }

    #[test]
    fn test_indent_left() {
        let mut e = make_editor(&["    hello"]);
        e.indent_line(false);
        assert_eq!(e.lines[0], "hello");
    }

    // --- Insert mode --------------------------------------------------------

    #[test]
    fn test_insert_char() {
        let mut e = make_editor(&["helo"]);
        e.mode = Mode::Insert;
        e.col = 3;
        e.insert_char('l');
        assert_eq!(e.lines[0], "hello");
    }

    #[test]
    fn test_insert_enter_splits() {
        let mut e = make_editor(&["hello"]);
        e.mode = Mode::Insert;
        e.col = 3;
        e.insert_enter();
        assert_eq!(e.lines[0], "hel");
        assert_eq!(e.lines[1], "lo");
    }

    #[test]
    fn test_insert_backspace_merges() {
        let mut e = make_editor(&["hel", "lo"]);
        e.mode = Mode::Insert;
        e.row = 1;
        e.col = 0;
        e.insert_backspace();
        assert_eq!(e.lines.len(), 1);
        assert_eq!(e.lines[0], "hello");
    }

    // --- Undo / redo --------------------------------------------------------

    #[test]
    fn test_undo_replace_lines() {
        let mut e = make_editor(&["hello"]);
        e.push_undo(UndoOp::ReplaceLines {
            start_row: 0,
            old_lines: vec!["original".to_owned()],
            new_lines: vec!["hello".to_owned()],
        });
        e.undo();
        assert_eq!(e.lines[0], "original");
    }

    #[test]
    fn test_redo_restores() {
        let mut e = make_editor(&["hello"]);
        e.push_undo(UndoOp::ReplaceLines {
            start_row: 0,
            old_lines: vec!["old".to_owned()],
            new_lines: vec!["hello".to_owned()],
        });
        e.undo();
        assert_eq!(e.lines[0], "old");
        e.redo();
        assert_eq!(e.lines[0], "hello");
    }

    #[test]
    fn test_new_change_clears_redo() {
        let mut e = make_editor(&["a"]);
        e.push_undo(UndoOp::ReplaceLines {
            start_row: 0,
            old_lines: vec!["x".to_owned()],
            new_lines: vec!["a".to_owned()],
        });
        e.undo();
        e.push_undo(UndoOp::ReplaceLines {
            start_row: 0,
            old_lines: vec!["x".to_owned()],
            new_lines: vec!["y".to_owned()],
        });
        assert_eq!(e.undo_stack.len(), 1);
        assert_eq!(e.undo_pos, 1);
    }

    // --- Search -------------------------------------------------------------

    #[test]
    fn test_search_fwd_finds() {
        let mut e = make_editor(&["hello world"]);
        e.row = 0;
        e.col = 0;
        e.search_fwd("world");
        assert_eq!(e.col, 6);
    }

    #[test]
    fn test_search_fwd_wraps() {
        let mut e = make_editor(&["abc", "def", "abc"]);
        e.row = 2;
        e.col = 0;
        e.search_fwd("abc");
        assert_eq!(e.row, 0);
    }

    #[test]
    fn test_search_bwd() {
        let mut e = make_editor(&["hello world hello"]);
        e.col = 12;
        e.search_bwd("hello");
        assert_eq!(e.col, 0);
    }

    #[test]
    fn test_search_not_found_message() {
        let mut e = make_editor(&["hello"]);
        e.search_fwd("xyz_absent");
        assert!(e.message.contains("not found"));
    }

    // --- Substitute ---------------------------------------------------------

    #[test]
    fn test_substitute_all_lines() {
        let mut e = make_editor(&["foo foo", "foo bar"]);
        let n = e.substitute_global("foo", "baz", true);
        assert_eq!(n, 3);
        assert_eq!(e.lines[0], "baz baz");
        assert_eq!(e.lines[1], "baz bar");
    }

    #[test]
    fn test_substitute_single_line() {
        let mut e = make_editor(&["hello world", "hello"]);
        e.row = 0;
        let n = e.substitute_global("hello", "hi", false);
        assert_eq!(n, 1);
        assert_eq!(e.lines[0], "hi world");
        assert_eq!(e.lines[1], "hello");
    }

    // --- Command execution --------------------------------------------------

    #[test]
    fn test_exec_q_unmodified() {
        let mut e = make_editor(&["a"]);
        assert!(e.exec_command("q"));
    }

    #[test]
    fn test_exec_q_modified_stays() {
        let mut e = make_editor(&["a"]);
        e.modified = true;
        assert!(!e.exec_command("q"));
        assert!(e.message.contains("No write"));
    }

    #[test]
    fn test_exec_q_force() {
        let mut e = make_editor(&["a"]);
        e.modified = true;
        assert!(e.exec_command("q!"));
    }

    #[test]
    fn test_exec_set_number() {
        let mut e = Editor::new();
        e.exec_command("set number");
        assert!(e.show_numbers);
        e.exec_command("set nonumber");
        assert!(!e.show_numbers);
    }

    #[test]
    fn test_exec_goto_line() {
        let mut e = make_editor(&["a", "b", "c", "d", "e"]);
        e.exec_command("3");
        assert_eq!(e.row, 2);
    }

    // --- String helpers -----------------------------------------------------

    #[test]
    fn test_find_after() {
        assert_eq!(find_after("hello world", "world", 0), Some(6));
        assert_eq!(find_after("hello world", "world", 7), None);
    }

    #[test]
    fn test_find_before() {
        assert_eq!(find_before("hello world hello", "hello", 17), Some(12));
        assert_eq!(find_before("hello", "hello", 0), None);
    }

    // --- Visual mode --------------------------------------------------------

    #[test]
    fn test_visual_line_range_normalized() {
        let mut e = make_editor(&["a", "b", "c"]);
        e.mode = Mode::VisualLine;
        e.visual_anchor = (2, 0);
        e.row = 0;
        assert_eq!(e.visual_line_range(), (0, 2));
    }

    #[test]
    fn test_visual_char_range_normalized() {
        let mut e = make_editor(&["abcdef"]);
        e.mode = Mode::VisualChar;
        e.visual_anchor = (0, 4);
        e.row = 0;
        e.col = 1;
        let ((sr, sc), (er, ec)) = e.visual_range();
        assert_eq!((sr, sc), (0, 1));
        assert_eq!((er, ec), (0, 4));
    }

    // --- Open lines ---------------------------------------------------------

    #[test]
    fn test_open_line_below() {
        let mut e = make_editor(&["first"]);
        e.row = 0;
        e.open_line_below();
        assert_eq!(e.lines.len(), 2);
        assert_eq!(e.row, 1);
        assert_eq!(e.mode, Mode::Insert);
    }

    #[test]
    fn test_open_line_above() {
        let mut e = make_editor(&["first"]);
        e.row = 0;
        e.open_line_above();
        assert_eq!(e.lines.len(), 2);
        assert_eq!(e.row, 0);
        assert_eq!(e.mode, Mode::Insert);
    }

    // --- Line-number width --------------------------------------------------

    #[test]
    fn test_line_num_width_disabled() {
        let e = make_editor(&["a", "b"]);
        assert_eq!(e.line_num_width(), 0);
    }

    #[test]
    fn test_line_num_width_enabled_ten_lines() {
        let lines: Vec<&str> = vec!["a"; 10];
        let mut e = make_editor(&lines);
        e.show_numbers = true;
        assert_eq!(e.line_num_width(), 3); // 2 digits + 1 space
    }
}
