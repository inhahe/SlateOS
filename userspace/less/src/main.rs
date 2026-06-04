//! OurOS `less` -- Terminal Pager
//!
//! A terminal pager inspired by the classic Unix `less` utility.  Reads a file
//! (or standard input) and presents it in a scrollable, searchable display.
//!
//! # Usage
//!
//! ```text
//! less [OPTIONS] [FILE]
//!
//! Options:
//!   -N            Show line numbers
//!   -S            Chop long lines (no wrapping)
//!   -h, --help    Show this help and exit
//!   --version     Show version and exit
//!
//! Key bindings:
//!   j / Down       Scroll down one line
//!   k / Up         Scroll up one line
//!   Space / PgDn   Scroll down one page
//!   b / PgUp       Scroll up one page
//!   d              Scroll down half a page
//!   u              Scroll up half a page
//!   g              Go to the first line
//!   G              Go to the last line
//!   /pattern       Search forward
//!   ?pattern       Search backward
//!   n              Next search match
//!   N              Previous search match
//!   F              Follow mode (like tail -f)
//!   h              Help screen
//!   =              File info
//!   q              Quit
//! ```

use std::env;
use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Maximum number of lines we buffer before refusing to load more.
const MAX_BUFFERED_LINES: usize = 100_000;

/// Tab stop width.
const TAB_WIDTH: usize = 8;

/// Default terminal dimensions when /proc lookup fails.
const DEFAULT_COLS: usize = 80;
const DEFAULT_ROWS: usize = 25;

// -- VT100 escape sequences -------------------------------------------------

const ESC_CLEAR_SCREEN: &str = "\x1b[2J";
const ESC_CURSOR_HOME: &str = "\x1b[H";
const ESC_REVERSE_VIDEO: &str = "\x1b[7m";
const ESC_RESET: &str = "\x1b[0m";
const ESC_CLEAR_EOL: &str = "\x1b[K";
#[allow(dead_code)]
const ESC_BOLD: &str = "\x1b[1m";
const ESC_HIDE_CURSOR: &str = "\x1b[?25l";
const ESC_SHOW_CURSOR: &str = "\x1b[?25h";
/// Yellow background + black foreground for search-match highlighting.
const ESC_HIGHLIGHT: &str = "\x1b[43;30m";

// ============================================================================
// Minimal libc bindings for termios (raw terminal mode)
// ============================================================================

// Our POSIX compatibility layer provides tcgetattr/tcsetattr.  We define just
// enough of the termios types and constants to call them, following the same
// pattern as the nano editor in this codebase.
#[cfg(unix)]
mod libc {
    //! Minimal libc bindings for termios -- just enough for raw mode.
    //! Our POSIX layer provides the real implementation.

    pub type TcflagT = u32;
    pub type CcT = u8;
    #[allow(dead_code)]
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
    pub const ICRNL: TcflagT = 0o000400;
    pub const IXON: TcflagT = 0o002000;
    pub const BRKINT: TcflagT = 0o000002;
    pub const INPCK: TcflagT = 0o000020;
    pub const ISTRIP: TcflagT = 0o000040;

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
// Terminal helpers
// ============================================================================

/// Query terminal size from /proc or /sys, falling back to defaults.
fn terminal_size() -> (usize, usize) {
    let rows = read_proc_value("/sys/tty/rows")
        .or_else(|| read_proc_value("/proc/self/tty_rows"))
        .unwrap_or(DEFAULT_ROWS);
    let cols = read_proc_value("/sys/tty/cols")
        .or_else(|| read_proc_value("/proc/self/tty_cols"))
        .unwrap_or(DEFAULT_COLS);
    (rows, cols)
}

/// Read a single numeric value from a /proc or /sys file.
fn read_proc_value(path: &str) -> Option<usize> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Write a string to stdout without flushing.
fn write_str(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
}

/// Write a string to stdout and flush immediately.
fn write_flush(s: &str) {
    let _ = io::stdout().write_all(s.as_bytes());
    let _ = io::stdout().flush();
}

/// Move the cursor to a specific row and column (1-based).
fn cursor_to(row: usize, col: usize) {
    let mut buf = String::new();
    let _ = write!(buf, "\x1b[{};{}H", row, col);
    write_str(&buf);
}

// ============================================================================
// Raw terminal mode
// ============================================================================

/// Saved original terminal settings for restoration on exit.
#[cfg(unix)]
static mut ORIG_TERMIOS: Option<libc::termios> = None;

/// Enable raw terminal mode: disable echo and line buffering so we receive
/// each keypress individually.
///
/// On OurOS this writes to `/proc/self/tty_raw` if available, otherwise we
/// use the POSIX termios interface through the POSIX compatibility layer.
fn enable_raw_mode() {
    // Try the OurOS-specific procfs toggle first.
    if std::fs::write("/proc/self/tty_raw", "1").is_ok() {
    }
    // Fallback: use libc termios interface.
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        // SAFETY: fd 0 is stdin; termios is plain-old-data that is safe to
        // zero-initialise and pass to the kernel via tcgetattr/tcsetattr.
        unsafe {
            let mut orig: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(fd, &mut orig) != 0 {
                return;
            }
            // Save original for restoration.
            ORIG_TERMIOS = Some(orig);

            let mut raw = orig;
            // Input: no break, no CR-to-NL, no parity, no strip, no flow ctrl.
            raw.c_iflag &=
                !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
            // Output: disable post-processing.
            raw.c_oflag &= !libc::OPOST;
            // Control: 8-bit chars.
            raw.c_cflag |= libc::CS8;
            // Local: no echo, no canonical, no signals, no extended.
            raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::ISIG | libc::IEXTEN);
            // Return each byte as it arrives.
            raw.c_cc[libc::VMIN] = 1;
            raw.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(fd, libc::TCSAFLUSH, &raw);
        }
    }
}

/// Restore normal (cooked) terminal mode.
fn disable_raw_mode() {
    if std::fs::write("/proc/self/tty_raw", "0").is_ok() {
    }
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = io::stdin().as_raw_fd();
        // SAFETY: restoring saved termios is safe; we saved it in enable_raw_mode.
        unsafe {
            if let Some(ref orig) = ORIG_TERMIOS {
                libc::tcsetattr(fd, libc::TCSAFLUSH, orig);
            }
        }
    }
}

// ============================================================================
// Input reading (single keypress)
// ============================================================================

/// Key codes returned by `read_key()`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    PageUp,
    PageDown,
    Home,
    End,
    Escape,
    Unknown,
}

/// Read a single key from stdin.  Handles multi-byte escape sequences for
/// arrow keys and other special keys.
fn read_key() -> Key {
    let mut buf = [0u8; 1];
    if io::stdin().read_exact(&mut buf).is_err() {
        return Key::Unknown;
    }

    match buf[0] {
        0x1b => {
            // Escape sequence -- read the next byte to disambiguate.
            let mut seq = [0u8; 1];
            if io::stdin().read_exact(&mut seq).is_err() {
                return Key::Escape;
            }
            if seq[0] == b'[' {
                // CSI sequence.
                let mut csi = [0u8; 1];
                if io::stdin().read_exact(&mut csi).is_err() {
                    return Key::Escape;
                }
                match csi[0] {
                    b'A' => Key::Up,
                    b'B' => Key::Down,
                    b'C' => Key::Right,
                    b'D' => Key::Left,
                    b'H' => Key::Home,
                    b'F' => Key::End,
                    // Extended sequences: \x1b[5~ (PgUp), \x1b[6~ (PgDn).
                    b'5' => {
                        let mut tilde = [0u8; 1];
                        let _ = io::stdin().read_exact(&mut tilde);
                        Key::PageUp
                    }
                    b'6' => {
                        let mut tilde = [0u8; 1];
                        let _ = io::stdin().read_exact(&mut tilde);
                        Key::PageDown
                    }
                    b'1' | b'7' => {
                        // Home: \x1b[1~ or \x1b[7~
                        let mut tilde = [0u8; 1];
                        let _ = io::stdin().read_exact(&mut tilde);
                        Key::Home
                    }
                    b'4' | b'8' => {
                        // End: \x1b[4~ or \x1b[8~
                        let mut tilde = [0u8; 1];
                        let _ = io::stdin().read_exact(&mut tilde);
                        Key::End
                    }
                    _ => Key::Unknown,
                }
            } else if seq[0] == b'O' {
                // SS3 sequences (some terminals use these for Home/End).
                let mut ss3 = [0u8; 1];
                if io::stdin().read_exact(&mut ss3).is_err() {
                    return Key::Escape;
                }
                match ss3[0] {
                    b'H' => Key::Home,
                    b'F' => Key::End,
                    _ => Key::Unknown,
                }
            } else {
                Key::Escape
            }
        }
        b if b < 0x80 => Key::Char(b as char),
        _ => {
            // UTF-8 multibyte -- read remaining bytes based on leading byte.
            let width = match buf[0] {
                0xC0..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF7 => 4,
                _ => 1,
            };
            let mut utf8_buf = vec![buf[0]];
            for _ in 1..width {
                let mut cont = [0u8; 1];
                if io::stdin().read_exact(&mut cont).is_err() {
                    break;
                }
                utf8_buf.push(cont[0]);
            }
            if let Ok(s) = std::str::from_utf8(&utf8_buf) {
                if let Some(ch) = s.chars().next() {
                    Key::Char(ch)
                } else {
                    Key::Unknown
                }
            } else {
                Key::Unknown
            }
        }
    }
}

// ============================================================================
// Configuration
// ============================================================================

struct Config {
    /// Show line numbers.
    line_numbers: bool,
    /// Chop long lines instead of wrapping.
    chop_long_lines: bool,
    /// File path (None = stdin).
    file_path: Option<String>,
}

impl Config {
    fn parse_args() -> Self {
        let args: Vec<String> = env::args().collect();
        let mut config = Config {
            line_numbers: false,
            chop_long_lines: false,
            file_path: None,
        };

        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "-N" => config.line_numbers = true,
                "-S" => config.chop_long_lines = true,
                "-h" | "--help" => {
                    print_help();
                    process::exit(0);
                }
                "--version" => {
                    println!("less (OurOS) {VERSION}");
                    process::exit(0);
                }
                other => {
                    if let Some(rest) = other.strip_prefix('-') {
                        // Handle combined flags like -NS.
                        let mut recognized = true;
                        for ch in rest.chars() {
                            match ch {
                                'N' => config.line_numbers = true,
                                'S' => config.chop_long_lines = true,
                                'h' => {
                                    print_help();
                                    process::exit(0);
                                }
                                _ => {
                                    recognized = false;
                                    break;
                                }
                            }
                        }
                        if !recognized {
                            eprintln!("less: unknown option: {other}");
                            process::exit(1);
                        }
                    } else {
                        config.file_path = Some(other.to_string());
                    }
                }
            }
            i += 1;
        }
        config
    }
}

fn print_help() {
    println!(
        "\
Usage: less [OPTIONS] [FILE]

A terminal pager. If FILE is omitted, reads from standard input.

Options:
  -N            Show line numbers
  -S            Chop long lines (no wrapping)
  -h, --help    Show this help and exit
  --version     Show version and exit

Key bindings:
  j / Down       Scroll down one line
  k / Up         Scroll up one line
  Space / PgDn   Scroll down one page
  b / PgUp       Scroll up one page
  d              Scroll down half a page
  u              Scroll up half a page
  g              Go to the first line
  G              Go to the last line
  /pattern       Search forward
  ?pattern       Search backward
  n              Next search match
  N              Previous search match
  F              Follow mode (like tail -f)
  h              Help screen
  =              File info
  q              Quit"
    );
}

// ============================================================================
// Line buffer
// ============================================================================

/// A line of content from the input, storing the raw text without the trailing
/// newline.
struct Line {
    /// Raw content (may contain ANSI escape sequences), without trailing newline.
    raw: String,
}

impl Line {
    fn new(raw: String) -> Self {
        Line { raw }
    }
}

/// Compute the visible width of a string, accounting for ANSI escape sequences
/// (which have zero display width) and multi-byte characters.
fn visible_width(s: &str) -> usize {
    let mut width = 0usize;
    let mut in_escape = false;
    for ch in s.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        if ch.is_control() {
            // Don't count control characters.
        } else if is_wide_char(ch) {
            width += 2;
        } else {
            width += 1;
        }
    }
    width
}

/// Very approximate check for wide (fullwidth/CJK) characters.
fn is_wide_char(ch: char) -> bool {
    let cp = ch as u32;
    (0x1100..=0x115F).contains(&cp)
        || (0x2E80..=0x303E).contains(&cp)
        || (0x3040..=0x9FFF).contains(&cp)
        || (0xAC00..=0xD7AF).contains(&cp)
        || (0xF900..=0xFAFF).contains(&cp)
        || (0xFE10..=0xFE6F).contains(&cp)
        || (0xFF01..=0xFF60).contains(&cp)
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x20000..=0x2FA1F).contains(&cp)
}

/// Expand tab characters to spaces based on the current column position.
fn expand_tabs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    let mut in_escape = false;

    for ch in s.chars() {
        if in_escape {
            out.push(ch);
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            out.push(ch);
            continue;
        }
        if ch == '\t' {
            let spaces = TAB_WIDTH - (col % TAB_WIDTH);
            for _ in 0..spaces {
                out.push(' ');
            }
            col += spaces;
        } else {
            out.push(ch);
            if !ch.is_control() {
                col += if is_wide_char(ch) { 2 } else { 1 };
            }
        }
    }
    out
}

/// Strip a trailing `\r` if present (handle CRLF input).
fn strip_cr(s: &str) -> &str {
    s.strip_suffix('\r').unwrap_or(s)
}

// ============================================================================
// Search
// ============================================================================

#[derive(Clone)]
enum SearchDirection {
    Forward,
    Backward,
}

/// Simple substring search state.
#[derive(Clone)]
struct Search {
    pattern: String,
    direction: SearchDirection,
}

/// Find all non-overlapping match positions in `haystack` for `pattern`.
/// Matching is case-insensitive and ignores ANSI escape sequences.
/// Returns (start_char_idx, end_char_idx) pairs in terms of the original
/// string's character positions.
fn find_matches(haystack: &str, pattern: &str) -> Vec<(usize, usize)> {
    if pattern.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::new();
    // Strip ANSI for matching but map positions back to the original string.
    let (stripped, char_map) = strip_ansi_with_map(haystack);
    let stripped_lower = stripped.to_lowercase();
    let pat_lower = pattern.to_lowercase();
    let pat_chars: Vec<char> = pat_lower.chars().collect();
    let stripped_chars: Vec<char> = stripped_lower.chars().collect();

    if pat_chars.is_empty() || stripped_chars.is_empty() {
        return result;
    }

    let haystack_char_count = haystack.chars().count();
    let mut i = 0;
    while i + pat_chars.len() <= stripped_chars.len() {
        if stripped_chars[i..i + pat_chars.len()] == pat_chars[..] {
            let orig_start = if i < char_map.len() { char_map[i] } else { i };
            let end_stripped = i + pat_chars.len();
            let orig_end = if end_stripped < char_map.len() {
                char_map[end_stripped]
            } else {
                haystack_char_count
            };
            result.push((orig_start, orig_end));
            i += pat_chars.len(); // non-overlapping
        } else {
            i += 1;
        }
    }
    result
}

/// Strip ANSI escape sequences from a string, returning the stripped string
/// and a mapping from stripped-char-index to original-char-index.
fn strip_ansi_with_map(s: &str) -> (String, Vec<usize>) {
    let mut stripped = String::new();
    let mut char_map: Vec<usize> = Vec::new();
    let mut in_escape = false;
    for (orig_idx, ch) in s.chars().enumerate() {
        if in_escape {
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            continue;
        }
        stripped.push(ch);
        char_map.push(orig_idx);
    }
    (stripped, char_map)
}

// ============================================================================
// Display-line wrapping
// ============================================================================

/// A display row: a slice of an original line to show on one terminal row.
struct DisplayRow {
    /// Index into the `lines` buffer (original line number).
    line_idx: usize,
    /// The rendered text for this terminal row (already truncated/wrapped).
    text: String,
}

/// Produce display rows for a single buffered line, performing either wrapping
/// or chopping.  `line_num_width` is the width reserved for line numbers (0 if
/// line numbers are disabled).
fn wrap_line(
    line: &Line,
    line_idx: usize,
    term_cols: usize,
    chop: bool,
    line_num_width: usize,
) -> Vec<DisplayRow> {
    let prefix_width = if line_num_width > 0 {
        line_num_width + 1 // number + space separator
    } else {
        0
    };
    let content_cols = if term_cols > prefix_width {
        term_cols - prefix_width
    } else {
        1
    };

    let expanded = expand_tabs(&line.raw);

    if chop {
        let truncated = truncate_to_width(&expanded, content_cols);
        return vec![DisplayRow {
            line_idx,
            text: truncated,
        }];
    }

    // Wrapping mode: split into multiple rows of content_cols width.
    let rows = wrap_to_width(&expanded, content_cols);
    if rows.is_empty() {
        return vec![DisplayRow {
            line_idx,
            text: String::new(),
        }];
    }
    rows.into_iter()
        .map(|text| DisplayRow { line_idx, text })
        .collect()
}

/// Truncate a string (which may contain ANSI escapes) to at most `max_width`
/// visible characters.  ANSI sequences pass through without counting.
fn truncate_to_width(s: &str, max_width: usize) -> String {
    let mut out = String::new();
    let mut width = 0usize;
    let mut in_escape = false;

    for ch in s.chars() {
        if in_escape {
            out.push(ch);
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            out.push(ch);
            continue;
        }
        let ch_width = if ch.is_control() {
            0
        } else if is_wide_char(ch) {
            2
        } else {
            1
        };
        if width + ch_width > max_width {
            break;
        }
        out.push(ch);
        width += ch_width;
    }
    out
}

/// Wrap a string into multiple chunks of at most `max_width` visible chars.
fn wrap_to_width(s: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![s.to_string()];
    }

    let mut rows: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut width = 0usize;
    let mut in_escape = false;

    for ch in s.chars() {
        if in_escape {
            current.push(ch);
            if ch.is_ascii_alphabetic() || ch == 'm' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\x1b' {
            in_escape = true;
            current.push(ch);
            continue;
        }
        let ch_width = if ch.is_control() {
            0
        } else if is_wide_char(ch) {
            2
        } else {
            1
        };
        if width + ch_width > max_width && width > 0 {
            rows.push(current);
            current = String::new();
            width = 0;
        }
        current.push(ch);
        width += ch_width;
    }
    rows.push(current);
    rows
}

// ============================================================================
// Highlight search matches in a display row
// ============================================================================

/// Insert ANSI highlight escapes around search matches in the given text.
fn highlight_matches(text: &str, pattern: &str) -> String {
    if pattern.is_empty() {
        return text.to_string();
    }
    let matches = find_matches(text, pattern);
    if matches.is_empty() {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() + matches.len() * 20);
    let mut i = 0usize;
    let mut match_idx = 0usize;

    while i < chars.len() {
        if match_idx < matches.len() && i == matches[match_idx].0 {
            let (start, end) = matches[match_idx];
            out.push_str(ESC_HIGHLIGHT);
            let slice_end = end.min(chars.len());
            for ch in chars.iter().take(slice_end).skip(start) {
                out.push(*ch);
            }
            out.push_str(ESC_RESET);
            i = end;
            match_idx += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

// ============================================================================
// Pager state
// ============================================================================

struct Pager {
    /// All buffered lines.
    lines: Vec<Line>,
    /// First visible display-row index.
    scroll_offset: usize,
    /// Terminal dimensions.
    term_rows: usize,
    term_cols: usize,
    /// Number of content rows (term_rows - 1 for the status bar).
    content_rows: usize,
    /// Configuration.
    config: Config,
    /// Current search state.
    search: Option<Search>,
    /// File name for display.
    file_name: String,
    /// Whether we have finished reading input (EOF reached).
    eof_reached: bool,
    /// Buffered reader for the input source.
    reader: Option<BufReader<Box<dyn Read>>>,
    /// Follow mode active.
    follow_mode: bool,
    /// Message to display on the status line (overrides default).
    status_message: Option<String>,
}

impl Pager {
    fn new(config: Config) -> io::Result<Self> {
        let (term_rows, term_cols) = terminal_size();
        let content_rows = if term_rows > 1 { term_rows - 1 } else { 1 };

        let (reader, file_name): (Box<dyn Read>, String) =
            if let Some(ref path) = config.file_path {
                let f = File::open(path)
                    .map_err(|e| io::Error::new(e.kind(), format!("{path}: {e}")))?;
                (Box::new(f), path.clone())
            } else {
                // Reading from stdin.  Slurp everything first so that keyboard
                // input can come from the tty afterwards.
                let stdin_data = read_stdin_fully()?;
                (
                    Box::new(io::Cursor::new(stdin_data)),
                    "(standard input)".to_string(),
                )
            };

        let mut pager = Pager {
            lines: Vec::new(),
            scroll_offset: 0,
            term_rows,
            term_cols,
            content_rows,
            config,
            search: None,
            file_name,
            eof_reached: false,
            reader: Some(BufReader::new(reader)),
            follow_mode: false,
            status_message: None,
        };

        // Load initial lines to fill the screen plus a buffer.
        pager.load_lines(content_rows + 256);
        Ok(pager)
    }

    /// Load up to `count` more lines from the reader.  Returns the number of
    /// new lines actually added.
    fn load_lines(&mut self, count: usize) -> usize {
        if self.eof_reached || self.lines.len() >= MAX_BUFFERED_LINES {
            return 0;
        }
        let reader = match self.reader.as_mut() {
            Some(r) => r,
            None => return 0,
        };

        let mut added = 0usize;
        let mut line_buf = String::new();
        for _ in 0..count {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => {
                    self.eof_reached = true;
                    break;
                }
                Ok(_) => {
                    let content = strip_cr(line_buf.trim_end_matches('\n'));
                    self.lines.push(Line::new(content.to_string()));
                    added += 1;
                    if self.lines.len() >= MAX_BUFFERED_LINES {
                        self.eof_reached = true;
                        break;
                    }
                }
                Err(_) => {
                    self.eof_reached = true;
                    break;
                }
            }
        }
        added
    }

    /// Ensure we have loaded enough lines to display up to the given display-
    /// row offset.
    fn ensure_loaded_for_offset(&mut self, target_offset: usize) {
        let needed = target_offset + self.content_rows + 64;
        if self.lines.len() < needed && !self.eof_reached {
            let deficit = needed - self.lines.len() + 256;
            self.load_lines(deficit);
        }
    }

    /// Compute the line-number column width (0 when disabled).
    fn line_num_width(&self) -> usize {
        if self.config.line_numbers {
            let max_num = self.lines.len();
            if max_num == 0 { 1 } else { digit_count(max_num) }
        } else {
            0
        }
    }

    /// Build display rows for the currently visible region.  Returns
    /// (total_display_rows, visible_rows).
    fn build_visible_rows(&self) -> (usize, Vec<DisplayRow>) {
        let lnw = self.line_num_width();

        let mut all_rows: Vec<DisplayRow> = Vec::new();
        for (idx, line) in self.lines.iter().enumerate() {
            let rows = wrap_line(
                line,
                idx,
                self.term_cols,
                self.config.chop_long_lines,
                lnw,
            );
            all_rows.extend(rows);
        }

        let total = all_rows.len();
        let start = self.scroll_offset.min(total.saturating_sub(1));
        let end = (start + self.content_rows).min(total);
        let visible: Vec<DisplayRow> =
            all_rows.into_iter().skip(start).take(end - start).collect();
        (total, visible)
    }

    /// Count total display rows without collecting them.
    fn total_display_rows(&self) -> usize {
        let lnw = self.line_num_width();
        let mut total = 0usize;
        for line in &self.lines {
            total += wrap_line(line, 0, self.term_cols, self.config.chop_long_lines, lnw)
                .len();
        }
        total
    }

    /// Render the visible portion of the file to the terminal.
    fn render(&mut self) {
        // Re-query terminal size in case of resize.
        let (term_rows, term_cols) = terminal_size();
        if term_rows != self.term_rows || term_cols != self.term_cols {
            self.term_rows = term_rows;
            self.term_cols = term_cols;
            self.content_rows = if term_rows > 1 { term_rows - 1 } else { 1 };
        }

        self.ensure_loaded_for_offset(self.scroll_offset);

        let lnw = self.line_num_width();
        let (total_display_rows, visible) = self.build_visible_rows();
        let search_pattern = self.search.as_ref().map(|s| s.pattern.clone());

        // Hide cursor, move home.
        write_str(ESC_HIDE_CURSOR);
        write_str(ESC_CURSOR_HOME);

        let mut output = String::with_capacity(self.term_cols * self.term_rows * 2);

        for row_num in 0..self.content_rows {
            if row_num < visible.len() {
                let drow = &visible[row_num];
                // Line number prefix.
                if lnw > 0 {
                    let is_first = if row_num == 0 {
                        true
                    } else {
                        visible[row_num - 1].line_idx != drow.line_idx
                    };
                    if is_first {
                        let _ = write!(
                            output,
                            "\x1b[33m{:>width$}\x1b[0m ",
                            drow.line_idx + 1,
                            width = lnw
                        );
                    } else {
                        for _ in 0..lnw {
                            output.push(' ');
                        }
                        output.push(' ');
                    }
                }
                // Content, with optional search highlighting.
                let content = if let Some(ref pat) = search_pattern {
                    highlight_matches(&drow.text, pat)
                } else {
                    drow.text.clone()
                };
                output.push_str(&content);
            }
            output.push_str(ESC_CLEAR_EOL);
            if row_num + 1 < self.term_rows {
                output.push('\n');
            }
        }

        // Status bar at the bottom row.
        let status = self.build_status_line(total_display_rows);
        output.push_str(ESC_REVERSE_VIDEO);
        output.push_str(&status);
        let status_vis_width = visible_width(&status);
        if status_vis_width < self.term_cols {
            for _ in 0..(self.term_cols - status_vis_width) {
                output.push(' ');
            }
        }
        output.push_str(ESC_RESET);

        output.push_str(ESC_SHOW_CURSOR);
        write_flush(&output);
    }

    /// Build the status/prompt line text.
    fn build_status_line(&self, total_display_rows: usize) -> String {
        if let Some(ref msg) = self.status_message {
            return msg.clone();
        }

        let mut status = String::new();
        let _ = write!(status, " {}", self.file_name);

        if total_display_rows > 0 {
            let first_visible = self.scroll_offset + 1;
            let last_visible =
                (self.scroll_offset + self.content_rows).min(total_display_rows);
            let _ = write!(
                status,
                "  lines {}-{} of {}",
                first_visible, last_visible, total_display_rows
            );
        } else {
            status.push_str("  (empty)");
        }

        if self.eof_reached
            && self.scroll_offset + self.content_rows >= total_display_rows
        {
            status.push_str("  (END)");
        }

        if self.follow_mode {
            status.push_str("  [FOLLOW]");
        }

        if let Some(ref search) = self.search {
            let dir_char = match search.direction {
                SearchDirection::Forward => '/',
                SearchDirection::Backward => '?',
            };
            let _ = write!(status, "  {}{}", dir_char, search.pattern);
        }

        status
    }

    /// Scroll down by `n` display rows.
    fn scroll_down(&mut self, n: usize) {
        self.ensure_loaded_for_offset(self.scroll_offset + n + self.content_rows);
        let total = self.total_display_rows();
        let max_offset = total.saturating_sub(self.content_rows);
        self.scroll_offset = (self.scroll_offset + n).min(max_offset);
    }

    /// Scroll up by `n` display rows.
    fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Jump to the top.
    fn go_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Jump to the bottom.
    fn go_bottom(&mut self) {
        while !self.eof_reached {
            self.load_lines(4096);
        }
        let total = self.total_display_rows();
        self.scroll_offset = total.saturating_sub(self.content_rows);
    }

    /// Search forward from the current scroll position for the next match.
    /// Returns true if a match was found and we scrolled to it.
    fn search_forward(&mut self, pattern: &str) -> bool {
        if pattern.is_empty() {
            return false;
        }
        // Load additional content to search through.
        self.ensure_loaded_for_offset(self.scroll_offset + 10000);

        let lnw = self.line_num_width();

        // We need to build display rows and scan them, but we cannot borrow
        // `self.lines` while also calling `self.load_lines()`.  So we first
        // build display rows from what we have, and only load more after.
        let mut row_idx = 0usize;
        let line_count = self.lines.len();
        for idx in 0..line_count {
            let rows = wrap_line(
                &self.lines[idx],
                idx,
                self.term_cols,
                self.config.chop_long_lines,
                lnw,
            );
            for drow in &rows {
                if row_idx > self.scroll_offset
                    && !find_matches(&drow.text, pattern).is_empty()
                {
                    self.scroll_offset = row_idx;
                    return true;
                }
                row_idx += 1;
            }
        }

        // If we haven't found a match yet, try loading more and searching.
        while !self.eof_reached {
            let before = self.lines.len();
            self.load_lines(4096);
            let after = self.lines.len();
            if after == before {
                break;
            }
            for idx in before..after {
                let rows = wrap_line(
                    &self.lines[idx],
                    idx,
                    self.term_cols,
                    self.config.chop_long_lines,
                    lnw,
                );
                for drow in &rows {
                    if row_idx > self.scroll_offset
                        && !find_matches(&drow.text, pattern).is_empty()
                    {
                        self.scroll_offset = row_idx;
                        return true;
                    }
                    row_idx += 1;
                }
            }
        }
        false
    }

    /// Search backward from the current scroll position for the previous match.
    fn search_backward(&mut self, pattern: &str) -> bool {
        if pattern.is_empty() {
            return false;
        }

        let lnw = self.line_num_width();

        // Build all display rows up to the current offset and find the last
        // matching row before the current scroll position.
        let mut best_match: Option<usize> = None;
        let mut row_idx = 0usize;
        for idx in 0..self.lines.len() {
            let rows = wrap_line(
                &self.lines[idx],
                idx,
                self.term_cols,
                self.config.chop_long_lines,
                lnw,
            );
            for drow in &rows {
                if row_idx < self.scroll_offset
                    && !find_matches(&drow.text, pattern).is_empty()
                {
                    best_match = Some(row_idx);
                }
                row_idx += 1;
            }
        }

        if let Some(ridx) = best_match {
            self.scroll_offset = ridx;
            true
        } else {
            false
        }
    }

    /// Prompt the user for a search pattern (displayed on the status line).
    fn prompt_search(&mut self, direction: SearchDirection) -> Option<String> {
        let prompt_char = match direction {
            SearchDirection::Forward => '/',
            SearchDirection::Backward => '?',
        };

        cursor_to(self.term_rows, 1);
        let prompt = format!("{ESC_REVERSE_VIDEO}{prompt_char}{ESC_RESET}{ESC_CLEAR_EOL}");
        write_flush(&prompt);

        let mut pattern = String::new();
        loop {
            let key = read_key();
            match key {
                Key::Char('\n') | Key::Char('\r') => {
                    if pattern.is_empty() {
                        return self.search.as_ref().map(|s| s.pattern.clone());
                    }
                    return Some(pattern);
                }
                Key::Escape => return None,
                Key::Char('\x7f') | Key::Char('\x08') => {
                    pattern.pop();
                }
                Key::Char(ch) if !ch.is_control() => {
                    pattern.push(ch);
                }
                _ => {}
            }
            cursor_to(self.term_rows, 1);
            let line = format!(
                "{ESC_REVERSE_VIDEO}{prompt_char}{ESC_RESET}{pattern}{ESC_CLEAR_EOL}"
            );
            write_flush(&line);
        }
    }

    /// Show the help screen.  Returns when the user presses q or h.
    fn show_help(&self) {
        write_str(ESC_CLEAR_SCREEN);
        write_str(ESC_CURSOR_HOME);

        let help = "\
\x1b[1mless - OurOS Terminal Pager - Help\x1b[0m

  NAVIGATION
    j, Down          Scroll down one line
    k, Up            Scroll up one line
    Space, PgDn      Scroll down one page
    b, PgUp          Scroll up one page
    d                Scroll down half a page
    u                Scroll up half a page
    g, Home          Go to beginning of file
    G, End           Go to end of file

  SEARCH
    /pattern         Search forward for pattern
    ?pattern         Search backward for pattern
    n                Repeat search in same direction
    N                Repeat search in opposite direction

  OTHER
    F                Follow mode (like tail -f)
    =                Display file information
    h                This help screen
    q                Quit

  OPTIONS
    -N               Show line numbers
    -S               Chop (truncate) long lines

  Press q or h to return to the file.
";
        write_flush(help);

        loop {
            let key = read_key();
            match key {
                Key::Char('q') | Key::Char('h') | Key::Escape => return,
                _ => {}
            }
        }
    }

    /// Show file info on the status line.
    fn show_file_info(&mut self) {
        while !self.eof_reached {
            self.load_lines(4096);
        }
        let total_lines = self.lines.len();
        let total_bytes: usize = self.lines.iter().map(|l| l.raw.len() + 1).sum();
        let info = format!(
            " {} : {} lines, ~{} bytes",
            self.file_name, total_lines, total_bytes
        );
        self.status_message = Some(info);
    }

    /// Follow mode: jump to end and wait for new data.
    fn enter_follow_mode(&mut self) {
        self.follow_mode = true;
        self.status_message =
            Some(" Waiting for data... (press q or F to stop)".to_string());
        self.go_bottom();
        self.render();

        loop {
            // Try to load more data if we previously hit EOF on a file.
            if self.eof_reached
                && let Some(ref path) = self.config.file_path.clone()
                    && let Ok(meta) = std::fs::metadata(path) {
                        let current_byte_count: usize =
                            self.lines.iter().map(|l| l.raw.len() + 1).sum();
                        if meta.len() as usize > current_byte_count
                            && let Ok(f) = File::open(path) {
                                let mut r =
                                    BufReader::new(Box::new(f) as Box<dyn Read>);
                                let mut skip_buf = String::new();
                                for _ in 0..self.lines.len() {
                                    if r.read_line(&mut skip_buf).unwrap_or(0) == 0 {
                                        break;
                                    }
                                    skip_buf.clear();
                                }
                                self.reader = Some(r);
                                self.eof_reached = false;
                            }
                    }

            let added = self.load_lines(256);
            if added > 0 {
                self.go_bottom();
                self.status_message =
                    Some(" Waiting for data... (press q or F to stop)".to_string());
                self.render();
            }

            std::thread::sleep(std::time::Duration::from_millis(500));

            // Check for a keypress to exit follow mode.
            if check_key_available() {
                let key = read_key();
                match key {
                    Key::Char('q') | Key::Char('F') | Key::Escape => {
                        self.follow_mode = false;
                        self.status_message = None;
                        return;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Main event loop.
    fn run(&mut self) {
        enable_raw_mode();
        self.render();

        loop {
            let key = read_key();

            // Clear any one-shot status message.
            self.status_message = None;

            match key {
                Key::Char('q') | Key::Char('Q') => break,

                Key::Char('j') | Key::Char('e') | Key::Down => {
                    self.scroll_down(1);
                }

                Key::Char('k') | Key::Char('y') | Key::Up => {
                    self.scroll_up(1);
                }

                Key::Char(' ') | Key::Char('f') | Key::PageDown => {
                    self.scroll_down(self.content_rows);
                }

                Key::Char('b') | Key::PageUp => {
                    self.scroll_up(self.content_rows);
                }

                Key::Char('d') => {
                    self.scroll_down(self.content_rows / 2);
                }

                Key::Char('u') => {
                    self.scroll_up(self.content_rows / 2);
                }

                Key::Char('g') | Key::Home => {
                    self.go_top();
                }

                Key::Char('G') | Key::End => {
                    self.go_bottom();
                }

                Key::Char('/') => {
                    if let Some(pattern) = self.prompt_search(SearchDirection::Forward) {
                        let found = self.search_forward(&pattern);
                        self.search = Some(Search {
                            pattern: pattern.clone(),
                            direction: SearchDirection::Forward,
                        });
                        if !found {
                            self.status_message =
                                Some(format!(" Pattern not found: {pattern}"));
                        }
                    }
                }

                Key::Char('?') => {
                    if let Some(pattern) =
                        self.prompt_search(SearchDirection::Backward)
                    {
                        let found = self.search_backward(&pattern);
                        self.search = Some(Search {
                            pattern: pattern.clone(),
                            direction: SearchDirection::Backward,
                        });
                        if !found {
                            self.status_message =
                                Some(format!(" Pattern not found: {pattern}"));
                        }
                    }
                }

                Key::Char('n') => {
                    let search_clone = self.search.clone();
                    if let Some(search) = search_clone {
                        let found = match search.direction {
                            SearchDirection::Forward => {
                                self.search_forward(&search.pattern)
                            }
                            SearchDirection::Backward => {
                                self.search_backward(&search.pattern)
                            }
                        };
                        if !found {
                            self.status_message = Some(format!(
                                " Pattern not found: {}",
                                search.pattern
                            ));
                        }
                    }
                }

                Key::Char('N') => {
                    let search_clone = self.search.clone();
                    if let Some(search) = search_clone {
                        let found = match search.direction {
                            SearchDirection::Forward => {
                                self.search_backward(&search.pattern)
                            }
                            SearchDirection::Backward => {
                                self.search_forward(&search.pattern)
                            }
                        };
                        if !found {
                            self.status_message = Some(format!(
                                " Pattern not found: {}",
                                search.pattern
                            ));
                        }
                    }
                }

                Key::Char('F') => {
                    self.enter_follow_mode();
                }

                Key::Char('h') => {
                    self.show_help();
                }

                Key::Char('=') => {
                    self.show_file_info();
                }

                Key::Char('\r') | Key::Char('\n') => {
                    self.scroll_down(1);
                }

                _ => {}
            }

            self.render();
        }

        // Cleanup: restore terminal and show cursor.
        disable_raw_mode();
        write_str(ESC_SHOW_CURSOR);
        write_flush(ESC_CLEAR_SCREEN);
    }
}

// ============================================================================
// Utilities
// ============================================================================

/// Count the number of decimal digits in `n`.
fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut digits = 0;
    let mut val = n;
    while val > 0 {
        digits += 1;
        val /= 10;
    }
    digits
}

/// Read all of stdin into a byte buffer (used when piping input).
fn read_stdin_fully() -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    io::stdin().read_to_end(&mut buf)?;
    Ok(buf)
}

/// Check if a key is available on stdin without blocking.
/// Uses the OurOS `/proc/self/tty_avail` interface or returns false.
fn check_key_available() -> bool {
    if let Ok(val) = std::fs::read_to_string("/proc/self/tty_avail")
        && let Ok(n) = val.trim().parse::<usize>() {
            return n > 0;
        }
    false
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let config = Config::parse_args();

    let mut pager = match Pager::new(config) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("less: {e}");
            process::exit(1);
        }
    };

    pager.run();
}
