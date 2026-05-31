//! nano -- simple terminal text editor for OurOS.
//!
//! A minimal clone of GNU nano providing full-screen editing, search/replace,
//! cut/paste, undo/redo, syntax highlighting, and line numbers. Communicates
//! with the terminal via VT100 escape sequences and raw-mode character input.
//!
//! # Usage
//!
//! ```text
//! nano [OPTIONS] [FILE]
//!   -l            Show line numbers
//!   -t <width>    Tab width (default 4)
//!   -w            Disable word wrap display
//!   -h, --help    Show help and exit
//! ```

use std::env;
use std::fs;
use std::io::{self, Read, Write};

// ============================================================================
// Constants
// ============================================================================

/// Application name shown in the title bar.
const APP_NAME: &str = "nano";
/// Version string.
const APP_VERSION: &str = "0.1.0";
/// Default tab stop width (spaces).
const DEFAULT_TAB_WIDTH: usize = 4;

// ============================================================================
// Terminal I/O via raw stdin
// ============================================================================

/// Read a single byte from stdin, returning `None` on EOF/error.
fn read_byte() -> Option<u8> {
    let mut buf = [0u8; 1];
    match io::stdin().lock().read(&mut buf) {
        Ok(1) => Some(buf[0]),
        _ => None,
    }
}

/// Read a byte with a very short implicit timeout (non-blocking peek for
/// escape sequence continuation). On our OS this just tries one read --
/// if the terminal has buffered multi-byte escape sequences they arrive
/// together.
fn read_byte_eager() -> Option<u8> {
    read_byte()
}

/// Flush stdout.
fn flush() {
    let _ = io::stdout().flush();
}

/// Write a string to stdout.
fn write_str(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}

/// Write a formatted string to stdout (convenience macro replacement).
fn write_fmt(args: std::fmt::Arguments<'_>) {
    let _ = io::stdout().write_fmt(args);
}

// ============================================================================
// VT100 escape helpers
// ============================================================================

fn cursor_to(row: usize, col: usize) {
    write_fmt(format_args!("\x1b[{};{}H", row + 1, col + 1));
}

fn clear_screen() {
    write_str("\x1b[2J");
}

#[allow(dead_code)]
fn clear_line() {
    write_str("\x1b[2K");
}

#[allow(dead_code)]
fn clear_to_eol() {
    write_str("\x1b[K");
}

fn hide_cursor() {
    write_str("\x1b[?25l");
}

fn show_cursor() {
    write_str("\x1b[?25h");
}

fn enter_alternate_screen() {
    write_str("\x1b[?1049h");
}

fn leave_alternate_screen() {
    write_str("\x1b[?1049l");
}

#[allow(dead_code)]
fn set_fg(color: u8) {
    write_fmt(format_args!("\x1b[38;5;{}m", color));
}

#[allow(dead_code)]
fn set_bg(color: u8) {
    write_fmt(format_args!("\x1b[48;5;{}m", color));
}

#[allow(dead_code)]
fn set_bold() {
    write_str("\x1b[1m");
}

fn set_reverse() {
    write_str("\x1b[7m");
}

fn reset_attr() {
    write_str("\x1b[0m");
}

/// Enable raw mode by setting the terminal via stty-equivalent ioctls.
/// On OurOS we use the libc termios interface.
fn enable_raw_mode() -> Option<RawModeGuard> {
    // We use the POSIX termios interface through std's libc bindings.
    // On our custom target this goes through the POSIX compatibility layer.
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        let mut orig: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(fd, &mut orig) } != 0 {
            return None;
        }
        let mut raw = orig;
        // Input: no break, no CR-to-NL, no parity check, no strip, no flow control
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        // Output: disable post-processing
        raw.c_oflag &= !libc::OPOST;
        // Control: 8-bit chars
        raw.c_cflag |= libc::CS8;
        // Local: no echo, no canonical, no signals, no extended
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
        // Return each byte as it arrives
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        if unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) } != 0 {
            return None;
        }
        Some(RawModeGuard { orig })
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// RAII guard that restores the original terminal mode on drop.
struct RawModeGuard {
    #[cfg(unix)]
    orig: libc::termios,
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

// Bring in libc for termios.
#[cfg(unix)]
mod libc {
    //! Minimal libc bindings for termios -- just enough for raw mode.
    //! Our POSIX layer provides the real implementation.

    pub type TcflagT = u32;
    pub type CcT = u8;
    pub type SpeedT = u32;

    pub const NCCS: usize = 32;

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct termios {
        pub c_iflag: TcflagT,
        pub c_oflag: TcflagT,
        pub c_cflag: TcflagT,
        pub c_lflag: TcflagT,
        pub c_line: CcT,
        pub c_cc: [CcT; NCCS],
        pub c_ispeed: SpeedT,
        pub c_ospeed: SpeedT,
    }

    // Input flags
    pub const BRKINT: TcflagT = 0o000002;
    pub const ICRNL: TcflagT = 0o000400;
    pub const INPCK: TcflagT = 0o000020;
    pub const ISTRIP: TcflagT = 0o000040;
    pub const IXON: TcflagT = 0o002000;

    // Output flags
    pub const OPOST: TcflagT = 0o000001;

    // Control flags
    pub const CS8: TcflagT = 0o000060;

    // Local flags
    pub const ECHO: TcflagT = 0o000010;
    pub const ICANON: TcflagT = 0o000002;
    pub const ISIG: TcflagT = 0o000001;
    pub const IEXTEN: TcflagT = 0o100000;

    // c_cc indices
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;

    // tcsetattr actions
    pub const TCSAFLUSH: i32 = 2;

    unsafe extern "C" {
        pub fn tcgetattr(fd: i32, termios_p: *mut termios) -> i32;
        pub fn tcsetattr(fd: i32, action: i32, termios_p: *const termios) -> i32;
    }
}

// ============================================================================
// Key representation
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Key {
    Char(char),
    Ctrl(char),
    Alt(char),
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Backspace,
    Enter,
    Tab,
    Escape,
    /// Unknown or unhandled escape sequence.
    Unknown,
}

/// Read one keypress, decoding VT100 escape sequences.
fn read_key() -> Option<Key> {
    let b = read_byte()?;
    match b {
        // Ctrl+A through Ctrl+Z (except special ones)
        0 => Some(Key::Ctrl('@')),
        1..=6 => Some(Key::Ctrl((b'a' + b - 1) as char)),
        // 7 = Ctrl+G
        7 => Some(Key::Ctrl('g')),
        8 => Some(Key::Backspace), // Ctrl+H
        9 => Some(Key::Tab),       // Ctrl+I
        10 => Some(Key::Enter),    // Ctrl+J / LF
        11 => Some(Key::Ctrl('k')),
        12 => Some(Key::Ctrl('l')),
        13 => Some(Key::Enter), // Ctrl+M / CR
        14..=26 => Some(Key::Ctrl((b'a' + b - 1) as char)),
        27 => {
            // Escape sequence
            let Some(b2) = read_byte_eager() else {
                return Some(Key::Escape);
            };
            match b2 {
                b'[' => {
                    let Some(b3) = read_byte_eager() else {
                        return Some(Key::Escape);
                    };
                    match b3 {
                        b'A' => Some(Key::Up),
                        b'B' => Some(Key::Down),
                        b'C' => Some(Key::Right),
                        b'D' => Some(Key::Left),
                        b'H' => Some(Key::Home),
                        b'F' => Some(Key::End),
                        // CSI sequences with numeric parameter
                        b'0'..=b'9' => {
                            let Some(b4) = read_byte_eager() else {
                                return Some(Key::Unknown);
                            };
                            if b4 == b'~' {
                                match b3 {
                                    b'1' => Some(Key::Home),
                                    b'3' => Some(Key::Delete),
                                    b'4' => Some(Key::End),
                                    b'5' => Some(Key::PageUp),
                                    b'6' => Some(Key::PageDown),
                                    b'7' => Some(Key::Home),
                                    b'8' => Some(Key::End),
                                    _ => Some(Key::Unknown),
                                }
                            } else {
                                // Consume remaining bytes of longer sequence
                                // e.g. \x1b[15~ (F5) -- just skip
                                if b4 != b'~' {
                                    // Try to consume the tilde
                                    let _ = read_byte_eager();
                                }
                                Some(Key::Unknown)
                            }
                        }
                        _ => Some(Key::Unknown),
                    }
                }
                b'O' => {
                    let Some(b3) = read_byte_eager() else {
                        return Some(Key::Escape);
                    };
                    match b3 {
                        b'H' => Some(Key::Home),
                        b'F' => Some(Key::End),
                        _ => Some(Key::Unknown),
                    }
                }
                // Alt+key
                c @ b'a'..=b'z' => Some(Key::Alt(c as char)),
                c @ b'A'..=b'Z' => Some(Key::Alt((c as char).to_ascii_lowercase())),
                _ => Some(Key::Unknown),
            }
        }
        28 => Some(Key::Ctrl('\\')),
        29 => Some(Key::Ctrl(']')),
        30 => Some(Key::Ctrl('^')),
        31 => Some(Key::Ctrl('_')),
        127 => Some(Key::Backspace),
        // Printable ASCII and UTF-8
        32..=126 => Some(Key::Char(b as char)),
        // UTF-8 lead bytes -- decode multi-byte character
        0xC0..=0xDF => {
            let b2 = read_byte_eager().unwrap_or(0);
            let cp = (u32::from(b & 0x1F) << 6) | u32::from(b2 & 0x3F);
            char::from_u32(cp).map(Key::Char)
        }
        0xE0..=0xEF => {
            let b2 = read_byte_eager().unwrap_or(0);
            let b3 = read_byte_eager().unwrap_or(0);
            let cp =
                (u32::from(b & 0x0F) << 12) | (u32::from(b2 & 0x3F) << 6) | u32::from(b3 & 0x3F);
            char::from_u32(cp).map(Key::Char)
        }
        0xF0..=0xF7 => {
            let b2 = read_byte_eager().unwrap_or(0);
            let b3 = read_byte_eager().unwrap_or(0);
            let b4 = read_byte_eager().unwrap_or(0);
            let cp = (u32::from(b & 0x07) << 18)
                | (u32::from(b2 & 0x3F) << 12)
                | (u32::from(b3 & 0x3F) << 6)
                | u32::from(b4 & 0x3F);
            char::from_u32(cp).map(Key::Char)
        }
        _ => Some(Key::Unknown),
    }
}

// ============================================================================
// Terminal size detection
// ============================================================================

/// Query terminal size. Falls back to 80x24 if unavailable.
fn terminal_size() -> (usize, usize) {
    // Try the COLUMNS/LINES env vars first (set by our shell).
    let cols = env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    let rows = env::var("LINES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);
    if cols > 0 && rows > 0 {
        return (rows, cols);
    }

    // Try ioctl TIOCGWINSZ.
    #[cfg(unix)]
    {
        #[repr(C)]
        struct Winsize {
            ws_row: u16,
            ws_col: u16,
            ws_xpixel: u16,
            ws_ypixel: u16,
        }
        use std::os::unix::io::AsRawFd;
        let fd = io::stdout().as_raw_fd();
        let mut ws: Winsize = unsafe { std::mem::zeroed() };
        // TIOCGWINSZ = 0x5413 on linux
        let ret = unsafe {
            // ioctl(fd, TIOCGWINSZ, &mut ws)
            unsafe extern "C" {
                fn ioctl(fd: i32, request: u64, ...) -> i32;
            }
            ioctl(fd, 0x5413, &mut ws as *mut Winsize)
        };
        if ret == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
            return (ws.ws_row as usize, ws.ws_col as usize);
        }
    }

    // Fallback: probe by moving cursor to bottom-right and querying position.
    // This handshake blocks reading the terminal's reply from stdin, so it must
    // only run on a real interactive terminal — under a pipe/redirect/test the
    // reply never arrives and the read would hang forever.
    use std::io::IsTerminal;
    if !(io::stdin().is_terminal() && io::stdout().is_terminal()) {
        return (24, 80);
    }
    write_str("\x1b[999;999H\x1b[6n");
    flush();
    // Read response: \x1b[<rows>;<cols>R
    let mut resp = Vec::new();
    loop {
        match read_byte() {
            Some(b'R') => break,
            Some(c) => resp.push(c),
            None => break,
        }
        if resp.len() > 32 {
            break;
        }
    }
    // Parse "\x1b[rows;cols"
    let s = String::from_utf8_lossy(&resp);
    if let Some(coords) = s.strip_prefix("\x1b[") {
        if let Some((r, c)) = coords.split_once(';') {
            let rows_parsed = r.parse::<usize>().unwrap_or(24);
            let cols_parsed = c.parse::<usize>().unwrap_or(80);
            return (rows_parsed, cols_parsed);
        }
    }

    (24, 80)
}

// ============================================================================
// Undo/Redo
// ============================================================================

#[derive(Clone)]
enum UndoAction {
    /// Inserted text at (line, col). Store the text inserted so we can remove it.
    Insert {
        line: usize,
        col: usize,
        text: String,
    },
    /// Deleted text at (line, col). Store what was deleted so we can re-insert.
    DeleteRange {
        line: usize,
        col: usize,
        text: String,
    },
    /// Inserted a new line (Enter pressed). Stores the line index of the new line.
    InsertLine { line: usize },
    /// Joined two lines (Backspace at start). Stores the line index and the
    /// column where the join happened (original length of the upper line).
    JoinLine { line: usize, col: usize },
    /// Cut a line. Stores the line index and its content.
    CutLine { line: usize, content: String },
    /// Pasted (uncutted) a line. Stores the line index.
    PasteLine { line: usize },
}

struct UndoStack {
    undo: Vec<UndoAction>,
    redo: Vec<UndoAction>,
}

impl UndoStack {
    fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn push(&mut self, action: UndoAction) {
        self.undo.push(action);
        // Any new action invalidates the redo stack.
        self.redo.clear();
    }

    fn pop_undo(&mut self) -> Option<UndoAction> {
        self.undo.pop()
    }

    fn push_redo(&mut self, action: UndoAction) {
        self.redo.push(action);
    }

    fn pop_redo(&mut self) -> Option<UndoAction> {
        self.redo.pop()
    }
}

// ============================================================================
// Syntax highlighting
// ============================================================================

/// Simple syntax highlight categories.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Highlight {
    Normal,
    Keyword,
    Type,
    String,
    Comment,
    Number,
    Operator,
}

impl Highlight {
    /// ANSI 256-color code for this highlight category.
    fn color(self) -> u8 {
        match self {
            Self::Normal => 255,  // white
            Self::Keyword => 204, // pink/red
            Self::Type => 222,    // gold
            Self::String => 114,  // green
            Self::Comment => 242, // gray
            Self::Number => 141,  // purple
            Self::Operator => 81, // cyan
        }
    }
}

/// Determine the file type from the filename extension.
fn detect_filetype(filename: &str) -> Option<&'static str> {
    let ext = filename.rsplit('.').next()?;
    match ext {
        "rs" => Some("rust"),
        "py" => Some("python"),
        "c" | "h" => Some("c"),
        "sh" | "bash" => Some("shell"),
        "json" => Some("json"),
        "toml" => Some("toml"),
        "yaml" | "yml" => Some("yaml"),
        "html" | "htm" => Some("html"),
        _ => None,
    }
}

/// Return the set of keywords for a given filetype.
fn keywords_for(filetype: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match filetype {
        "rust" => (
            &[
                "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
                "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct",
                "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield",
            ],
            &[
                "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "str",
                "u8", "u16", "u32", "u64", "u128", "usize", "String", "Vec", "Option", "Result",
                "Box", "Rc", "Arc",
            ],
        ),
        "python" => (
            &[
                "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                "del", "elif", "else", "except", "False", "finally", "for", "from", "global", "if",
                "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass", "raise",
                "return", "True", "try", "while", "with", "yield",
            ],
            &[
                "int", "float", "str", "bool", "list", "dict", "set", "tuple", "bytes",
            ],
        ),
        "c" => (
            &[
                "auto", "break", "case", "char", "const", "continue", "default", "do", "double",
                "else", "enum", "extern", "float", "for", "goto", "if", "inline", "int", "long",
                "register", "return", "short", "signed", "sizeof", "static", "struct", "switch",
                "typedef", "union", "unsigned", "void", "volatile", "while",
            ],
            &[
                "int8_t", "int16_t", "int32_t", "int64_t", "uint8_t", "uint16_t", "uint32_t",
                "uint64_t", "size_t", "ssize_t", "bool", "NULL", "FILE",
            ],
        ),
        "shell" => (
            &[
                "if", "then", "else", "elif", "fi", "case", "esac", "for", "while", "until", "do",
                "done", "in", "function", "select", "return", "exit", "break", "continue", "local",
                "export", "readonly", "declare", "typeset", "source",
            ],
            &["true", "false"],
        ),
        _ => (&[], &[]),
    }
}

/// Return the single-line comment prefix for a filetype (if any).
fn comment_prefix(filetype: &str) -> Option<&'static str> {
    match filetype {
        "rust" | "c" => Some("//"),
        "python" | "shell" | "toml" | "yaml" => Some("#"),
        _ => None,
    }
}

/// Highlight a single line, returning a Vec of Highlight per character.
fn highlight_line(line: &str, filetype: Option<&str>) -> Vec<Highlight> {
    let len = line.len();
    let mut hl = vec![Highlight::Normal; len];

    let ft = match filetype {
        Some(ft) => ft,
        None => return hl,
    };

    let bytes = line.as_bytes();

    // Check for line comment.
    if let Some(prefix) = comment_prefix(ft) {
        let pb = prefix.as_bytes();
        if bytes.len() >= pb.len() && &bytes[..pb.len()] == pb {
            for h in &mut hl {
                *h = Highlight::Comment;
            }
            return hl;
        }
        // Also check for comment after leading whitespace.
        let trimmed_start = bytes.iter().position(|&b| b != b' ' && b != b'\t');
        if let Some(start) = trimmed_start {
            if bytes.len() >= start + pb.len() && &bytes[start..start + pb.len()] == pb {
                for h in hl.iter_mut().skip(start) {
                    *h = Highlight::Comment;
                }
                return hl;
            }
        }
    }

    let (keywords, types) = keywords_for(ft);

    let mut i = 0;
    while i < len {
        let b = bytes[i];

        // String literals.
        if b == b'"' || b == b'\'' {
            let quote = b;
            hl[i] = Highlight::String;
            i += 1;
            while i < len {
                hl[i] = Highlight::String;
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 1;
                    hl[i] = Highlight::String;
                } else if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Numbers.
        if b.is_ascii_digit() || (b == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()) {
            // Check that the previous char is not alphanumeric (word boundary).
            let prev_alpha =
                i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_');
            if !prev_alpha {
                while i < len
                    && (bytes[i].is_ascii_digit()
                        || bytes[i] == b'.'
                        || bytes[i] == b'x'
                        || bytes[i] == b'X'
                        || bytes[i] == b'o'
                        || bytes[i] == b'b'
                        || bytes[i] == b'_'
                        || (bytes[i] >= b'a' && bytes[i] <= b'f')
                        || (bytes[i] >= b'A' && bytes[i] <= b'F'))
                {
                    hl[i] = Highlight::Number;
                    i += 1;
                }
                continue;
            }
        }

        // Operators.
        if matches!(
            b,
            b'+' | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'='
                | b'!'
                | b'<'
                | b'>'
                | b'&'
                | b'|'
                | b'^'
                | b'~'
        ) {
            hl[i] = Highlight::Operator;
            i += 1;
            continue;
        }

        // Identifiers / keywords.
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];
            if keywords.contains(&word) {
                for h in hl.iter_mut().take(i).skip(start) {
                    *h = Highlight::Keyword;
                }
            } else if types.contains(&word) {
                for h in hl.iter_mut().take(i).skip(start) {
                    *h = Highlight::Type;
                }
            }
            continue;
        }

        i += 1;
    }

    hl
}

// ============================================================================
// Editor state
// ============================================================================

struct Editor {
    /// Lines of the buffer.  Always has at least one (empty) line.
    lines: Vec<String>,
    /// Cursor position: line index (0-based).
    cursor_line: usize,
    /// Cursor position: column index (0-based, byte offset into the line).
    cursor_col: usize,
    /// First visible line (vertical scroll offset).
    scroll_row: usize,
    /// First visible column (horizontal scroll offset).
    scroll_col: usize,
    /// Terminal rows (total).
    term_rows: usize,
    /// Terminal columns.
    term_cols: usize,
    /// Filename (None = new file).
    filename: Option<String>,
    /// Whether the buffer has been modified since the last save.
    modified: bool,
    /// Status message (shown briefly on the status bar).
    status_msg: String,
    /// Show line numbers?
    show_line_numbers: bool,
    /// Tab width in spaces.
    tab_width: usize,
    /// Insert spaces instead of tab character.
    tabs_to_spaces: bool,
    /// Enable word-wrap display.
    word_wrap: bool,
    /// Detected file type for syntax highlighting.
    filetype: Option<&'static str>,
    /// Cut buffer for Ctrl+K / Ctrl+U.
    cut_buffer: Vec<String>,
    /// Undo/redo stack.
    undo: UndoStack,
    /// Whether we are currently showing the help screen.
    showing_help: bool,
    /// Desired column when moving up/down (sticky column).
    desired_col: Option<usize>,
    /// Search query (last used).
    last_search: String,
}

impl Editor {
    fn new() -> Self {
        let (rows, cols) = terminal_size();
        Self {
            lines: vec![String::new()],
            cursor_line: 0,
            cursor_col: 0,
            scroll_row: 0,
            scroll_col: 0,
            term_rows: rows,
            term_cols: cols,
            filename: None,
            modified: false,
            status_msg: String::new(),
            show_line_numbers: false,
            tab_width: DEFAULT_TAB_WIDTH,
            tabs_to_spaces: true,
            word_wrap: true,
            filetype: None,
            cut_buffer: Vec::new(),
            undo: UndoStack::new(),
            showing_help: false,
            desired_col: None,
            last_search: String::new(),
        }
    }

    /// Number of rows available for the text editing area (excluding title,
    /// status, and shortcut bars).
    fn edit_rows(&self) -> usize {
        // title(1) + status(1) + shortcuts(2) = 4 reserved rows
        self.term_rows.saturating_sub(4)
    }

    /// Width of the line-number gutter (0 if line numbers are off).
    fn gutter_width(&self) -> usize {
        if self.show_line_numbers {
            // Enough digits for the total line count, plus a space separator.
            let digits = format!("{}", self.lines.len()).len();
            digits.max(3) + 1 // at least 3 digits + 1 space
        } else {
            0
        }
    }

    /// Number of columns available for text after the gutter.
    fn text_cols(&self) -> usize {
        self.term_cols.saturating_sub(self.gutter_width())
    }

    // ========================================================================
    // File I/O
    // ========================================================================

    fn open_file(&mut self, path: &str) {
        match fs::read_to_string(path) {
            Ok(content) => {
                self.lines = content.lines().map(|l| l.to_string()).collect();
                if self.lines.is_empty() {
                    self.lines.push(String::new());
                }
                self.filename = Some(path.to_string());
                self.filetype = detect_filetype(path);
                self.modified = false;
                self.status_msg = format!("Read {} lines", self.lines.len());
            }
            Err(e) => {
                // New file -- open empty.
                if e.kind() == io::ErrorKind::NotFound {
                    self.filename = Some(path.to_string());
                    self.filetype = detect_filetype(path);
                    self.status_msg = format!("[ New File: {path} ]");
                } else {
                    self.status_msg = format!("Error opening {path}: {e}");
                }
            }
        }
    }

    fn save_file(&mut self) -> bool {
        let path = match &self.filename {
            Some(p) => p.clone(),
            None => return false, // caller must prompt for filename first
        };
        let mut content = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            content.push_str(line);
            if i + 1 < self.lines.len() {
                content.push('\n');
            }
        }
        // Ensure trailing newline.
        if !content.ends_with('\n') {
            content.push('\n');
        }
        match fs::write(&path, &content) {
            Ok(()) => {
                self.modified = false;
                let bytes = content.len();
                self.status_msg = format!("Wrote {} lines ({bytes} bytes)", self.lines.len());
                true
            }
            Err(e) => {
                self.status_msg = format!("Error writing {path}: {e}");
                false
            }
        }
    }

    // ========================================================================
    // Cursor / scroll management
    // ========================================================================

    fn clamp_cursor(&mut self) {
        if self.cursor_line >= self.lines.len() {
            self.cursor_line = self.lines.len().saturating_sub(1);
        }
        let line_len = self.lines[self.cursor_line].len();
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
        }
    }

    fn ensure_visible(&mut self) {
        let edit_rows = self.edit_rows();
        let text_cols = self.text_cols();

        // Vertical scroll.
        if self.cursor_line < self.scroll_row {
            self.scroll_row = self.cursor_line;
        }
        if self.cursor_line >= self.scroll_row + edit_rows {
            self.scroll_row = self.cursor_line.saturating_sub(edit_rows.saturating_sub(1));
        }

        // Horizontal scroll.
        if self.cursor_col < self.scroll_col {
            self.scroll_col = self.cursor_col;
        }
        if self.cursor_col >= self.scroll_col + text_cols {
            self.scroll_col = self.cursor_col.saturating_sub(text_cols.saturating_sub(1));
        }
    }

    // ========================================================================
    // Movement
    // ========================================================================

    fn move_up(&mut self) {
        if self.cursor_line > 0 {
            self.cursor_line -= 1;
            if let Some(dc) = self.desired_col {
                self.cursor_col = dc.min(self.lines[self.cursor_line].len());
            }
        }
    }

    fn move_down(&mut self) {
        if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            if let Some(dc) = self.desired_col {
                self.cursor_col = dc.min(self.lines[self.cursor_line].len());
            }
        }
    }

    fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_line > 0 {
            self.cursor_line -= 1;
            self.cursor_col = self.lines[self.cursor_line].len();
        }
        self.desired_col = None;
    }

    fn move_right(&mut self) {
        let line_len = self.lines[self.cursor_line].len();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_line + 1 < self.lines.len() {
            self.cursor_line += 1;
            self.cursor_col = 0;
        }
        self.desired_col = None;
    }

    fn move_home(&mut self) {
        self.cursor_col = 0;
        self.desired_col = None;
    }

    fn move_end(&mut self) {
        self.cursor_col = self.lines[self.cursor_line].len();
        self.desired_col = None;
    }

    fn page_up(&mut self) {
        let edit_rows = self.edit_rows();
        if self.cursor_line > edit_rows {
            self.cursor_line -= edit_rows;
        } else {
            self.cursor_line = 0;
        }
        self.clamp_cursor();
    }

    fn page_down(&mut self) {
        let edit_rows = self.edit_rows();
        self.cursor_line += edit_rows;
        self.clamp_cursor();
    }

    // ========================================================================
    // Editing operations
    // ========================================================================

    fn insert_char(&mut self, ch: char) {
        let s = ch.to_string();
        self.lines[self.cursor_line].insert_str(self.cursor_col, &s);
        self.undo.push(UndoAction::Insert {
            line: self.cursor_line,
            col: self.cursor_col,
            text: s,
        });
        self.cursor_col += ch.len_utf8();
        self.modified = true;
        self.desired_col = None;
    }

    fn insert_tab(&mut self) {
        if self.tabs_to_spaces {
            let spaces = self.tab_width - (self.cursor_col % self.tab_width);
            let s: String = " ".repeat(spaces);
            self.lines[self.cursor_line].insert_str(self.cursor_col, &s);
            self.undo.push(UndoAction::Insert {
                line: self.cursor_line,
                col: self.cursor_col,
                text: s.clone(),
            });
            self.cursor_col += spaces;
        } else {
            self.lines[self.cursor_line].insert(self.cursor_col, '\t');
            self.undo.push(UndoAction::Insert {
                line: self.cursor_line,
                col: self.cursor_col,
                text: "\t".to_string(),
            });
            self.cursor_col += 1;
        }
        self.modified = true;
        self.desired_col = None;
    }

    fn insert_newline(&mut self) {
        let tail = self.lines[self.cursor_line][self.cursor_col..].to_string();
        self.lines[self.cursor_line].truncate(self.cursor_col);
        self.cursor_line += 1;
        self.lines.insert(self.cursor_line, tail);
        self.cursor_col = 0;
        self.undo.push(UndoAction::InsertLine {
            line: self.cursor_line,
        });
        self.modified = true;
        self.desired_col = None;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let removed = self.lines[self.cursor_line]
                .remove(self.cursor_col - 1)
                .to_string();
            self.cursor_col -= 1;
            self.undo.push(UndoAction::DeleteRange {
                line: self.cursor_line,
                col: self.cursor_col,
                text: removed,
            });
            self.modified = true;
        } else if self.cursor_line > 0 {
            // Join with previous line.
            let current = self.lines.remove(self.cursor_line);
            self.cursor_line -= 1;
            let join_col = self.lines[self.cursor_line].len();
            self.lines[self.cursor_line].push_str(&current);
            self.cursor_col = join_col;
            self.undo.push(UndoAction::JoinLine {
                line: self.cursor_line,
                col: join_col,
            });
            self.modified = true;
        }
        self.desired_col = None;
    }

    fn delete_char(&mut self) {
        let line_len = self.lines[self.cursor_line].len();
        if self.cursor_col < line_len {
            let removed = self.lines[self.cursor_line]
                .remove(self.cursor_col)
                .to_string();
            self.undo.push(UndoAction::DeleteRange {
                line: self.cursor_line,
                col: self.cursor_col,
                text: removed,
            });
            self.modified = true;
        } else if self.cursor_line + 1 < self.lines.len() {
            // Join with next line.
            let next = self.lines.remove(self.cursor_line + 1);
            let join_col = self.lines[self.cursor_line].len();
            self.lines[self.cursor_line].push_str(&next);
            self.undo.push(UndoAction::JoinLine {
                line: self.cursor_line,
                col: join_col,
            });
            self.modified = true;
        }
    }

    fn cut_line(&mut self) {
        let content = self.lines[self.cursor_line].clone();
        self.undo.push(UndoAction::CutLine {
            line: self.cursor_line,
            content: content.clone(),
        });
        self.cut_buffer.push(content);
        if self.lines.len() > 1 {
            self.lines.remove(self.cursor_line);
            if self.cursor_line >= self.lines.len() {
                self.cursor_line = self.lines.len().saturating_sub(1);
            }
        } else {
            self.lines[0].clear();
        }
        self.clamp_cursor();
        self.modified = true;
        self.status_msg = "Cut 1 line".to_string();
    }

    fn uncut_line(&mut self) {
        if self.cut_buffer.is_empty() {
            self.status_msg = "Cut buffer is empty".to_string();
            return;
        }
        // Paste all cut lines below current line.
        for (i, line) in self.cut_buffer.clone().iter().enumerate() {
            self.lines.insert(self.cursor_line + 1 + i, line.clone());
            self.undo.push(UndoAction::PasteLine {
                line: self.cursor_line + 1 + i,
            });
        }
        let count = self.cut_buffer.len();
        self.cursor_line += count;
        self.clamp_cursor();
        self.modified = true;
        self.status_msg = format!("Uncut {count} line(s)");
    }

    // ========================================================================
    // Undo / Redo
    // ========================================================================

    fn perform_undo(&mut self) {
        let Some(action) = self.undo.pop_undo() else {
            self.status_msg = "Nothing to undo".to_string();
            return;
        };
        match action.clone() {
            UndoAction::Insert {
                line,
                col,
                ref text,
            } => {
                // Remove the inserted text.
                let end = col + text.len();
                self.lines[line].replace_range(col..end, "");
                self.cursor_line = line;
                self.cursor_col = col;
            }
            UndoAction::DeleteRange {
                line,
                col,
                ref text,
            } => {
                // Re-insert the deleted text.
                self.lines[line].insert_str(col, text);
                self.cursor_line = line;
                self.cursor_col = col + text.len();
            }
            UndoAction::InsertLine { line } => {
                // Merge line back with previous.
                if line > 0 && line < self.lines.len() {
                    let removed = self.lines.remove(line);
                    self.lines[line - 1].push_str(&removed);
                    self.cursor_line = line - 1;
                    self.cursor_col = self.lines[self.cursor_line].len() - removed.len();
                }
            }
            UndoAction::JoinLine { line, col } => {
                // Split the line back.
                if line < self.lines.len() {
                    let tail = self.lines[line][col..].to_string();
                    self.lines[line].truncate(col);
                    self.lines.insert(line + 1, tail);
                    self.cursor_line = line + 1;
                    self.cursor_col = 0;
                }
            }
            UndoAction::CutLine { line, ref content } => {
                // Re-insert the cut line.
                self.lines.insert(line, content.clone());
                self.cursor_line = line;
                self.cursor_col = 0;
            }
            UndoAction::PasteLine { line } => {
                // Remove the pasted line.
                if line < self.lines.len() && self.lines.len() > 1 {
                    self.lines.remove(line);
                }
                if self.cursor_line >= self.lines.len() {
                    self.cursor_line = self.lines.len().saturating_sub(1);
                }
            }
        }
        self.undo.push_redo(action);
        self.modified = true;
        self.clamp_cursor();
        self.status_msg = "Undo".to_string();
    }

    fn perform_redo(&mut self) {
        let Some(action) = self.undo.pop_redo() else {
            self.status_msg = "Nothing to redo".to_string();
            return;
        };
        match action.clone() {
            UndoAction::Insert {
                line,
                col,
                ref text,
            } => {
                self.lines[line].insert_str(col, text);
                self.cursor_line = line;
                self.cursor_col = col + text.len();
            }
            UndoAction::DeleteRange {
                line,
                col,
                ref text,
            } => {
                let end = col + text.len();
                self.lines[line].replace_range(col..end, "");
                self.cursor_line = line;
                self.cursor_col = col;
            }
            UndoAction::InsertLine { line } => {
                if line > 0 {
                    let prev = &self.lines[line - 1];
                    let tail = prev[prev.len()..].to_string();
                    self.lines.insert(line, tail);
                    self.cursor_line = line;
                    self.cursor_col = 0;
                }
            }
            UndoAction::JoinLine { line, col } => {
                if line + 1 < self.lines.len() {
                    let next = self.lines.remove(line + 1);
                    self.lines[line].push_str(&next);
                    self.cursor_line = line;
                    self.cursor_col = col;
                }
            }
            UndoAction::CutLine { line, ref content } => {
                if line < self.lines.len() && self.lines[line] == *content {
                    if self.lines.len() > 1 {
                        self.lines.remove(line);
                    } else {
                        self.lines[0].clear();
                    }
                }
                self.clamp_cursor();
            }
            UndoAction::PasteLine { line } => {
                if !self.cut_buffer.is_empty() {
                    // Re-paste -- use last cut buffer entry.
                    let content = self.cut_buffer.last().cloned().unwrap_or_default();
                    self.lines.insert(line, content);
                }
                self.clamp_cursor();
            }
        }
        self.undo.push(action);
        self.modified = true;
        self.clamp_cursor();
        self.status_msg = "Redo".to_string();
    }

    // ========================================================================
    // Prompts (blocking mini-input at the status bar)
    // ========================================================================

    /// Show a prompt at the status line and collect a line of input.
    /// Returns None if the user presses Escape.
    fn prompt(&mut self, message: &str, prefill: &str) -> Option<String> {
        let mut input = prefill.to_string();
        let mut cursor = input.len();
        loop {
            // Draw the prompt on the status line.
            let status_row = self.term_rows.saturating_sub(3);
            cursor_to(status_row, 0);
            set_reverse();
            let prompt_text = format!("{message}{input}");
            let padded = format!("{prompt_text:<width$}", width = self.term_cols);
            write_str(&padded[..padded.len().min(self.term_cols)]);
            reset_attr();

            // Position cursor inside the prompt.
            let prompt_cursor_col = message.len() + cursor;
            cursor_to(
                status_row,
                prompt_cursor_col.min(self.term_cols.saturating_sub(1)),
            );
            show_cursor();
            flush();

            let Some(key) = read_key() else {
                continue;
            };
            match key {
                Key::Enter => return Some(input),
                Key::Escape | Key::Ctrl('c') => return None,
                Key::Backspace => {
                    if cursor > 0 {
                        input.remove(cursor - 1);
                        cursor -= 1;
                    }
                }
                Key::Delete => {
                    if cursor < input.len() {
                        input.remove(cursor);
                    }
                }
                Key::Left => {
                    if cursor > 0 {
                        cursor -= 1;
                    }
                }
                Key::Right => {
                    if cursor < input.len() {
                        cursor += 1;
                    }
                }
                Key::Home => cursor = 0,
                Key::End => cursor = input.len(),
                Key::Char(c) => {
                    input.insert(cursor, c);
                    cursor += 1;
                }
                _ => {}
            }
        }
    }

    // ========================================================================
    // Search
    // ========================================================================

    fn search(&mut self) {
        let query = match self.prompt("Search: ", &self.last_search.clone()) {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.status_msg = "Cancelled".to_string();
                return;
            }
        };
        self.last_search = query.clone();

        // Search forward from current position.
        let start_line = self.cursor_line;
        let start_col = self.cursor_col + 1; // skip current position

        // Search rest of current line.
        if start_col <= self.lines[start_line].len() {
            if let Some(pos) = self.lines[start_line][start_col..].find(&query) {
                self.cursor_line = start_line;
                self.cursor_col = start_col + pos;
                self.status_msg = String::new();
                self.desired_col = None;
                return;
            }
        }

        // Search subsequent lines, wrapping around.
        let total = self.lines.len();
        for offset in 1..=total {
            let li = (start_line + offset) % total;
            if let Some(pos) = self.lines[li].find(&query) {
                self.cursor_line = li;
                self.cursor_col = pos;
                if li < start_line {
                    self.status_msg = "Search wrapped".to_string();
                } else {
                    self.status_msg = String::new();
                }
                self.desired_col = None;
                return;
            }
        }

        self.status_msg = format!("\"{query}\" not found");
    }

    fn search_and_replace(&mut self) {
        let query = match self.prompt("Search (replace): ", &self.last_search.clone()) {
            Some(q) if !q.is_empty() => q,
            _ => {
                self.status_msg = "Cancelled".to_string();
                return;
            }
        };
        self.last_search = query.clone();

        let replacement = match self.prompt("Replace with: ", "") {
            Some(r) => r,
            None => {
                self.status_msg = "Cancelled".to_string();
                return;
            }
        };

        let mut count = 0usize;
        for line in &mut self.lines {
            while let Some(pos) = line.find(&query) {
                line.replace_range(pos..pos + query.len(), &replacement);
                count += 1;
            }
        }

        if count > 0 {
            self.modified = true;
            self.status_msg = format!("Replaced {count} occurrence(s)");
        } else {
            self.status_msg = format!("\"{query}\" not found");
        }
        self.clamp_cursor();
    }

    fn goto_line(&mut self) {
        let input = match self.prompt("Enter line number: ", "") {
            Some(s) => s,
            None => {
                self.status_msg = "Cancelled".to_string();
                return;
            }
        };
        match input.trim().parse::<usize>() {
            Ok(n) if n >= 1 => {
                self.cursor_line = (n - 1).min(self.lines.len().saturating_sub(1));
                self.cursor_col = 0;
                self.desired_col = None;
                self.status_msg = String::new();
            }
            _ => {
                self.status_msg = "Invalid line number".to_string();
            }
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    fn render(&mut self) {
        hide_cursor();
        let buf = &mut String::with_capacity(self.term_rows * self.term_cols * 2);

        if self.showing_help {
            self.render_help_screen(buf);
        } else {
            self.render_title_bar(buf);
            self.render_text_area(buf);
            self.render_status_bar(buf);
            self.render_shortcut_bar(buf);
        }

        write_str(buf);

        // Position the real cursor.
        if !self.showing_help {
            let gw = self.gutter_width();
            let screen_row = self.cursor_line.saturating_sub(self.scroll_row) + 1; // +1 for title
            let screen_col = self.cursor_col.saturating_sub(self.scroll_col) + gw;
            cursor_to(screen_row, screen_col);
        }

        show_cursor();
        flush();
    }

    fn render_title_bar(&self, buf: &mut String) {
        use std::fmt::Write;

        let _ = write!(buf, "\x1b[1;1H"); // row 1, col 1
        let _ = write!(buf, "\x1b[7m"); // reverse video

        let fname = self.filename.as_deref().unwrap_or("[New Buffer]");
        let mod_indicator = if self.modified { " [Modified]" } else { "" };
        let title = format!("  {APP_NAME} {APP_VERSION}    {fname}{mod_indicator}");

        let padded = format!("{title:<width$}", width = self.term_cols);
        // Truncate to terminal width.
        for (i, ch) in padded.chars().enumerate() {
            if i >= self.term_cols {
                break;
            }
            let _ = write!(buf, "{ch}");
        }
        let _ = write!(buf, "\x1b[0m"); // reset
    }

    fn render_text_area(&self, buf: &mut String) {
        use std::fmt::Write;

        let edit_rows = self.edit_rows();
        let text_cols = self.text_cols();
        let gw = self.gutter_width();

        for screen_row in 0..edit_rows {
            let file_line = self.scroll_row + screen_row;
            // Move to the correct screen position (row offset by 1 for title bar).
            let _ = write!(buf, "\x1b[{};1H", screen_row + 2);
            let _ = write!(buf, "\x1b[K"); // clear line

            if file_line < self.lines.len() {
                // Draw gutter.
                if self.show_line_numbers {
                    let digits = gw.saturating_sub(1);
                    let _ = write!(buf, "\x1b[38;5;240m"); // dim gray
                    let _ = write!(buf, "{:>width$} ", file_line + 1, width = digits);
                    let _ = write!(buf, "\x1b[0m");
                }

                // Draw text with syntax highlighting.
                let line = &self.lines[file_line];
                let highlights = highlight_line(line, self.filetype);

                let end_col = (self.scroll_col + text_cols).min(line.len());
                let start_col = self.scroll_col.min(line.len());

                let mut last_hl = Highlight::Normal;
                for col in start_col..end_col {
                    let ch = line.as_bytes().get(col).copied().unwrap_or(b' ');
                    let hl = highlights.get(col).copied().unwrap_or(Highlight::Normal);

                    if hl != last_hl {
                        let _ = write!(buf, "\x1b[38;5;{}m", hl.color());
                        last_hl = hl;
                    }

                    if ch == b'\t' {
                        // Render tab as spaces.
                        let spaces = self.tab_width - ((col - start_col) % self.tab_width);
                        for _ in 0..spaces {
                            buf.push(' ');
                        }
                    } else if ch < 0x20 {
                        buf.push('?'); // control char placeholder
                    } else {
                        buf.push(ch as char);
                    }
                }

                if last_hl != Highlight::Normal {
                    let _ = write!(buf, "\x1b[0m");
                }
            } else {
                // Past end of file -- draw tilde.
                if self.show_line_numbers {
                    let digits = gw.saturating_sub(1);
                    let _ = write!(buf, "{:>width$} ", "~", width = digits);
                } else {
                    buf.push('~');
                }
            }
        }
    }

    fn render_status_bar(&self, buf: &mut String) {
        use std::fmt::Write;

        let status_row = self.term_rows.saturating_sub(3);
        let _ = write!(buf, "\x1b[{};1H", status_row + 1);
        let _ = write!(buf, "\x1b[7m"); // reverse video

        let left = if self.status_msg.is_empty() {
            let fname = self.filename.as_deref().unwrap_or("[No Name]");
            let mod_str = if self.modified { " [Modified]" } else { "" };
            format!(" {fname}{mod_str}")
        } else {
            format!(" {}", self.status_msg)
        };

        let right = format!(
            "Line {}/{} Col {} ",
            self.cursor_line + 1,
            self.lines.len(),
            self.cursor_col + 1,
        );

        let padding = self.term_cols.saturating_sub(left.len() + right.len());
        let line = format!("{left}{:padding$}{right}", "", padding = padding);

        for (i, ch) in line.chars().enumerate() {
            if i >= self.term_cols {
                break;
            }
            let _ = write!(buf, "{ch}");
        }
        let _ = write!(buf, "\x1b[0m");
    }

    fn render_shortcut_bar(&self, buf: &mut String) {
        use std::fmt::Write;

        let shortcuts_1 = [
            ("^G", "Help"),
            ("^O", "Write Out"),
            ("^W", "Where Is"),
            ("^K", "Cut"),
            ("^C", "Cur Pos"),
        ];
        let shortcuts_2 = [
            ("^X", "Exit"),
            ("^\\", "Replace"),
            ("^U", "Uncut"),
            ("^Z", "Undo"),
            ("^_", "Go To Line"),
        ];

        let row1 = self.term_rows.saturating_sub(2);
        let row2 = self.term_rows.saturating_sub(1);

        // Row 1
        let _ = write!(buf, "\x1b[{};1H\x1b[K", row1 + 1);
        self.render_shortcut_row(buf, &shortcuts_1);

        // Row 2
        let _ = write!(buf, "\x1b[{};1H\x1b[K", row2 + 1);
        self.render_shortcut_row(buf, &shortcuts_2);
    }

    fn render_shortcut_row(&self, buf: &mut String, shortcuts: &[(&str, &str)]) {
        use std::fmt::Write;

        let item_width = self.term_cols / shortcuts.len().max(1);

        for (key, label) in shortcuts {
            // Key in reverse video.
            let _ = write!(buf, "\x1b[7m{key}\x1b[0m");
            // Label in normal text, truncated to fit.
            let max_label = item_width.saturating_sub(key.len() + 1);
            let display_label = if label.len() > max_label {
                &label[..max_label]
            } else {
                label
            };
            let _ = write!(buf, " {display_label:<width$}", width = max_label);
        }
    }

    fn render_help_screen(&self, buf: &mut String) {
        use std::fmt::Write;

        let _ = write!(buf, "\x1b[2J\x1b[1;1H"); // clear + home

        let _ = write!(buf, "\x1b[7m");
        let title = format!("  {APP_NAME} {APP_VERSION} Help");
        let padded = format!("{title:<width$}", width = self.term_cols);
        for (i, ch) in padded.chars().enumerate() {
            if i >= self.term_cols {
                break;
            }
            let _ = write!(buf, "{ch}");
        }
        let _ = write!(buf, "\x1b[0m\r\n\r\n");

        let help_lines = [
            "Navigation:",
            "  Arrow keys      Move cursor",
            "  Home / End      Go to start / end of line",
            "  PgUp / PgDn     Scroll one page up / down",
            "  Ctrl+_          Go to line number",
            "",
            "Editing:",
            "  Enter           Insert new line",
            "  Backspace       Delete character before cursor",
            "  Delete          Delete character at cursor",
            "  Tab             Insert tab (spaces or tab char)",
            "  Ctrl+K          Cut current line",
            "  Ctrl+U          Paste (uncut) line(s)",
            "  Ctrl+Z          Undo last action",
            "",
            "File Operations:",
            "  Ctrl+O          Save file (Write Out)",
            "  Ctrl+X          Exit (prompts to save if modified)",
            "",
            "Search:",
            "  Ctrl+W          Search forward",
            "  Ctrl+\\          Search and replace",
            "",
            "Display:",
            "  Alt+N           Toggle line numbers",
            "  Ctrl+C          Show cursor position",
            "  Ctrl+G          Show this help screen",
            "  Ctrl+L          Refresh screen",
            "",
            "Press any key to return to editing.",
        ];

        for line in &help_lines {
            let _ = write!(buf, "  {line}\r\n");
        }
    }

    // ========================================================================
    // Command: save-as (Ctrl+O)
    // ========================================================================

    fn cmd_save(&mut self) {
        if self.filename.is_none() {
            match self.prompt("File Name to Write: ", "") {
                Some(name) if !name.is_empty() => {
                    self.filename = Some(name.clone());
                    self.filetype = detect_filetype(&name);
                }
                _ => {
                    self.status_msg = "Cancelled".to_string();
                    return;
                }
            }
        }
        self.save_file();
    }

    /// Returns true if the user confirms exit (or there are no changes).
    fn cmd_exit(&mut self) -> bool {
        if !self.modified {
            return true;
        }
        let fname = self.filename.as_deref().unwrap_or("[New Buffer]");
        let answer = self.prompt(&format!("Save modified buffer ({fname})? (Y/N/C): "), "");
        match answer.as_deref() {
            Some("Y") | Some("y") | Some("yes") => {
                self.cmd_save();
                !self.modified // exit only if save succeeded
            }
            Some("N") | Some("n") | Some("no") => true,
            _ => false, // cancel or anything else
        }
    }

    fn cmd_show_position(&mut self) {
        let total_lines = self.lines.len();
        let line = self.cursor_line + 1;
        let col = self.cursor_col + 1;
        let total_chars: usize = self.lines.iter().map(|l| l.len() + 1).sum();
        self.status_msg = format!("line {line}/{total_lines}, col {col}, chars: {total_chars}");
    }

    // ========================================================================
    // Main event loop
    // ========================================================================

    fn run(&mut self) {
        enter_alternate_screen();
        clear_screen();

        loop {
            self.clamp_cursor();
            self.ensure_visible();
            self.render();

            let Some(key) = read_key() else {
                continue;
            };

            // If we are on the help screen, any key dismisses it.
            if self.showing_help {
                self.showing_help = false;
                continue;
            }

            match key {
                // Navigation
                Key::Up => {
                    if self.desired_col.is_none() {
                        self.desired_col = Some(self.cursor_col);
                    }
                    self.move_up();
                }
                Key::Down => {
                    if self.desired_col.is_none() {
                        self.desired_col = Some(self.cursor_col);
                    }
                    self.move_down();
                }
                Key::Left => self.move_left(),
                Key::Right => self.move_right(),
                Key::Home => self.move_home(),
                Key::End => self.move_end(),
                Key::PageUp => self.page_up(),
                Key::PageDown => self.page_down(),

                // Editing
                Key::Char(c) => self.insert_char(c),
                Key::Tab => self.insert_tab(),
                Key::Enter => self.insert_newline(),
                Key::Backspace => self.backspace(),
                Key::Delete => self.delete_char(),

                // Ctrl commands
                Key::Ctrl('o') => self.cmd_save(),
                Key::Ctrl('x') => {
                    if self.cmd_exit() {
                        break;
                    }
                }
                Key::Ctrl('k') => self.cut_line(),
                Key::Ctrl('u') => self.uncut_line(),
                Key::Ctrl('w') => self.search(),
                Key::Ctrl('\\') => self.search_and_replace(),
                Key::Ctrl('g') => {
                    self.showing_help = true;
                }
                Key::Ctrl('c') => self.cmd_show_position(),
                Key::Ctrl('_') => self.goto_line(),
                Key::Ctrl('z') => self.perform_undo(),
                Key::Ctrl('y') => self.perform_redo(),
                Key::Ctrl('l') => {
                    // Refresh: re-detect terminal size and redraw.
                    let (rows, cols) = terminal_size();
                    self.term_rows = rows;
                    self.term_cols = cols;
                    clear_screen();
                }

                // Alt commands
                Key::Alt('n') => {
                    self.show_line_numbers = !self.show_line_numbers;
                    self.status_msg = if self.show_line_numbers {
                        "Line numbers enabled".to_string()
                    } else {
                        "Line numbers disabled".to_string()
                    };
                }

                Key::Escape | Key::Unknown => {}
                _ => {}
            }

            // Clear status message on next keypress (except when just set).
            // We leave the message for the next render, then clear it.
        }

        leave_alternate_screen();
        show_cursor();
        reset_attr();
        flush();
    }
}

// ============================================================================
// Argument parsing and entry point
// ============================================================================

fn print_usage() {
    println!("Usage: nano [OPTIONS] [FILE]");
    println!();
    println!("Options:");
    println!("  -l            Show line numbers");
    println!("  -t <width>    Tab width (default {DEFAULT_TAB_WIDTH})");
    println!("  -T            Insert real tab characters (default: spaces)");
    println!("  -w            Disable word wrap display");
    println!("  -h, --help    Show this help and exit");
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut editor = Editor::new();
    let mut filename: Option<String> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_usage();
                return;
            }
            "-l" => {
                editor.show_line_numbers = true;
            }
            "-t" => {
                i += 1;
                if i < args.len() {
                    if let Ok(w) = args[i].parse::<usize>() {
                        if w > 0 && w <= 16 {
                            editor.tab_width = w;
                        }
                    }
                }
            }
            "-T" => {
                editor.tabs_to_spaces = false;
            }
            "-w" => {
                editor.word_wrap = false;
            }
            other => {
                filename = Some(other.to_string());
            }
        }
        i += 1;
    }

    // Enable raw terminal mode.
    let _raw_guard = enable_raw_mode();

    if let Some(ref path) = filename {
        editor.open_file(path);
    }

    editor.run();
}
