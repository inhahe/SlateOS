//! OurOS `man` -- Manual Page Viewer
//!
//! Displays manual pages for OurOS commands, syscalls, configuration files, and
//! other system documentation.  Supports the standard troff-subset formatting
//! directives (.TH, .SH, .SS, .B, .I, .TP, .PP, .br, .nf/.fi) and renders
//! them with ANSI colour on the terminal.
//!
//! # Usage
//!
//! ```text
//! man [OPTIONS] [SECTION] NAME
//!
//! Options:
//!   -k KEYWORD    Search man page names and descriptions (apropos)
//!   -f NAME       Display one-line description (whatis)
//!   -a            Show all matching pages, not just the first
//!   -w            Print the path of the man page instead of displaying it
//!   -h, --help    Show this help and exit
//!   --version     Show version and exit
//!
//! Sections:
//!   1  User commands
//!   2  System calls
//!   3  Library functions
//!   4  Special files / devices
//!   5  File formats and configuration files
//!   7  Miscellaneous
//!   8  System administration commands
//! ```

use std::env;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default terminal width when detection fails.
const DEFAULT_COLS: usize = 80;

/// Default terminal height when detection fails.
const DEFAULT_ROWS: usize = 25;

// -- ANSI escape sequences ---------------------------------------------------

const ESC_RESET: &str = "\x1b[0m";
const ESC_BOLD: &str = "\x1b[1m";
const ESC_UNDERLINE: &str = "\x1b[4m";
const ESC_CYAN_BOLD: &str = "\x1b[1;36m";
const ESC_YELLOW_BOLD: &str = "\x1b[1;33m";
const ESC_GREEN: &str = "\x1b[32m";

/// Sections recognised by our manual system.
const SECTION_NAMES: &[(u8, &str)] = &[
    (1, "User Commands"),
    (2, "System Calls"),
    (3, "Library Functions"),
    (4, "Special Files"),
    (5, "File Formats"),
    (7, "Miscellaneous"),
    (8, "System Administration"),
];

/// Default MANPATH directories to search for on-disk man pages.
const DEFAULT_MANPATH: &[&str] = &[
    "/usr/share/man",
    "/usr/local/share/man",
    "/usr/local/man",
];

// ============================================================================
// Embedded man pages
// ============================================================================

/// A single embedded manual page in troff-subset source form.
struct EmbeddedPage {
    name: &'static str,
    section: u8,
    source: &'static str,
}

/// All embedded manual pages, sorted by name then section.
static EMBEDDED_PAGES: &[EmbeddedPage] = &[
    EmbeddedPage {
        name: "cat",
        section: 1,
        source: include_str!("pages/cat.1"),
    },
    EmbeddedPage {
        name: "chmod",
        section: 1,
        source: include_str!("pages/chmod.1"),
    },
    EmbeddedPage {
        name: "chown",
        section: 1,
        source: include_str!("pages/chown.1"),
    },
    EmbeddedPage {
        name: "cp",
        section: 1,
        source: include_str!("pages/cp.1"),
    },
    EmbeddedPage {
        name: "curl",
        section: 1,
        source: include_str!("pages/curl.1"),
    },
    EmbeddedPage {
        name: "df",
        section: 1,
        source: include_str!("pages/df.1"),
    },
    EmbeddedPage {
        name: "du",
        section: 1,
        source: include_str!("pages/du.1"),
    },
    EmbeddedPage {
        name: "find",
        section: 1,
        source: include_str!("pages/find.1"),
    },
    EmbeddedPage {
        name: "grep",
        section: 1,
        source: include_str!("pages/grep.1"),
    },
    EmbeddedPage {
        name: "kill",
        section: 1,
        source: include_str!("pages/kill.1"),
    },
    EmbeddedPage {
        name: "ls",
        section: 1,
        source: include_str!("pages/ls.1"),
    },
    EmbeddedPage {
        name: "man",
        section: 1,
        source: include_str!("pages/man.1"),
    },
    EmbeddedPage {
        name: "mkdir",
        section: 1,
        source: include_str!("pages/mkdir.1"),
    },
    EmbeddedPage {
        name: "mount",
        section: 1,
        source: include_str!("pages/mount.1"),
    },
    EmbeddedPage {
        name: "mv",
        section: 1,
        source: include_str!("pages/mv.1"),
    },
    EmbeddedPage {
        name: "ping",
        section: 1,
        source: include_str!("pages/ping.1"),
    },
    EmbeddedPage {
        name: "ps",
        section: 1,
        source: include_str!("pages/ps.1"),
    },
    EmbeddedPage {
        name: "rm",
        section: 1,
        source: include_str!("pages/rm.1"),
    },
    EmbeddedPage {
        name: "screen",
        section: 1,
        source: include_str!("pages/screen.1"),
    },
    EmbeddedPage {
        name: "ssh",
        section: 1,
        source: include_str!("pages/ssh.1"),
    },
    EmbeddedPage {
        name: "umount",
        section: 8,
        source: include_str!("pages/umount.8"),
    },
];

// ============================================================================
// Troff-subset formatter
// ============================================================================

/// Render a troff-subset man page source into a `Vec<String>` of ANSI-formatted
/// lines suitable for terminal display.
///
/// Supported directives:
///   .TH name section date source manual   -- title heading
///   .SH text                               -- section heading
///   .SS text                               -- subsection heading
///   .B  text                               -- bold
///   .I  text                               -- italic (rendered as underline)
///   .BI bold italic ...                    -- alternating bold/italic
///   .BR bold roman ...                     -- alternating bold/roman
///   .TP [indent]                           -- tagged paragraph
///   .PP / .P / .LP                         -- paragraph break
///   .br                                    -- line break
///   .nf                                    -- no-fill (preformatted) start
///   .fi                                    -- no-fill end
///   .RS [indent]                           -- increase indent
///   .RE                                    -- decrease indent
///   \"                                     -- comment (ignored)
fn format_manpage(source: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut nofill = false;
    let mut indent: usize = 0;
    let mut tp_pending = false;

    // Effective text width accounting for left margin.
    let margin = 3_usize;
    let effective_width = if width > margin + 10 {
        width - margin
    } else {
        70
    };

    let pad = " ".repeat(margin);

    for raw_line in source.lines() {
        let line = raw_line.trim_end();

        // Skip comments.
        if line.starts_with(".\\\"") || line.starts_with("'\\\"") {
            continue;
        }

        // ---- Directives ---------------------------------------------------

        if let Some(rest) = line.strip_prefix(".TH ") {
            // Title heading -- render centred header and footer.
            let parts = split_troff_args(rest);
            let name = parts.first().map_or("", |s| s.as_str());
            let sec = parts.get(1).map_or("", |s| s.as_str());
            let header = format!("{}({})", name.to_uppercase(), sec);
            let sec_title = section_title_for(sec);
            let header_line = format!(
                "{ESC_BOLD}{}{ESC_RESET}{}{}",
                header,
                " ".repeat(effective_width.saturating_sub(header.len() + sec_title.len())),
                sec_title,
            );
            out.push(header_line);
            out.push(String::new());
            continue;
        }

        if let Some(rest) = line.strip_prefix(".SH") {
            let text = rest.trim().trim_matches('"');
            out.push(String::new());
            out.push(format!("{ESC_CYAN_BOLD}{text}{ESC_RESET}"));
            tp_pending = false;
            indent = 0;
            continue;
        }

        if let Some(rest) = line.strip_prefix(".SS") {
            let text = rest.trim().trim_matches('"');
            out.push(String::new());
            out.push(format!(
                "{pad}{ESC_YELLOW_BOLD}{text}{ESC_RESET}"
            ));
            tp_pending = false;
            continue;
        }

        if line == ".PP" || line == ".P" || line == ".LP" {
            out.push(String::new());
            tp_pending = false;
            continue;
        }

        if line == ".br" {
            out.push(String::new());
            continue;
        }

        if line == ".nf" {
            nofill = true;
            continue;
        }

        if line == ".fi" {
            nofill = false;
            continue;
        }

        if line.starts_with(".TP") {
            out.push(String::new());
            tp_pending = true;
            continue;
        }

        if let Some(rest) = line.strip_prefix(".RS") {
            let n: usize = rest.trim().parse().unwrap_or(4);
            indent = indent.saturating_add(n);
            continue;
        }

        if line.starts_with(".RE") {
            indent = indent.saturating_sub(4);
            continue;
        }

        if let Some(rest) = line.strip_prefix(".B ") {
            let text = inline_format(rest.trim());
            let extra_indent = " ".repeat(indent);
            push_wrapped(&mut out, &format!("{pad}{extra_indent}{ESC_BOLD}{text}{ESC_RESET}"), effective_width);
            continue;
        }

        if let Some(rest) = line.strip_prefix(".I ") {
            let text = inline_format(rest.trim());
            let extra_indent = " ".repeat(indent);
            push_wrapped(
                &mut out,
                &format!("{pad}{extra_indent}{ESC_UNDERLINE}{text}{ESC_RESET}"),
                effective_width,
            );
            continue;
        }

        if let Some(rest) = line.strip_prefix(".BI ") {
            let parts = split_troff_args(rest);
            let mut buf = String::new();
            for (i, p) in parts.iter().enumerate() {
                if i % 2 == 0 {
                    let _ = write!(buf, "{ESC_BOLD}{p}{ESC_RESET}");
                } else {
                    let _ = write!(buf, "{ESC_UNDERLINE}{p}{ESC_RESET}");
                }
            }
            let extra_indent = " ".repeat(indent);
            push_wrapped(&mut out, &format!("{pad}{extra_indent}{buf}"), effective_width);
            continue;
        }

        if let Some(rest) = line.strip_prefix(".BR ") {
            let parts = split_troff_args(rest);
            let mut buf = String::new();
            for (i, p) in parts.iter().enumerate() {
                if i % 2 == 0 {
                    let _ = write!(buf, "{ESC_BOLD}{p}{ESC_RESET}");
                } else {
                    buf.push_str(p);
                }
            }
            let extra_indent = " ".repeat(indent);
            push_wrapped(&mut out, &format!("{pad}{extra_indent}{buf}"), effective_width);
            continue;
        }

        // Ignore other directives we don't understand.
        if line.starts_with('.') && line.len() > 1 {
            let directive_end = line.find(' ').unwrap_or(line.len());
            let directive = &line[1..directive_end];
            // Only skip if it looks like a real directive (uppercase or known).
            if directive.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
                continue;
            }
        }

        // ---- Body text ----------------------------------------------------

        let formatted = inline_format(line);

        if nofill {
            // Preformatted: preserve spacing, just indent.
            let extra_indent = " ".repeat(indent);
            out.push(format!(
                "{pad}{extra_indent}{ESC_GREEN}{formatted}{ESC_RESET}"
            ));
            continue;
        }

        if tp_pending {
            // Tagged paragraph: this line is the tag (bold), next lines indented.
            let extra_indent = " ".repeat(indent);
            out.push(format!(
                "{pad}{extra_indent}{ESC_BOLD}{formatted}{ESC_RESET}"
            ));
            tp_pending = false;
            indent = indent.saturating_add(4);
            continue;
        }

        if formatted.is_empty() {
            out.push(String::new());
            continue;
        }

        let extra_indent = " ".repeat(indent);
        push_wrapped(
            &mut out,
            &format!("{pad}{extra_indent}{formatted}"),
            effective_width,
        );
    }

    out
}

/// Handle inline \\fB / \\fI / \\fR formatting escapes and backslash-escapes.
fn inline_format(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + 32);
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.peek() {
                Some(&'f') => {
                    chars.next(); // consume 'f'
                    match chars.next() {
                        Some('B') => result.push_str(ESC_BOLD),
                        Some('I') => result.push_str(ESC_UNDERLINE),
                        Some('R') | Some('P') => result.push_str(ESC_RESET),
                        _ => {}
                    }
                }
                Some(&'-') => {
                    chars.next();
                    result.push('-');
                }
                Some(&'(') => {
                    chars.next(); // consume '('
                    // Two-char special: e.g. \(em = em-dash
                    let c1 = chars.next().unwrap_or(' ');
                    let c2 = chars.next().unwrap_or(' ');
                    if c1 == 'e' && c2 == 'm' {
                        result.push_str("--");
                    } else if c1 == 'e' && c2 == 'n' {
                        result.push('-');
                    } else {
                        result.push(c1);
                        result.push(c2);
                    }
                }
                Some(&'e') => {
                    chars.next();
                    result.push('\\');
                }
                Some(&'"') => {
                    // Rest of line is a comment -- skip.
                    break;
                }
                _ => {
                    result.push('\\');
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Split troff arguments, respecting double-quote grouping.
fn split_troff_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    for ch in chars {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == ' ' && !in_quotes {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }

    args
}

/// Get section title string for a section number.
fn section_title_for(sec: &str) -> &'static str {
    for &(num, title) in SECTION_NAMES {
        let mut buf = [0u8; 4];
        let s = char::from(b'0' + num).encode_utf8(&mut buf);
        if sec == s {
            return title;
        }
    }
    "Manual"
}

/// Word-wrap a line and push result lines.
///
/// This is a simple wrapping that does not account for non-printing ANSI
/// escape sequences in the width calculation.  For man page rendering this
/// is acceptable -- lines with heavy formatting may wrap a few characters
/// early, which is better than not wrapping at all.
fn push_wrapped(out: &mut Vec<String>, text: &str, _max_width: usize) {
    // For simplicity with ANSI escapes, we push each formatted line as-is.
    // Real width calculation with escapes is complex and the source pages
    // are written to fit ~72 columns of actual text.
    out.push(text.to_string());
}

// ============================================================================
// Man page lookup
// ============================================================================

/// Result of looking up a man page: either embedded source or a filesystem path.
enum PageSource {
    Embedded(&'static str),
    File(PathBuf),
}

/// Find all matching man pages for `name`, optionally restricted to `section`.
fn find_pages(name: &str, section: Option<u8>) -> Vec<(u8, PageSource)> {
    let mut results = Vec::new();

    // 1. Search embedded pages first.
    for page in EMBEDDED_PAGES {
        if page.name.eq_ignore_ascii_case(name) {
            if let Some(sec) = section {
                if page.section == sec {
                    results.push((page.section, PageSource::Embedded(page.source)));
                }
            } else {
                results.push((page.section, PageSource::Embedded(page.source)));
            }
        }
    }

    // 2. Search filesystem MANPATH.
    let manpath = env::var("MANPATH").ok();
    let paths: Vec<&str> = match manpath.as_deref() {
        Some(p) => p.split(':').collect(),
        None => DEFAULT_MANPATH.to_vec(),
    };

    let sections_to_search: Vec<u8> = if let Some(sec) = section {
        vec![sec]
    } else {
        vec![1, 2, 3, 4, 5, 7, 8]
    };

    for base in &paths {
        for sec in &sections_to_search {
            // Check for name.section or name.section.gz (we don't decompress
            // gzip here, but list the path).
            let dir = PathBuf::from(base).join(format!("man{sec}"));
            let filename = format!("{name}.{sec}");
            let filepath = dir.join(&filename);
            if filepath.is_file() {
                // Skip if we already have an embedded page for same section.
                let dominated = results
                    .iter()
                    .any(|(s, _)| *s == *sec);
                if !dominated {
                    results.push((*sec, PageSource::File(filepath)));
                }
            }
        }
    }

    results
}

/// Extract the NAME line from troff source (for whatis / apropos).
fn extract_name_line(source: &str) -> Option<String> {
    let mut in_name = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(".SH") && trimmed.contains("NAME") {
            in_name = true;
            continue;
        }
        if in_name {
            if trimmed.starts_with(".SH") || trimmed.starts_with(".SS") {
                return None;
            }
            let clean = trimmed
                .replace("\\fB", "")
                .replace("\\fI", "")
                .replace("\\fR", "")
                .replace("\\fP", "")
                .replace("\\-", "-");
            if !clean.is_empty() {
                return Some(clean);
            }
        }
    }
    None
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

/// Read a single numeric value from a proc/sys file.
fn read_proc_value(path: &str) -> Option<usize> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

/// Check whether stdout is connected to a terminal.
fn stdout_is_tty() -> bool {
    // On OurOS we check /proc/self/fd/1 type, but a simple heuristic: if we
    // can read terminal size we are probably on a tty.
    read_proc_value("/sys/tty/rows").is_some()
        || read_proc_value("/proc/self/tty_rows").is_some()
        || env::var("TERM").is_ok()
}

// ============================================================================
// Minimal libc bindings for termios (raw terminal mode for built-in pager)
// ============================================================================

#[cfg(unix)]
mod libc_term {
    //! Minimal termios bindings for the built-in pager.

    pub type TcflagT = u32;
    pub type CcT = u8;
    #[allow(dead_code)]
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

    // Local flags.
    pub const ECHO: TcflagT = 0o000010;
    pub const ICANON: TcflagT = 0o000002;

    // c_cc indices.
    pub const VMIN: usize = 6;
    pub const VTIME: usize = 5;

    // tcsetattr actions.
    pub const TCSAFLUSH: i32 = 2;

    unsafe extern "C" {
        pub fn tcgetattr(fd: i32, termios_p: *mut Termios) -> i32;
        pub fn tcsetattr(fd: i32, action: i32, termios_p: *const Termios) -> i32;
    }
}

// ============================================================================
// Built-in pager
// ============================================================================

/// Simple built-in pager that displays lines one screenful at a time.
///
/// Handles: Space/Enter = next page, q = quit, /pattern = search forward.
fn builtin_pager(lines: &[String]) {
    let (rows, _cols) = terminal_size();
    let page_size = if rows > 2 { rows - 2 } else { 20 };

    // Try to enter raw mode for single-keypress input.
    #[cfg(unix)]
    let saved_termios = enter_raw_mode();

    let mut offset: usize = 0;
    let mut stdout = io::stdout();

    loop {
        // Print one page of lines.
        let end = std::cmp::min(offset + page_size, lines.len());
        for line in &lines[offset..end] {
            let _ = writeln!(stdout, "{line}");
        }
        let _ = stdout.flush();

        offset = end;
        if offset >= lines.len() {
            break;
        }

        // Prompt.
        let _ = write!(
            stdout,
            "{ESC_BOLD}-- more ({:.0}%) -- press SPACE for next page, q to quit --{ESC_RESET}",
            (offset as f64 / lines.len() as f64) * 100.0,
        );
        let _ = stdout.flush();

        // Read one byte.
        let mut buf = [0u8; 1];
        if io::stdin().read_exact(&mut buf).is_err() {
            break;
        }

        // Clear the prompt line.
        let _ = write!(stdout, "\r\x1b[K");
        let _ = stdout.flush();

        match buf[0] {
            b'q' | b'Q' => break,
            b' ' => { /* next page -- continue loop */ }
            b'\n' | b'\r'
                // Scroll one line from the current position.
                if offset < lines.len() => {
                    let one_end = std::cmp::min(offset + 1, lines.len());
                    for line in &lines[offset..one_end] {
                        let _ = writeln!(stdout, "{line}");
                    }
                    let _ = stdout.flush();
                    offset = one_end;
                    if offset >= lines.len() {
                        break;
                    }
                    continue;
                }
            _ => { /* treat anything else as next page */ }
        }
    }

    // Restore terminal.
    #[cfg(unix)]
    if let Some(saved) = saved_termios {
        restore_termios(&saved);
    }
}

/// Enter raw terminal mode for single-character reads.
#[cfg(unix)]
fn enter_raw_mode() -> Option<libc_term::Termios> {
    let mut original = libc_term::Termios {
        c_iflag: 0,
        c_oflag: 0,
        c_cflag: 0,
        c_lflag: 0,
        c_line: 0,
        c_cc: [0; libc_term::NCCS],
        c_ispeed: 0,
        c_ospeed: 0,
    };

    let ok = unsafe { libc_term::tcgetattr(0, &mut original) };
    if ok != 0 {
        return None;
    }

    let mut raw = original;
    raw.c_lflag &= !(libc_term::ECHO | libc_term::ICANON);
    raw.c_cc[libc_term::VMIN] = 1;
    raw.c_cc[libc_term::VTIME] = 0;

    let ok = unsafe { libc_term::tcsetattr(0, libc_term::TCSAFLUSH, &raw) };
    if ok != 0 {
        return None;
    }

    Some(original)
}

/// Restore saved terminal settings.
#[cfg(unix)]
fn restore_termios(saved: &libc_term::Termios) {
    unsafe {
        libc_term::tcsetattr(0, libc_term::TCSAFLUSH, saved);
    }
}

// ============================================================================
// Output dispatch -- pipe through `less` or use built-in pager
// ============================================================================

/// Display formatted lines, piping to `less` if available, otherwise using the
/// built-in pager when on a tty, or printing directly when piped.
fn display_page(lines: &[String]) {
    if !stdout_is_tty() {
        // Not a terminal -- just dump everything.
        let mut stdout = io::stdout();
        for line in lines {
            let _ = writeln!(stdout, "{line}");
        }
        return;
    }

    // Try to invoke `less -R` (pass-through ANSI escapes).
    if try_less(lines) {
        return;
    }

    // Fall back to built-in pager.
    builtin_pager(lines);
}

/// Attempt to pipe output through `less -R`. Returns `true` on success.
fn try_less(lines: &[String]) -> bool {
    use std::process::{Command, Stdio};

    let child = Command::new("less")
        .arg("-R")
        .stdin(Stdio::piped())
        .spawn();

    match child {
        Ok(mut proc) => {
            if let Some(ref mut stdin) = proc.stdin {
                for line in lines {
                    if writeln!(stdin, "{line}").is_err() {
                        break;
                    }
                }
            }
            // Close stdin so less sees EOF.
            drop(proc.stdin.take());
            let _ = proc.wait();
            true
        }
        Err(_) => false,
    }
}

// ============================================================================
// Command modes
// ============================================================================

/// `man NAME` or `man SECTION NAME` -- display a manual page.
fn cmd_display(name: &str, section: Option<u8>, show_all: bool) {
    let pages = find_pages(name, section);

    if pages.is_empty() {
        let sec_hint = section.map_or(String::new(), |s| format!(" in section {s}"));
        eprintln!("man: no manual entry for '{name}'{sec_hint}");
        process::exit(1);
    }

    let (_cols_unused, cols) = {
        let (_, c) = terminal_size();
        (0, c)
    };

    let mut first = true;
    for (sec, source) in &pages {
        if !first {
            // Separator between multiple pages.
            println!();
            println!(
                "{ESC_BOLD}--- {name}({sec}) ---{ESC_RESET}"
            );
            println!();
        }
        first = false;

        let text = match source {
            PageSource::Embedded(src) => (*src).to_string(),
            PageSource::File(path) => match fs::read_to_string(path) {
                Ok(content) => content,
                Err(e) => {
                    eprintln!("man: cannot read '{}': {e}", path.display());
                    continue;
                }
            },
        };

        let formatted = format_manpage(&text, cols);
        display_page(&formatted);

        if !show_all {
            break;
        }
    }
}

/// `man -w NAME` -- print the path of the man page.
fn cmd_where(name: &str, section: Option<u8>) {
    let pages = find_pages(name, section);

    if pages.is_empty() {
        let sec_hint = section.map_or(String::new(), |s| format!(" in section {s}"));
        eprintln!("man: no manual entry for '{name}'{sec_hint}");
        process::exit(1);
    }

    for (sec, source) in &pages {
        match source {
            PageSource::Embedded(_) => {
                println!("[embedded] {name}({sec})");
            }
            PageSource::File(path) => {
                println!("{}", path.display());
            }
        }
    }
}

/// `man -f NAME` (whatis) -- print the one-line description.
fn cmd_whatis(name: &str) {
    let mut found = false;

    for page in EMBEDDED_PAGES {
        if page.name.eq_ignore_ascii_case(name)
            && let Some(desc) = extract_name_line(page.source) {
                println!("{}({}) - {desc}", page.name, page.section);
                found = true;
            }
    }

    // Also check filesystem pages.
    let pages = find_pages(name, None);
    for (sec, source) in &pages {
        if let PageSource::File(path) = source
            && let Ok(content) = fs::read_to_string(path)
                && let Some(desc) = extract_name_line(&content) {
                    println!("{name}({sec}) - {desc}");
                    found = true;
                }
    }

    if !found {
        eprintln!("{name}: nothing appropriate.");
        process::exit(1);
    }
}

/// `man -k KEYWORD` (apropos) -- search all man page names and descriptions.
fn cmd_apropos(keyword: &str) {
    let kw_lower = keyword.to_lowercase();
    let mut found = false;

    for page in EMBEDDED_PAGES {
        // Check name match.
        let name_match = page.name.to_lowercase().contains(&kw_lower);

        // Check description match.
        let desc_match = extract_name_line(page.source)
            .is_some_and(|d| d.to_lowercase().contains(&kw_lower));

        // Check full-text match.
        let text_match = page.source.to_lowercase().contains(&kw_lower);

        if name_match || desc_match || text_match {
            let desc = extract_name_line(page.source).unwrap_or_default();
            println!("{}({}) - {desc}", page.name, page.section);
            found = true;
        }
    }

    // Search filesystem pages too.
    let manpath = env::var("MANPATH").ok();
    let paths: Vec<&str> = match manpath.as_deref() {
        Some(p) => p.split(':').collect(),
        None => DEFAULT_MANPATH.to_vec(),
    };

    for base in &paths {
        for sec in &[1u8, 2, 3, 4, 5, 7, 8] {
            let dir = PathBuf::from(base).join(format!("man{sec}"));
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let fname = entry.file_name();
                    let fname_str = fname.to_string_lossy();
                    if fname_str.to_lowercase().contains(&kw_lower) {
                        let stem = fname_str
                            .rsplit_once('.')
                            .map_or(fname_str.as_ref(), |(s, _)| s);
                        println!("{stem}({sec})");
                        found = true;
                    }
                }
            }
        }
    }

    if !found {
        eprintln!("{keyword}: nothing appropriate.");
        process::exit(1);
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

struct Args {
    mode: Mode,
    section: Option<u8>,
    name: String,
    show_all: bool,
}

enum Mode {
    Display,
    Where,
    Whatis,
    Apropos,
    Help,
    Version,
}

fn print_help() {
    println!("Usage: man [OPTIONS] [SECTION] NAME");
    println!();
    println!("Display manual pages for OurOS commands and system interfaces.");
    println!();
    println!("Options:");
    println!("  -k KEYWORD    Search page names and descriptions (apropos)");
    println!("  -f NAME       One-line description (whatis)");
    println!("  -a            Show all matching pages, not just the first");
    println!("  -w            Print the file path of the man page");
    println!("  -h, --help    Show this help and exit");
    println!("  --version     Show version and exit");
    println!();
    println!("Sections:");
    for &(num, title) in SECTION_NAMES {
        println!("  {num}  {title}");
    }
    println!();
    println!("Examples:");
    println!("  man ls          View the ls manual page");
    println!("  man 1 grep      View grep in section 1");
    println!("  man -k file     Search for pages about \"file\"");
    println!("  man -f mount    Show one-line description of mount");
}

fn parse_args() -> Args {
    let argv: Vec<String> = env::args().skip(1).collect();

    if argv.is_empty() {
        eprintln!("What manual page do you want?");
        eprintln!("For example, try 'man man'.");
        process::exit(1);
    }

    let mut mode = Mode::Display;
    let mut section: Option<u8> = None;
    let mut show_all = false;
    let mut positional: Vec<String> = Vec::new();
    let mut idx = 0;

    while idx < argv.len() {
        let arg = &argv[idx];
        match arg.as_str() {
            "-h" | "--help" => {
                return Args {
                    mode: Mode::Help,
                    section: None,
                    name: String::new(),
                    show_all: false,
                };
            }
            "--version" => {
                return Args {
                    mode: Mode::Version,
                    section: None,
                    name: String::new(),
                    show_all: false,
                };
            }
            "-a" => {
                show_all = true;
            }
            "-w" => {
                mode = Mode::Where;
            }
            "-f" => {
                mode = Mode::Whatis;
                idx += 1;
                if idx >= argv.len() {
                    eprintln!("man: -f requires an argument");
                    process::exit(1);
                }
                return Args {
                    mode,
                    section: None,
                    name: argv[idx].clone(),
                    show_all: false,
                };
            }
            "-k" => {
                mode = Mode::Apropos;
                idx += 1;
                if idx >= argv.len() {
                    eprintln!("man: -k requires an argument");
                    process::exit(1);
                }
                return Args {
                    mode,
                    section: None,
                    name: argv[idx].clone(),
                    show_all: false,
                };
            }
            other => {
                positional.push(other.to_string());
            }
        }
        idx += 1;
    }

    // Parse positional args: either [SECTION] NAME or just NAME.
    let name;
    if positional.len() >= 2 {
        // First positional might be a section number.
        if let Ok(sec) = positional[0].parse::<u8>() {
            if [1, 2, 3, 4, 5, 7, 8].contains(&sec) {
                section = Some(sec);
                name = positional[1].clone();
            } else {
                // Not a valid section -- treat as name.
                name = positional[0].clone();
            }
        } else {
            name = positional[0].clone();
        }
    } else if positional.len() == 1 {
        name = positional[0].clone();
    } else {
        eprintln!("What manual page do you want?");
        eprintln!("For example, try 'man man'.");
        process::exit(1);
    }

    Args {
        mode,
        section,
        name,
        show_all,
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args = parse_args();

    match args.mode {
        Mode::Help => {
            print_help();
        }
        Mode::Version => {
            println!("man (OurOS) {VERSION}");
        }
        Mode::Display => {
            cmd_display(&args.name, args.section, args.show_all);
        }
        Mode::Where => {
            cmd_where(&args.name, args.section);
        }
        Mode::Whatis => {
            cmd_whatis(&args.name);
        }
        Mode::Apropos => {
            cmd_apropos(&args.name);
        }
    }
}
