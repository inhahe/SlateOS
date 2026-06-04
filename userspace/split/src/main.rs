//! OurOS `split` Utility -- Split a File into Pieces
//!
//! Splits a file into fixed-size pieces by line count, byte count, or number
//! of output chunks.  Modeled after POSIX/GNU `split`.
//!
//! # Usage
//!
//! ```text
//! split [OPTION]... [FILE [PREFIX]]
//!
//! Split FILE into pieces. Output pieces are named PREFIXaa, PREFIXab, ...
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -l, --lines=N             Put N lines per output file (default 1000)
//!   -b, --bytes=N             Put N bytes per output file (K/M/G suffixes)
//!   -C, --line-bytes=N        Put at most N bytes per file, breaking at lines
//!   -n, --number=N            Split into N roughly equal files
//!   -a, --suffix-length=N     Use suffixes of length N (default 2)
//!   -d, --numeric-suffixes    Use numeric suffixes (00, 01, ...) not alphabetic
//!       --additional-suffix=S Append S to each output filename
//!   -e, --elide-empty-files   Do not create empty output files
//!       --verbose             Print a diagnostic for each output file opened
//!       --filter=CMD          (Reserved) Write to shell command, not files
//!       --help                Display this help and exit
//!       --version             Output version information and exit
//! ```

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_LINES: u64 = 1000;
const DEFAULT_SUFFIX_LEN: usize = 2;

// ============================================================================
// Split mode
// ============================================================================

/// How the input should be split.
enum Mode {
    /// Split every N lines (default).
    Lines(u64),
    /// Split every N bytes.
    Bytes(u64),
    /// Split into chunks of at most N bytes, breaking at line boundaries.
    LineBytes(u64),
    /// Split into exactly N output files of roughly equal size.
    Number(u64),
}

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Input file path. `-` means stdin.
    input_path: String,
    /// Output filename prefix.
    prefix: String,
    /// How to split.
    mode: Mode,
    /// Length of the generated suffix (e.g. 2 -> aa..zz or 00..99).
    suffix_len: usize,
    /// Use numeric suffixes instead of alphabetic.
    numeric: bool,
    /// Extra string appended after the generated suffix.
    additional_suffix: String,
    /// Suppress creation of empty output files.
    elide_empty: bool,
    /// Print diagnostic to stderr when opening each output file.
    verbose: bool,
    /// Filter command (reserved, not implemented).
    #[allow(dead_code)]
    filter: Option<String>,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help,
    Version,
}

// ============================================================================
// Byte-size parsing (supports K, M, G suffixes)
// ============================================================================

/// Parse a byte-size string such as `"100"`, `"10K"`, `"2M"`, `"1G"`.
/// Returns `None` if the string is not a valid size.
fn parse_byte_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    let (digits, multiplier) = match s.as_bytes().last()? {
        b'K' | b'k' => (&s[..s.len() - 1], 1024_u64),
        b'M' | b'm' => (&s[..s.len() - 1], 1024_u64 * 1024),
        b'G' | b'g' => (&s[..s.len() - 1], 1024_u64 * 1024 * 1024),
        _ => (s, 1_u64),
    };

    let n: u64 = digits.parse().ok()?;
    n.checked_mul(multiplier)
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Consume the value for a flag that expects an argument.  Handles both
/// `--flag=VAL` (returns the part after `=`) and `--flag VAL` (advances `i`
/// and returns `args[i]`).
fn take_value<'a>(
    args: &'a [String],
    i: &mut usize,
    flag: &str,
    eq_val: Option<&'a str>,
) -> Result<&'a str, String> {
    if let Some(v) = eq_val {
        return Ok(v);
    }
    *i += 1;
    if *i >= args.len() {
        return Err(format!("split: option '{flag}' requires an argument"));
    }
    Ok(&args[*i])
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut input_path: Option<String> = None;
    let mut prefix: Option<String> = None;
    let mut mode: Option<Mode> = None;
    let mut suffix_len: Option<usize> = None;
    let mut numeric = false;
    let mut additional_suffix = String::new();
    let mut elide_empty = false;
    let mut verbose = false;
    let mut filter: Option<String> = None;
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        // After `--`, everything is a positional argument.
        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            if input_path.is_none() {
                input_path = Some(arg.clone());
            } else if prefix.is_none() {
                prefix = Some(arg.clone());
            } else {
                eprintln!("split: extra operand '{arg}'");
                eprintln!("Try 'split --help' for more information.");
                process::exit(1);
            }
            i += 1;
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i += 1;
            continue;
        }

        // ---- Long options ----
        if arg.starts_with("--") {
            // Split on first `=` for --key=value forms.
            let (key, eq_val) = match arg.find('=') {
                Some(pos) => (&arg[..pos], Some(&arg[pos + 1..])),
                None => (arg.as_str(), None),
            };

            match key {
                "--help" => return ParseResult::Help,
                "--version" => return ParseResult::Version,
                "--verbose" => verbose = true,
                "--numeric-suffixes" => numeric = true,
                "--elide-empty-files" => elide_empty = true,
                "--lines" => {
                    let v = unwrap_val(take_value(args, &mut i, "--lines", eq_val));
                    let n = parse_positive(v, "--lines");
                    mode = Some(Mode::Lines(n));
                }
                "--bytes" => {
                    let v = unwrap_val(take_value(args, &mut i, "--bytes", eq_val));
                    let n = parse_byte_val(v, "--bytes");
                    mode = Some(Mode::Bytes(n));
                }
                "--line-bytes" => {
                    let v = unwrap_val(take_value(args, &mut i, "--line-bytes", eq_val));
                    let n = parse_byte_val(v, "--line-bytes");
                    mode = Some(Mode::LineBytes(n));
                }
                "--number" => {
                    let v = unwrap_val(take_value(args, &mut i, "--number", eq_val));
                    let n = parse_positive(v, "--number");
                    mode = Some(Mode::Number(n));
                }
                "--suffix-length" => {
                    let v = unwrap_val(take_value(args, &mut i, "--suffix-length", eq_val));
                    let n = parse_positive(v, "--suffix-length");
                    suffix_len = Some(n as usize);
                }
                "--additional-suffix" => {
                    let v = unwrap_val(take_value(args, &mut i, "--additional-suffix", eq_val));
                    additional_suffix = v.to_string();
                }
                "--filter" => {
                    let v = unwrap_val(take_value(args, &mut i, "--filter", eq_val));
                    filter = Some(v.to_string());
                }
                _ => {
                    eprintln!("split: unrecognized option '{arg}'");
                    eprintln!("Try 'split --help' for more information.");
                    process::exit(1);
                }
            }

            i += 1;
            continue;
        }

        // ---- Short options (may be bundled, e.g. `-de`) ----
        let short = &arg[1..];
        let mut chars = short.chars();
        while let Some(ch) = chars.next() {
            match ch {
                'd' => numeric = true,
                'e' => elide_empty = true,
                'l' => {
                    let v = collect_or_next(&mut chars, args, &mut i, "-l");
                    let n = parse_positive(&v, "-l");
                    mode = Some(Mode::Lines(n));
                    break; // consumed remainder
                }
                'b' => {
                    let v = collect_or_next(&mut chars, args, &mut i, "-b");
                    let n = parse_byte_val(&v, "-b");
                    mode = Some(Mode::Bytes(n));
                    break;
                }
                'C' => {
                    let v = collect_or_next(&mut chars, args, &mut i, "-C");
                    let n = parse_byte_val(&v, "-C");
                    mode = Some(Mode::LineBytes(n));
                    break;
                }
                'n' => {
                    let v = collect_or_next(&mut chars, args, &mut i, "-n");
                    let n = parse_positive(&v, "-n");
                    mode = Some(Mode::Number(n));
                    break;
                }
                'a' => {
                    let v = collect_or_next(&mut chars, args, &mut i, "-a");
                    let n = parse_positive(&v, "-a");
                    suffix_len = Some(n as usize);
                    break;
                }
                _ => {
                    eprintln!("split: invalid option -- '{ch}'");
                    eprintln!("Try 'split --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    if filter.is_some() {
        eprintln!("split: --filter is reserved and not yet implemented");
        process::exit(1);
    }

    ParseResult::Run(Config {
        input_path: input_path.unwrap_or_else(|| "-".to_string()),
        prefix: prefix.unwrap_or_else(|| "x".to_string()),
        mode: mode.unwrap_or(Mode::Lines(DEFAULT_LINES)),
        suffix_len: suffix_len.unwrap_or(DEFAULT_SUFFIX_LEN),
        numeric,
        additional_suffix,
        elide_empty,
        verbose,
        filter,
    })
}

/// Collect the remaining chars of the current short-option cluster as the
/// value string.  If nothing remains, consume the next CLI argument.
fn collect_or_next(
    chars: &mut std::str::Chars<'_>,
    args: &[String],
    i: &mut usize,
    flag: &str,
) -> String {
    let remainder: String = chars.collect();
    if !remainder.is_empty() {
        return remainder;
    }
    *i += 1;
    if *i >= args.len() {
        eprintln!("split: option '{flag}' requires an argument");
        eprintln!("Try 'split --help' for more information.");
        process::exit(1);
    }
    args[*i].clone()
}

/// Unwrap a `take_value` result, printing the error and exiting on failure.
fn unwrap_val(r: Result<&str, String>) -> &str {
    match r {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            eprintln!("Try 'split --help' for more information.");
            process::exit(1);
        }
    }
}

/// Parse a string as a positive (> 0) u64.
fn parse_positive(s: &str, flag: &str) -> u64 {
    match s.parse::<u64>() {
        Ok(n) if n > 0 => n,
        _ => {
            eprintln!("split: invalid number of {flag}: '{s}'");
            process::exit(1);
        }
    }
}

/// Parse a byte-size string (with optional K/M/G suffix) that must be > 0.
fn parse_byte_val(s: &str, flag: &str) -> u64 {
    match parse_byte_size(s) {
        Some(n) if n > 0 => n,
        _ => {
            eprintln!("split: invalid number of bytes for {flag}: '{s}'");
            process::exit(1);
        }
    }
}

// ============================================================================
// Suffix generation
// ============================================================================

/// Generate the suffix string for file index `idx` with the given length and
/// style.  Returns `None` if the index exceeds the suffix space (e.g. > 675
/// for 2-char alphabetic).
fn make_suffix(idx: u64, len: usize, numeric: bool) -> Option<String> {
    if numeric {
        let s = format!("{idx}");
        if s.len() > len {
            return None;
        }
        // Zero-pad to `len` digits.
        Some(format!("{idx:0>width$}", width = len))
    } else {
        // Alphabetic: treat `idx` as a base-26 number with digits a..z.
        let mut buf = vec![0u8; len];
        let mut remaining = idx;
        for pos in (0..len).rev() {
            buf[pos] = b'a' + (remaining % 26) as u8;
            remaining /= 26;
        }
        if remaining > 0 {
            // Index exceeded the suffix space.
            return None;
        }
        // SAFETY: every byte is in b'a'..=b'z' or b'0'..=b'9', all valid ASCII.
        Some(String::from_utf8(buf).expect("suffix is ASCII"))
    }
}

/// Build the full output filename for piece `idx`.
fn output_name(config: &Config, idx: u64) -> Option<String> {
    let suffix = make_suffix(idx, config.suffix_len, config.numeric)?;
    Some(format!("{}{}{}", config.prefix, suffix, config.additional_suffix))
}

// ============================================================================
// Output file helper
// ============================================================================

/// Create (or truncate) an output file and optionally print a diagnostic.
fn open_output(path: &str, verbose: bool) -> io::Result<File> {
    if verbose {
        eprintln!("creating file '{path}'");
    }
    File::create(path)
}

// ============================================================================
// Split implementations
// ============================================================================

/// Split by line count (`-l`).
fn split_by_lines(config: &Config, reader: &mut dyn BufRead) -> io::Result<()> {
    let chunk_lines = match config.mode {
        Mode::Lines(n) => n,
        _ => unreachable!(),
    };

    let mut file_idx: u64 = 0;
    let mut line_count: u64 = 0;
    let mut out: Option<File> = None;
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break; // EOF
        }

        // Start a new output file every `chunk_lines` lines.
        if line_count.is_multiple_of(chunk_lines) {
            // Flush previous file (drop closes it).
            drop(out.take());
            let name = output_name(config, file_idx).ok_or_else(|| {
                io::Error::other("output file suffixes exhausted")
            })?;
            out = Some(open_output(&name, config.verbose)?);
            file_idx += 1;
        }

        if let Some(ref mut f) = out {
            f.write_all(line_buf.as_bytes())?;
        }
        line_count += 1;
    }

    Ok(())
}

/// Split by byte count (`-b`).
fn split_by_bytes(config: &Config, reader: &mut dyn Read) -> io::Result<()> {
    let chunk_bytes = match config.mode {
        Mode::Bytes(n) => n,
        _ => unreachable!(),
    };

    let mut file_idx: u64 = 0;
    let mut bytes_in_chunk: u64 = 0;
    let mut out: Option<File> = None;
    let mut buf = [0u8; 8192];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break; // EOF
        }

        let mut offset = 0;
        while offset < n {
            // Open a new file if needed.
            if out.is_none() {
                let name = output_name(config, file_idx).ok_or_else(|| {
                    io::Error::other("output file suffixes exhausted")
                })?;
                out = Some(open_output(&name, config.verbose)?);
                file_idx += 1;
                bytes_in_chunk = 0;
            }

            let remaining_in_chunk = chunk_bytes - bytes_in_chunk;
            let available = (n - offset) as u64;
            let to_write = remaining_in_chunk.min(available) as usize;

            if let Some(ref mut f) = out {
                f.write_all(&buf[offset..offset + to_write])?;
            }

            offset += to_write;
            bytes_in_chunk += to_write as u64;

            if bytes_in_chunk >= chunk_bytes {
                drop(out.take());
            }
        }
    }

    Ok(())
}

/// Split by line-bytes (`-C`): at most N bytes per file, but break at line
/// boundaries whenever possible.
fn split_by_line_bytes(config: &Config, reader: &mut dyn BufRead) -> io::Result<()> {
    let max_bytes = match config.mode {
        Mode::LineBytes(n) => n,
        _ => unreachable!(),
    };

    let mut file_idx: u64 = 0;
    let mut bytes_in_chunk: u64 = 0;
    let mut out: Option<File> = None;
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break; // EOF
        }

        let line_bytes = line_buf.len() as u64;

        // If the current chunk has content and adding this line would exceed
        // the limit, start a new file.
        if bytes_in_chunk > 0 && bytes_in_chunk + line_bytes > max_bytes {
            drop(out.take());
            bytes_in_chunk = 0;
        }

        // Open a new file if needed.
        if out.is_none() {
            let name = output_name(config, file_idx).ok_or_else(|| {
                io::Error::other("output file suffixes exhausted")
            })?;
            out = Some(open_output(&name, config.verbose)?);
            file_idx += 1;
        }

        // If a single line exceeds max_bytes, we must still write it (the line
        // is atomic in -C mode per GNU behavior).  If it fits, just write it.
        if let Some(ref mut f) = out {
            f.write_all(line_buf.as_bytes())?;
        }
        bytes_in_chunk += line_bytes;

        // If we have reached or exceeded the limit, close the file so the next
        // line starts a fresh one.
        if bytes_in_chunk >= max_bytes {
            drop(out.take());
            bytes_in_chunk = 0;
        }
    }

    Ok(())
}

/// Split into N roughly equal files (`-n`).
///
/// Reads the entire input into memory to determine total size, then writes
/// ceil(total / N) bytes to each output file.  Stdin is fully buffered.
fn split_by_number(config: &Config, reader: &mut dyn Read) -> io::Result<()> {
    let num_files = match config.mode {
        Mode::Number(n) => n,
        _ => unreachable!(),
    };

    // Read entire input.
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;

    let total = data.len() as u64;
    // Size of each chunk (last one may be smaller).
    let chunk_size = if total == 0 { 0 } else { total.div_ceil(num_files) };

    let mut offset: u64 = 0;
    for idx in 0..num_files {
        let start = offset.min(total) as usize;
        let end = (offset + chunk_size).min(total) as usize;
        let piece = &data[start..end];

        if piece.is_empty() && config.elide_empty {
            offset = total;
            continue;
        }

        let name = output_name(config, idx).ok_or_else(|| {
            io::Error::other("output file suffixes exhausted")
        })?;
        let mut f = open_output(&name, config.verbose)?;
        f.write_all(piece)?;

        offset += chunk_size;
    }

    Ok(())
}

/// Remove empty output files that were created during line-based or byte-based
/// splitting.  This is a post-pass used with `-e` for modes other than `-n`
/// (which handles elision inline).
fn elide_empty_files(config: &Config, count: u64) {
    for idx in 0..count {
        if let Some(name) = output_name(config, idx)
            && let Ok(meta) = fs::metadata(&name)
                && meta.len() == 0 {
                    let _ = fs::remove_file(&name);
                }
    }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS split v{VERSION}");
    println!();
    println!("Split FILE into pieces. Output pieces are named PREFIXaa, PREFIXab, ...");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  split [OPTION]... [FILE [PREFIX]]");
    println!();
    println!("OPTIONS:");
    println!("  -l, --lines=N             Put N lines per output file (default 1000)");
    println!("  -b, --bytes=N             Put N bytes per output file (K/M/G suffixes)");
    println!("  -C, --line-bytes=N        At most N bytes per file, breaking at lines");
    println!("  -n, --number=N            Split into N roughly equal files");
    println!("  -a, --suffix-length=N     Use suffixes of length N (default 2)");
    println!("  -d, --numeric-suffixes    Use numeric suffixes (00, 01, ...) not aa, ab, ...");
    println!("      --additional-suffix=S Append S to each output filename");
    println!("  -e, --elide-empty-files   Do not create empty output files");
    println!("      --verbose             Print diagnostic when each file is opened");
    println!("      --filter=CMD          (Reserved -- not yet implemented)");
    println!("      --help                Display this help and exit");
    println!("      --version             Output version information and exit");
    println!();
    println!("SIZE SUFFIXES:");
    println!("  K = 1024, M = 1048576, G = 1073741824");
    println!();
    println!("EXAMPLES:");
    println!("  split largefile            Split into 1000-line pieces xaa, xab, ...");
    println!("  split -l 500 data chunk_   500 lines per file, prefix 'chunk_'");
    println!("  split -b 10M bigfile       10 MiB per file");
    println!("  split -n 4 archive         Split into 4 roughly equal parts");
    println!("  split -d -a 3 log          Numeric suffixes: log000, log001, ...");
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
            println!("split (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let result = run_split(&config);
            match result {
                Ok(()) => process::exit(0),
                Err(e) => {
                    eprintln!("split: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

/// Open the input source and dispatch to the appropriate split strategy.
fn run_split(config: &Config) -> io::Result<()> {
    let stdin = io::stdin();

    match config.mode {
        Mode::Lines(_) => {
            let mut reader: Box<dyn BufRead> = if config.input_path == "-" {
                Box::new(stdin.lock())
            } else {
                let f = File::open(&config.input_path).map_err(|e| {
                    io::Error::new(e.kind(), format!("{}: {e}", config.input_path))
                })?;
                Box::new(BufReader::new(f))
            };
            split_by_lines(config, &mut *reader)?;
            if config.elide_empty {
                // Count how many files could have been created (upper bound).
                // Walk suffix space until the file doesn't exist.
                let count = count_existing_outputs(config);
                elide_empty_files(config, count);
            }
        }
        Mode::Bytes(_) => {
            let mut reader: Box<dyn Read> = if config.input_path == "-" {
                Box::new(stdin.lock())
            } else {
                let f = File::open(&config.input_path).map_err(|e| {
                    io::Error::new(e.kind(), format!("{}: {e}", config.input_path))
                })?;
                Box::new(BufReader::new(f))
            };
            split_by_bytes(config, &mut *reader)?;
            if config.elide_empty {
                let count = count_existing_outputs(config);
                elide_empty_files(config, count);
            }
        }
        Mode::LineBytes(_) => {
            let mut reader: Box<dyn BufRead> = if config.input_path == "-" {
                Box::new(stdin.lock())
            } else {
                let f = File::open(&config.input_path).map_err(|e| {
                    io::Error::new(e.kind(), format!("{}: {e}", config.input_path))
                })?;
                Box::new(BufReader::new(f))
            };
            split_by_line_bytes(config, &mut *reader)?;
            if config.elide_empty {
                let count = count_existing_outputs(config);
                elide_empty_files(config, count);
            }
        }
        Mode::Number(_) => {
            let mut reader: Box<dyn Read> = if config.input_path == "-" {
                Box::new(stdin.lock())
            } else {
                let f = File::open(&config.input_path).map_err(|e| {
                    io::Error::new(e.kind(), format!("{}: {e}", config.input_path))
                })?;
                Box::new(BufReader::new(f))
            };
            split_by_number(config, &mut *reader)?;
        }
    }

    Ok(())
}

/// Count how many output files exist by probing sequential suffix indices.
fn count_existing_outputs(config: &Config) -> u64 {
    let mut idx: u64 = 0;
    while let Some(name) = output_name(config, idx) {
        if fs::metadata(&name).is_err() {
            break;
        }
        idx += 1;
    }
    idx
}
