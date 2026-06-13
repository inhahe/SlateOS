//! Slate OS `wc` Utility -- Word, Line, Character, and Byte Count
//!
//! Counts lines, words, characters, and bytes in files or standard input.
//! Modeled after GNU coreutils `wc` with the same flag set.
//!
//! # Usage
//!
//! ```text
//! wc [OPTION]... [FILE]...
//!
//! Print newline, word, and byte counts for each FILE, and a total line if
//! more than one FILE is specified. A word is a non-zero-length sequence of
//! non-whitespace characters delimited by whitespace.
//!
//!   -c, --bytes             Print the byte counts
//!   -m, --chars             Print the character counts
//!   -l, --lines             Print the newline counts
//!   -w, --words             Print the word counts
//!   -L, --max-line-length   Print the maximum display width
//!       --files0-from=FILE  Read NUL-delimited filenames from FILE
//!       --json              Output results as JSON
//!       --help              Display this help and exit
//!       --version           Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Read buffer size. 8 KiB balances syscall overhead against memory usage.
const BUF_SIZE: usize = 8192;

// ============================================================================
// Counts
// ============================================================================

/// Accumulated counts for a single file or the grand total.
#[derive(Clone, Copy, Default)]
struct Counts {
    lines: u64,
    words: u64,
    bytes: u64,
    chars: u64,
    max_line_len: u64,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.lines += other.lines;
        self.words += other.words;
        self.bytes += other.bytes;
        self.chars += other.chars;
        if other.max_line_len > self.max_line_len {
            self.max_line_len = other.max_line_len;
        }
    }
}

// ============================================================================
// Display selection flags
// ============================================================================

/// Which columns to display. When no flags are given, lines+words+bytes is the
/// default (matching GNU wc).
#[derive(Clone, Copy)]
#[derive(Default)]
struct DisplayFlags {
    lines: bool,
    words: bool,
    bytes: bool,
    chars: bool,
    max_line_len: bool,
}

impl DisplayFlags {
    /// True when nothing was explicitly requested -- caller should apply
    /// the default set.
    fn none_set(self) -> bool {
        !(self.lines || self.words || self.bytes || self.chars || self.max_line_len)
    }

    /// Apply the default selection: lines, words, bytes.
    fn apply_defaults(&mut self) {
        self.lines = true;
        self.words = true;
        self.bytes = true;
    }
}


// ============================================================================
// Parsed configuration
// ============================================================================

struct Config {
    /// Files to count. Empty means read stdin.
    file_paths: Vec<String>,
    /// Which columns to show.
    display: DisplayFlags,
    /// Output as JSON instead of columnar text.
    json: bool,
}

enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> ParseResult {
    let mut file_paths: Vec<String> = Vec::new();
    let mut display = DisplayFlags::default();
    let mut json = false;
    let mut files0_from: Option<String> = None;
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            file_paths.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            if arg == "--lines" {
                display.lines = true;
            } else if arg == "--words" {
                display.words = true;
            } else if arg == "--bytes" {
                display.bytes = true;
            } else if arg == "--chars" {
                display.chars = true;
            } else if arg == "--max-line-length" {
                display.max_line_len = true;
            } else if arg == "--json" {
                json = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--files0-from" || arg.starts_with("--files0-from=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--files0-from=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("wc: option '--files0-from' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                files0_from = Some(val);
            } else {
                eprintln!("wc: unrecognized option '{arg}'");
                eprintln!("Try 'wc --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options: may be combined (e.g., `-lw`).
        for ch in arg[1..].chars() {
            match ch {
                'l' => display.lines = true,
                'w' => display.words = true,
                'c' => display.bytes = true,
                'm' => display.chars = true,
                'L' => display.max_line_len = true,
                _ => {
                    eprintln!("wc: invalid option -- '{ch}'");
                    eprintln!("Try 'wc --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    // Load filenames from --files0-from if specified.
    if let Some(f0) = files0_from {
        match read_files0(&f0) {
            Ok(names) => {
                for name in names {
                    if !name.is_empty() {
                        file_paths.push(name);
                    }
                }
            }
            Err(e) => {
                eprintln!("wc: cannot open '{f0}' for reading: {e}");
                process::exit(1);
            }
        }
    }

    if display.none_set() {
        display.apply_defaults();
    }

    ParseResult::Run(Config {
        file_paths,
        display,
        json,
    })
}

/// Read NUL-delimited filenames from a file (or stdin if path is `-`).
fn read_files0(path: &str) -> io::Result<Vec<String>> {
    let data = if path == "-" {
        let mut buf = Vec::new();
        io::stdin().lock().read_to_end(&mut buf)?;
        buf
    } else {
        let mut buf = Vec::new();
        File::open(path)?.read_to_end(&mut buf)?;
        buf
    };

    // Split on NUL bytes. Each segment is a filename.
    let names: Vec<String> = data
        .split(|&b| b == 0)
        .map(|seg| String::from_utf8_lossy(seg).into_owned())
        .collect();

    Ok(names)
}

// ============================================================================
// Counting engine
// ============================================================================

/// Count lines, words, bytes, chars, and max-line-length by reading `reader`
/// in chunks. Only computes the metrics that `display` requests, except that
/// bytes are always counted (virtually free since we see every chunk anyway).
fn count_reader<R: Read>(reader: &mut R, display: &DisplayFlags) -> io::Result<Counts> {
    let mut counts = Counts::default();
    let mut buf = [0u8; BUF_SIZE];

    // Track whether the previous byte was whitespace, for word-boundary
    // detection. Start as true so a leading non-whitespace byte starts a word.
    let mut prev_ws = true;

    // Current line length (bytes or chars depending on what is requested).
    // We track both byte-length and char-length of the current line so we can
    // report char-based max-line-length when -m is also active, matching GNU
    // wc behavior (max-line-length counts characters, not bytes).
    let mut cur_line_bytes: u64 = 0;
    let mut cur_line_chars: u64 = 0;

    let need_words = display.words;
    let need_chars = display.chars;
    let need_max_line = display.max_line_len;

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }

        let chunk = &buf[..n];
        counts.bytes += n as u64;

        for &byte in chunk {
            // Lines: count newline bytes.
            if byte == b'\n' {
                counts.lines += 1;

                if need_max_line {
                    // Use character length if chars mode requested, else byte
                    // length (GNU wc uses "display width" which for ASCII is
                    // equivalent to bytes; we approximate with char count when
                    // -m is active).
                    let line_len = if need_chars {
                        cur_line_chars
                    } else {
                        cur_line_bytes
                    };
                    if line_len > counts.max_line_len {
                        counts.max_line_len = line_len;
                    }
                    cur_line_bytes = 0;
                    cur_line_chars = 0;
                }
            } else if need_max_line {
                cur_line_bytes += 1;
            }

            // Words: detect transitions from whitespace to non-whitespace.
            if need_words {
                let is_ws = matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C);
                if prev_ws && !is_ws {
                    counts.words += 1;
                }
                prev_ws = is_ws;
            }

            // Chars: count UTF-8 start bytes. A byte is a start byte if it
            // does not match the continuation pattern 10xxxxxx. Invalid bytes
            // (bare continuations) each count as one character, matching the
            // behavior of counting "replacement characters."
            if need_chars
                && (byte & 0xC0) != 0x80 {
                    counts.chars += 1;
                    if need_max_line {
                        cur_line_chars += 1;
                    }
                }
                // Continuation bytes: part of a multi-byte char, don't
                // increment char count or line-char-length.
        }
    }

    // Handle the last line if it didn't end with \n.
    if need_max_line {
        let line_len = if need_chars {
            cur_line_chars
        } else {
            cur_line_bytes
        };
        if line_len > counts.max_line_len {
            counts.max_line_len = line_len;
        }
    }

    Ok(counts)
}

// ============================================================================
// Output formatting
// ============================================================================

/// Collect the values that will be printed for a given set of counts, in
/// display order: lines, words, chars, bytes, max-line-length.
fn selected_values(counts: &Counts, display: &DisplayFlags) -> Vec<u64> {
    let mut vals = Vec::new();
    if display.lines {
        vals.push(counts.lines);
    }
    if display.words {
        vals.push(counts.words);
    }
    if display.chars {
        vals.push(counts.chars);
    }
    if display.bytes {
        vals.push(counts.bytes);
    }
    if display.max_line_len {
        vals.push(counts.max_line_len);
    }
    vals
}

/// Determine the minimum column width needed to right-align all values across
/// all results. Examines the total row (or the single row if only one file).
fn column_width(total: &Counts, display: &DisplayFlags) -> usize {
    let vals = selected_values(total, display);
    let max_val = vals.iter().copied().max().unwrap_or(0);
    // Number of digits in the largest value, minimum 1.
    
    if max_val == 0 {
        1
    } else {
        (max_val as f64).log10().floor() as usize + 1
    }
}

/// Print one row of output (columnar, not JSON).
fn print_row(
    out: &mut impl Write,
    counts: &Counts,
    display: &DisplayFlags,
    width: usize,
    label: &str,
) -> io::Result<()> {
    let vals = selected_values(counts, display);
    for (i, val) in vals.iter().enumerate() {
        if i > 0 {
            write!(out, " ")?;
        }
        write!(out, "{val:>width$}")?;
    }
    if !label.is_empty() {
        write!(out, " {label}")?;
    }
    writeln!(out)?;
    Ok(())
}

/// Column names in display order, for JSON output.
fn selected_field_names(display: &DisplayFlags) -> Vec<&'static str> {
    let mut names = Vec::new();
    if display.lines {
        names.push("lines");
    }
    if display.words {
        names.push("words");
    }
    if display.chars {
        names.push("chars");
    }
    if display.bytes {
        names.push("bytes");
    }
    if display.max_line_len {
        names.push("max_line_length");
    }
    names
}

/// Print JSON output for all results.
fn print_json(
    out: &mut impl Write,
    results: &[(String, Counts)],
    total: Option<&Counts>,
    display: &DisplayFlags,
) -> io::Result<()> {
    let fields = selected_field_names(display);

    writeln!(out, "[")?;
    let entry_count = results.len() + if total.is_some() { 1 } else { 0 };
    let mut idx = 0;

    for (name, counts) in results {
        let vals = selected_values(counts, display);
        write!(out, "  {{\"filename\":{}", json_escape(name))?;
        for (fi, field) in fields.iter().enumerate() {
            write!(out, ",\"{}\":{}", field, vals[fi])?;
        }
        write!(out, "}}")?;
        idx += 1;
        if idx < entry_count {
            writeln!(out, ",")?;
        } else {
            writeln!(out)?;
        }
    }

    if let Some(t) = total {
        let vals = selected_values(t, display);
        write!(out, "  {{\"filename\":\"total\"")?;
        for (fi, field) in fields.iter().enumerate() {
            write!(out, ",\"{}\":{}", field, vals[fi])?;
        }
        writeln!(out, "}}")?;
    }

    writeln!(out, "]")?;
    Ok(())
}

/// Minimally escape a string for JSON output. Handles backslash, double-quote,
/// and control characters.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                // \uXXXX for other control chars.
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Core driver
// ============================================================================

/// Process all files (or stdin) and produce output. Returns exit code.
fn run(config: &Config) -> i32 {
    let mut results: Vec<(String, Counts)> = Vec::new();
    let mut total = Counts::default();
    let mut had_error = false;

    let sources: Vec<String> = if config.file_paths.is_empty() {
        // No files specified: read stdin.
        vec!["-".to_string()]
    } else {
        config.file_paths.clone()
    };

    for path in &sources {
        let result = if path == "-" {
            let stdin = io::stdin();
            let mut locked = stdin.lock();
            count_reader(&mut locked, &config.display)
        } else {
            match File::open(path) {
                Ok(f) => {
                    let mut reader = BufReader::with_capacity(BUF_SIZE, f);
                    count_reader(&mut reader, &config.display)
                }
                Err(e) => {
                    eprintln!("wc: {path}: {e}");
                    had_error = true;
                    continue;
                }
            }
        };

        match result {
            Ok(counts) => {
                total.add(&counts);
                let label = if path == "-" {
                    String::new()
                } else {
                    path.clone()
                };
                results.push((label, counts));
            }
            Err(e) => {
                let display_name = if path == "-" { "standard input" } else { path.as_str() };
                eprintln!("wc: {display_name}: {e}");
                had_error = true;
            }
        }
    }

    // Produce output.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if config.json {
        let show_total = results.len() > 1;
        let total_ref = if show_total { Some(&total) } else { None };
        if let Err(e) = print_json(&mut out, &results, total_ref, &config.display) {
            eprintln!("wc: write error: {e}");
            return 1;
        }
    } else {
        let width = if results.len() > 1 {
            column_width(&total, &config.display)
        } else {
            // For a single file, width is based on its own values.
            results
                .first()
                .map(|(_, c)| column_width(c, &config.display))
                .unwrap_or(1)
        };

        for (name, counts) in &results {
            if let Err(e) = print_row(&mut out, counts, &config.display, width, name) {
                eprintln!("wc: write error: {e}");
                return 1;
            }
        }

        if results.len() > 1
            && let Err(e) = print_row(&mut out, &total, &config.display, width, "total") {
                eprintln!("wc: write error: {e}");
                return 1;
            }
    }

    if had_error { 1 } else { 0 }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS wc v{VERSION}");
    println!();
    println!("Print newline, word, and byte counts for each FILE, and a total");
    println!("line if more than one FILE is specified.");
    println!();
    println!("USAGE:");
    println!("  wc [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -c, --bytes             Print the byte counts");
    println!("  -m, --chars             Print the character counts");
    println!("  -l, --lines             Print the newline counts");
    println!("  -w, --words             Print the word counts");
    println!("  -L, --max-line-length   Print the maximum display width");
    println!("      --files0-from=FILE  Read NUL-delimited filenames from FILE");
    println!("      --json              Output results as JSON");
    println!("      --help              Display this help and exit");
    println!("      --version           Output version information and exit");
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("If no counting flags are given, the default is -lwc (lines, words, bytes).");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    match parse_args(&args) {
        ParseResult::Help => {
            print_help();
            process::exit(0);
        }
        ParseResult::Version => {
            println!("wc (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}
