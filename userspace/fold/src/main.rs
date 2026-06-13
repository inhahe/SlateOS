//! SlateOS `fold` Utility -- Wrap Input Lines to Fit a Specified Width
//!
//! Reads input and wraps long lines so they fit within a column width,
//! writing the result to standard output. Modeled after POSIX/GNU `fold`.
//!
//! # Usage
//!
//! ```text
//! fold [OPTION]... [FILE]...
//!
//! Wrap input lines in each FILE, writing to standard output.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -w, --width=N   Use N columns instead of 80
//!   -s, --spaces    Break at spaces (word wrap)
//!   -b, --bytes     Count bytes rather than columns
//!       --help      Display this help and exit
//!       --version   Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_WIDTH: usize = 80;
const TAB_STOP: usize = 8;

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Input file paths. `-` means stdin.
    file_paths: Vec<String>,
    /// Maximum output line width.
    width: usize,
    /// Break at spaces instead of mid-word.
    spaces: bool,
    /// Count bytes rather than columns (skip tab/backspace expansion).
    bytes: bool,
}

/// Result of argument parsing.
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
    let mut width: Option<usize> = None;
    let mut spaces = false;
    let mut bytes = false;
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
            if arg == "--spaces" {
                spaces = true;
            } else if arg == "--bytes" {
                bytes = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--width" || arg.starts_with("--width=") {
                let val_str = if let Some(eq_val) = arg.strip_prefix("--width=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("fold: option '--width' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                match val_str.parse::<usize>() {
                    Ok(n) => width = Some(n),
                    Err(_) => {
                        eprintln!("fold: invalid width: '{val_str}'");
                        process::exit(1);
                    }
                }
            } else {
                eprintln!("fold: unrecognized option '{arg}'");
                eprintln!("Try 'fold --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options. `-w` consumes the next argument (or the rest of
        // this token if bundled, e.g. `-w40`).
        let short = &arg[1..];
        let mut chars = short.chars();
        while let Some(ch) = chars.next() {
            match ch {
                's' => spaces = true,
                'b' => bytes = true,
                'w' => {
                    let remainder: String = chars.collect();
                    let val_str = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("fold: option '-w' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    match val_str.parse::<usize>() {
                        Ok(n) => width = Some(n),
                        Err(_) => {
                            eprintln!("fold: invalid width: '{val_str}'");
                            process::exit(1);
                        }
                    }
                    break;
                }
                _ => {
                    eprintln!("fold: invalid option -- '{ch}'");
                    eprintln!("Try 'fold --help' for more information.");
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

    ParseResult::Run(Config {
        file_paths,
        width: width.unwrap_or(DEFAULT_WIDTH),
        spaces,
        bytes,
    })
}

// ============================================================================
// Folding logic
// ============================================================================

/// Fold a single line in byte-counting mode.
///
/// Simply splits at every `width` bytes, optionally preferring a space
/// boundary when `-s` is active.
fn fold_line_bytes<W: Write>(
    out: &mut W,
    line: &[u8],
    width: usize,
    spaces: bool,
) -> io::Result<()> {
    if width == 0 {
        // Width of zero: emit each byte on its own line (matches GNU).
        for &b in line {
            out.write_all(&[b, b'\n'])?;
        }
        return Ok(());
    }

    let mut start = 0;
    while start < line.len() {
        let remaining = line.len() - start;

        if remaining <= width {
            // Rest of the line fits.
            out.write_all(&line[start..])?;
            break;
        }

        let mut break_at = start + width;

        if spaces {
            // Scan backwards from the break point for a space.
            if let Some(pos) = line[start..break_at].iter().rposition(|&b| b == b' ') {
                // Break after the space (include the space on this segment).
                break_at = start + pos + 1;
            }
            // If no space found, break_at stays at start + width.
        }

        out.write_all(&line[start..break_at])?;
        out.write_all(b"\n")?;
        start = break_at;
    }

    Ok(())
}

/// Fold a single line in column-counting mode (default).
///
/// Tabs expand to the next multiple-of-8 column. Backspaces decrement the
/// column (but not below zero). With `-s`, prefer breaking at the last
/// space that fits.
fn fold_line_columns<W: Write>(
    out: &mut W,
    line: &str,
    width: usize,
    spaces: bool,
) -> io::Result<()> {
    if width == 0 {
        // Width of zero: emit each character on its own line.
        for ch in line.chars() {
            let mut buf = [0u8; 4];
            let encoded = ch.encode_utf8(&mut buf);
            out.write_all(encoded.as_bytes())?;
            out.write_all(b"\n")?;
        }
        return Ok(());
    }

    // We accumulate characters in a buffer and track the display column.
    // When the column would exceed width, we emit and start fresh.
    let mut col: usize = 0;
    let mut buf = String::new();
    // Index into `buf` (in bytes) of the position just after the last
    // space character seen on this segment, used for `-s` word wrapping.
    let mut last_space_buf_end: Option<usize> = None;

    for ch in line.chars() {
        let new_col = match ch {
            '\t' => {
                // Advance to the next tab stop.
                
                (col / TAB_STOP + 1) * TAB_STOP
            }
            '\x08' => {
                // Backspace: decrement column, but not below zero.
                col.saturating_sub(1)
            }
            _ => col + 1,
        };

        if new_col > width {
            // This character would push us past the width limit.
            if spaces
                && let Some(sp_end) = last_space_buf_end {
                    // Break at the last space: emit up to (and including)
                    // the space, then carry over the remainder.
                    out.write_all(&buf.as_bytes()[..sp_end])?;
                    out.write_all(b"\n")?;

                    let leftover = buf[sp_end..].to_string();
                    buf.clear();
                    buf.push_str(&leftover);

                    // Recompute column for the leftover portion.
                    col = recompute_column(&leftover);
                    last_space_buf_end = None;

                    // Now try to fit the current character again.
                    let retry_col = char_advance(col, ch);
                    if retry_col > width && col > 0 {
                        // Still doesn't fit -- flush what we have and
                        // put the character on a fresh line.
                        out.write_all(buf.as_bytes())?;
                        out.write_all(b"\n")?;
                        buf.clear();
                        col = 0;
                        last_space_buf_end = None;
                    }

                    buf.push(ch);
                    col = char_advance(col, ch);

                    if ch == ' ' {
                        last_space_buf_end = Some(buf.len());
                    }
                    continue;
                }

            // No space break available (or -s not active): hard break now.
            out.write_all(buf.as_bytes())?;
            out.write_all(b"\n")?;
            buf.clear();
            col = 0;
            last_space_buf_end = None;
        }

        buf.push(ch);
        col = match ch {
            '\t' => (col / TAB_STOP + 1) * TAB_STOP,
            '\x08' => col.saturating_sub(1),
            _ => col + 1,
        };

        if ch == ' ' && spaces {
            last_space_buf_end = Some(buf.len());
        }
    }

    // Emit whatever remains (the original newline will be added by the
    // caller via the line terminator).
    if !buf.is_empty() {
        out.write_all(buf.as_bytes())?;
    }

    Ok(())
}

/// Compute the display column contribution of a single character, given the
/// current column.
fn char_advance(col: usize, ch: char) -> usize {
    match ch {
        '\t' => (col / TAB_STOP + 1) * TAB_STOP,
        '\x08' => col.saturating_sub(1),
        _ => col + 1,
    }
}

/// Recompute the display column for a string fragment (used after splitting
/// at a space boundary).
fn recompute_column(s: &str) -> usize {
    let mut col: usize = 0;
    for ch in s.chars() {
        col = char_advance(col, ch);
    }
    col
}

// ============================================================================
// File processing
// ============================================================================

/// Process a single input source, folding each line and writing to `out`.
fn fold_input<R: BufRead, W: Write>(
    reader: &mut R,
    out: &mut W,
    config: &Config,
) -> io::Result<()> {
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break;
        }

        // Strip the trailing newline for processing; we re-add it after
        // folding.
        let has_newline = line_buf.ends_with('\n');
        if has_newline {
            line_buf.pop();
            if line_buf.ends_with('\r') {
                line_buf.pop();
            }
        }

        if config.bytes {
            fold_line_bytes(out, line_buf.as_bytes(), config.width, config.spaces)?;
        } else {
            fold_line_columns(out, &line_buf, config.width, config.spaces)?;
        }

        if has_newline {
            out.write_all(b"\n")?;
        }
    }

    Ok(())
}

/// Process all files in the configuration.
fn run(config: &Config) -> io::Result<i32> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut exit_code = 0;

    for path in &config.file_paths {
        if path == "-" {
            let mut reader = stdin.lock();
            fold_input(&mut reader, &mut out, config)?;
        } else {
            match File::open(path) {
                Ok(f) => {
                    let mut reader = BufReader::new(f);
                    fold_input(&mut reader, &mut out, config)?;
                }
                Err(e) => {
                    eprintln!("fold: {path}: {e}");
                    exit_code = 1;
                }
            }
        }
    }

    out.flush()?;
    Ok(exit_code)
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS fold v{VERSION}");
    println!();
    println!("Wrap input lines in each FILE, writing to standard output.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  fold [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -w, --width=N   Use N columns instead of 80");
    println!("  -s, --spaces    Break at spaces (word wrap, don't split mid-word)");
    println!("  -b, --bytes     Count bytes rather than columns (no tab expansion)");
    println!("      --help      Display this help and exit");
    println!("      --version   Output version information and exit");
    println!();
    println!("Without -b, tabs expand to the next tab stop (every 8 columns)");
    println!("and backspaces decrement the column counter.");
    println!();
    println!("EXAMPLES:");
    println!("  fold file.txt              Wrap at 80 columns");
    println!("  fold -w 40 file.txt        Wrap at 40 columns");
    println!("  fold -s -w 72 file.txt     Word-wrap at 72 columns");
    println!("  fold -b -w 100 file.txt    Wrap at 100 bytes");
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
            println!("fold (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => match run(&config) {
            Ok(code) => process::exit(code),
            Err(e) => {
                eprintln!("fold: {e}");
                process::exit(1);
            }
        },
    }
}
