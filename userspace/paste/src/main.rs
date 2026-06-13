//! SlateOS `paste` Utility -- Merge Lines of Files Side by Side
//!
//! Reads corresponding lines from each input file and joins them with a
//! delimiter, writing the merged result to standard output. Modeled after
//! POSIX/GNU `paste`.
//!
//! # Usage
//!
//! ```text
//! paste [OPTION]... [FILE]...
//!
//! Merge corresponding lines from each FILE, separated by TABs.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -d, --delimiters=LIST   Use characters from LIST instead of TABs
//!   -s, --serial            Paste one file at a time, not in parallel
//!   -z, --zero-terminated   Use NUL as line terminator instead of newline
//!       --json              Output merged lines as JSON array of arrays
//!       --help              Display this help and exit
//!       --version           Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Input file paths. `-` means stdin.
    file_paths: Vec<String>,
    /// Delimiter characters to cycle through between fields.
    delimiters: Vec<Delimiter>,
    /// Serial mode: paste each file's lines onto a single output line.
    serial: bool,
    /// Use NUL (`\0`) as the line terminator instead of newline.
    zero_terminated: bool,
    /// Emit output as JSON (array of arrays of strings).
    json: bool,
}

/// A single delimiter element, parsed from the `-d` list.
#[derive(Clone)]
enum Delimiter {
    /// A literal character to insert between fields.
    Char(char),
    /// Empty delimiter (`\0` escape) -- no separator between adjacent fields.
    Empty,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Delimiter parsing
// ============================================================================

/// Parse a delimiter list string, interpreting escape sequences:
///   `\n` -> newline, `\t` -> tab, `\\` -> backslash, `\0` -> empty delimiter
///
/// Any other `\X` is treated as the literal character `X` (matching GNU
/// behavior).
fn parse_delimiters(s: &str) -> Vec<Delimiter> {
    let mut delims = Vec::new();
    let mut chars = s.chars();

    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => delims.push(Delimiter::Char('\n')),
                Some('t') => delims.push(Delimiter::Char('\t')),
                Some('\\') => delims.push(Delimiter::Char('\\')),
                Some('0') => delims.push(Delimiter::Empty),
                Some(other) => delims.push(Delimiter::Char(other)),
                // Trailing backslash -- treat as literal backslash.
                None => delims.push(Delimiter::Char('\\')),
            }
        } else {
            delims.push(Delimiter::Char(ch));
        }
    }

    // If the delimiter string was empty, default to a single tab.
    if delims.is_empty() {
        delims.push(Delimiter::Char('\t'));
    }

    delims
}

// ============================================================================
// Argument parsing
// ============================================================================

fn parse_args(args: &[String]) -> ParseResult {
    let mut file_paths: Vec<String> = Vec::new();
    let mut delimiters_str: Option<String> = None;
    let mut serial = false;
    let mut zero_terminated = false;
    let mut json = false;
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
            if arg == "--serial" {
                serial = true;
            } else if arg == "--zero-terminated" {
                zero_terminated = true;
            } else if arg == "--json" {
                json = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--delimiters" || arg.starts_with("--delimiters=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--delimiters=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("paste: option '--delimiters' requires an argument");
                        eprintln!("Try 'paste --help' for more information.");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                delimiters_str = Some(val);
            } else {
                eprintln!("paste: unrecognized option '{arg}'");
                eprintln!("Try 'paste --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options. `-d` consumes the next argument (or the rest of this
        // one if bundled, e.g. `-d,` is equivalent to `-d ','`).
        let short = &arg[1..];
        let mut chars = short.chars();
        while let Some(ch) = chars.next() {
            match ch {
                's' => serial = true,
                'z' => zero_terminated = true,
                'd' => {
                    // Everything remaining in this argument is the delimiter
                    // string (allows `-d,` without a space). If nothing
                    // remains, consume the next argument.
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("paste: option '-d' requires an argument");
                            eprintln!("Try 'paste --help' for more information.");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    delimiters_str = Some(val);
                    // chars iterator is now exhausted (we collected it), so
                    // break out of the short-option loop.
                    break;
                }
                _ => {
                    eprintln!("paste: invalid option -- '{ch}'");
                    eprintln!("Try 'paste --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    // Default to stdin if no files given.
    if file_paths.is_empty() {
        file_paths.push("-".to_string());
    }

    let delimiters = match delimiters_str {
        Some(s) => parse_delimiters(&s),
        None => vec![Delimiter::Char('\t')],
    };

    ParseResult::Run(Config {
        file_paths,
        delimiters,
        serial,
        zero_terminated,
        json,
    })
}

// ============================================================================
// Input source abstraction
// ============================================================================

/// A line-oriented input source. Wraps either a file or a shared reference to
/// stdin. Each call to `read_line` returns the next line (without the trailing
/// newline/NUL terminator), or `None` at EOF.
enum InputSource {
    File(BufReader<File>),
    Stdin,
}

impl InputSource {
    /// Read the next line from this source.
    ///
    /// `line_buf` is reused across calls to avoid allocation.
    /// `zero_term` controls whether lines are delimited by NUL or newline.
    ///
    /// Returns `Ok(Some(line))` for a line, `Ok(None)` at EOF, or `Err` on
    /// I/O failure.
    fn read_line(
        &mut self,
        line_buf: &mut String,
        zero_term: bool,
        stdin_lock: &mut io::StdinLock<'_>,
    ) -> io::Result<Option<String>> {
        line_buf.clear();

        let bytes_read = if zero_term {
            match self {
                InputSource::File(reader) => read_until_byte(reader, 0, line_buf),
                InputSource::Stdin => read_until_byte(stdin_lock, 0, line_buf),
            }
        } else {
            match self {
                InputSource::File(reader) => reader.read_line(line_buf),
                InputSource::Stdin => stdin_lock.read_line(line_buf),
            }
        }?;

        if bytes_read == 0 {
            return Ok(None);
        }

        // Strip the trailing terminator character.
        if zero_term {
            if line_buf.ends_with('\0') {
                line_buf.pop();
            }
        } else if line_buf.ends_with('\n') {
            line_buf.pop();
            // Also strip \r\n on Windows-style line endings.
            if line_buf.ends_with('\r') {
                line_buf.pop();
            }
        }

        Ok(Some(line_buf.clone()))
    }
}

/// Read from `reader` until byte `delim` is found, appending valid UTF-8 to
/// `buf`. Returns the number of bytes read (0 at EOF).
fn read_until_byte<R: BufRead>(reader: &mut R, delim: u8, buf: &mut String) -> io::Result<usize> {
    let mut raw = Vec::new();
    let n = reader.read_until(delim, &mut raw)?;
    // Convert to UTF-8, replacing invalid sequences (paths/data could contain
    // arbitrary bytes when NUL-terminated mode is used, but String requires
    // UTF-8). In practice, paste operates on text, so this is fine.
    let text = String::from_utf8(raw).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
    buf.push_str(&text);
    Ok(n)
}

/// Open an input source from a file path. `-` means stdin.
fn open_input(path: &str) -> io::Result<InputSource> {
    if path == "-" {
        Ok(InputSource::Stdin)
    } else {
        let f = File::open(path)?;
        Ok(InputSource::File(BufReader::new(f)))
    }
}

// ============================================================================
// JSON helpers
// ============================================================================

/// Write a JSON-escaped string (with surrounding quotes) to the writer.
fn write_json_string<W: Write>(w: &mut W, s: &str) -> io::Result<()> {
    w.write_all(b"\"")?;
    for ch in s.chars() {
        match ch {
            '"' => w.write_all(b"\\\"")?,
            '\\' => w.write_all(b"\\\\")?,
            '\n' => w.write_all(b"\\n")?,
            '\r' => w.write_all(b"\\r")?,
            '\t' => w.write_all(b"\\t")?,
            c if c < '\x20' => {
                // Control characters as \uXXXX.
                write!(w, "\\u{:04x}", c as u32)?;
            }
            c => {
                let mut utf8_buf = [0u8; 4];
                let encoded = c.encode_utf8(&mut utf8_buf);
                w.write_all(encoded.as_bytes())?;
            }
        }
    }
    w.write_all(b"\"")?;
    Ok(())
}

/// Write a JSON array of strings to the writer.
fn write_json_row<W: Write>(w: &mut W, fields: &[String]) -> io::Result<()> {
    w.write_all(b"[")?;
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            w.write_all(b", ")?;
        }
        write_json_string(w, field)?;
    }
    w.write_all(b"]")?;
    Ok(())
}

// ============================================================================
// Parallel paste (default mode)
// ============================================================================

/// Parallel paste: merge corresponding lines from all inputs, separated by the
/// cycling delimiter list. Stop when every input has reached EOF.
fn paste_parallel(config: &Config) -> io::Result<i32> {
    let mut sources: Vec<InputSource> = Vec::new();
    let mut all_ok = true;

    for path in &config.file_paths {
        match open_input(path) {
            Ok(src) => sources.push(src),
            Err(e) => {
                eprintln!("paste: {path}: {e}");
                all_ok = false;
            }
        }
    }

    if sources.is_empty() {
        return Ok(if all_ok { 0 } else { 1 });
    }

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let terminator: &[u8] = if config.zero_terminated { b"\0" } else { b"\n" };

    let mut line_buf = String::new();
    // Track which inputs have reached EOF so we can stop when all are done.
    let mut eof_flags: Vec<bool> = vec![false; sources.len()];

    if config.json {
        out.write_all(b"[")?;
    }

    let mut first_json_row = true;

    loop {
        let mut any_alive = false;
        let mut fields: Vec<String> = Vec::with_capacity(sources.len());

        for (idx, source) in sources.iter_mut().enumerate() {
            if eof_flags[idx] {
                fields.push(String::new());
                continue;
            }

            match source.read_line(&mut line_buf, config.zero_terminated, &mut stdin_lock)? {
                Some(line) => {
                    any_alive = true;
                    fields.push(line);
                }
                None => {
                    eof_flags[idx] = true;
                    fields.push(String::new());
                }
            }
        }

        if !any_alive {
            break;
        }

        if config.json {
            if !first_json_row {
                out.write_all(b",\n  ")?;
            } else {
                out.write_all(b"\n  ")?;
                first_json_row = false;
            }
            write_json_row(&mut out, &fields)?;
        } else {
            write_delimited_line(&mut out, &fields, &config.delimiters)?;
            out.write_all(terminator)?;
        }
    }

    if config.json {
        out.write_all(b"\n]\n")?;
    }

    out.flush()?;
    Ok(if all_ok { 0 } else { 1 })
}

/// Write fields joined by the cycling delimiter list.
fn write_delimited_line<W: Write>(
    w: &mut W,
    fields: &[String],
    delimiters: &[Delimiter],
) -> io::Result<()> {
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            // Cycle through delimiter list. The delimiter between field N-1 and
            // field N uses index (N-1) mod len(delimiters).
            let delim_idx = (i - 1) % delimiters.len();
            match &delimiters[delim_idx] {
                Delimiter::Char(c) => {
                    let mut utf8_buf = [0u8; 4];
                    let encoded = c.encode_utf8(&mut utf8_buf);
                    w.write_all(encoded.as_bytes())?;
                }
                Delimiter::Empty => {}
            }
        }
        w.write_all(field.as_bytes())?;
    }
    Ok(())
}

// ============================================================================
// Serial paste (-s mode)
// ============================================================================

/// Serial paste: for each input file, join all its lines onto a single output
/// line using the cycling delimiters.
fn paste_serial(config: &Config) -> io::Result<i32> {
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let terminator: &[u8] = if config.zero_terminated { b"\0" } else { b"\n" };
    let mut all_ok = true;

    if config.json {
        out.write_all(b"[")?;
    }

    let mut first_json_row = true;

    for path in &config.file_paths {
        let mut source = match open_input(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("paste: {path}: {e}");
                all_ok = false;
                continue;
            }
        };

        let mut line_buf = String::new();
        let mut fields: Vec<String> = Vec::new();

        while let Some(line) =
            source.read_line(&mut line_buf, config.zero_terminated, &mut stdin_lock)?
        {
            fields.push(line);
        }

        if config.json {
            if !first_json_row {
                out.write_all(b",\n  ")?;
            } else {
                out.write_all(b"\n  ")?;
                first_json_row = false;
            }
            write_json_row(&mut out, &fields)?;
        } else {
            // In serial mode, delimiters cycle across the fields within one
            // file. Write the first field, then delimiter+field for each
            // subsequent.
            for (i, field) in fields.iter().enumerate() {
                if i > 0 {
                    let delim_idx = (i - 1) % config.delimiters.len();
                    match &config.delimiters[delim_idx] {
                        Delimiter::Char(c) => {
                            let mut utf8_buf = [0u8; 4];
                            let encoded = c.encode_utf8(&mut utf8_buf);
                            out.write_all(encoded.as_bytes())?;
                        }
                        Delimiter::Empty => {}
                    }
                }
                out.write_all(field.as_bytes())?;
            }
            out.write_all(terminator)?;
        }
    }

    if config.json {
        out.write_all(b"\n]\n")?;
    }

    out.flush()?;
    Ok(if all_ok { 0 } else { 1 })
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("SlateOS paste v{VERSION}");
    println!();
    println!("Merge corresponding lines from each FILE, separated by TABs.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  paste [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -d, --delimiters=LIST   Use characters from LIST instead of TABs");
    println!("                          Escape sequences: \\n (newline), \\t (tab),");
    println!("                          \\\\ (backslash), \\0 (empty = no delimiter)");
    println!("  -s, --serial            Paste one file at a time instead of in parallel");
    println!("  -z, --zero-terminated   Use NUL as line terminator instead of newline");
    println!("      --json              Output merged lines as JSON array of arrays");
    println!("      --help              Display this help and exit");
    println!("      --version           Output version information and exit");
    println!();
    println!("EXAMPLES:");
    println!("  paste file1 file2           Merge lines side by side with TAB");
    println!("  paste -d ',' f1 f2          Use comma as delimiter");
    println!("  paste -s -d ',' file        Join all lines of file with commas");
    println!("  paste - - < input           Interleave stdin lines into two columns");
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
            println!("paste (SlateOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let result = if config.serial {
                paste_serial(&config)
            } else {
                paste_parallel(&config)
            };

            match result {
                Ok(code) => process::exit(code),
                Err(e) => {
                    eprintln!("paste: {e}");
                    process::exit(1);
                }
            }
        }
    }
}
