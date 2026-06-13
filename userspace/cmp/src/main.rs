//! SlateOS `cmp` Utility -- Byte-by-Byte File Comparison
//!
//! Compares two files byte by byte and reports the first difference, or all
//! differences with `-l`. Supports reading from stdin via `-`, skip offsets,
//! byte limits, and JSON output.
//!
//! # Usage
//!
//! ```text
//! cmp [OPTION]... FILE1 FILE2
//!
//! Compare two files byte by byte.
//!
//!   -l, --verbose             Print all differing bytes
//!   -s, --silent, --quiet     Suppress all output; exit status only
//!   -b, --print-bytes         Print differing bytes alongside octal values
//!   -i <N>, --ignore-initial=<N>       Skip first N bytes of both files
//!   -i <N1:N2>, --ignore-initial=<N1:N2>  Skip N1 bytes of file1, N2 of file2
//!   -n <N>, --bytes=<N>       Compare at most N bytes
//!       --json                JSON output
//!       --help                Display this help and exit
//!       --version             Output version information and exit
//!
//! Use `-` for stdin as one of the files.
//! ```
//!
//! # Exit codes
//!
//! - 0: files are identical
//! - 1: files differ
//! - 2: error occurred

use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Size of each read chunk. 8 KiB balances syscall overhead with memory use.
const CHUNK_SIZE: usize = 8 * 1024;

// ============================================================================
// Configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    path1: String,
    path2: String,
    verbose: bool,
    silent: bool,
    print_bytes: bool,
    skip1: u64,
    skip2: u64,
    max_bytes: Option<u64>,
    json: bool,
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

/// Parse a byte count string. Supports plain decimal numbers only.
fn parse_byte_count(s: &str, opt_name: &str) -> u64 {
    match s.parse::<u64>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("cmp: invalid {opt_name} value '{s}'");
            process::exit(2);
        }
    }
}

/// Parse the `--ignore-initial` value which may be `N` or `N1:N2`.
fn parse_skip(s: &str) -> (u64, u64) {
    if let Some((a, b)) = s.split_once(':') {
        let n1 = parse_byte_count(a, "ignore-initial");
        let n2 = parse_byte_count(b, "ignore-initial");
        (n1, n2)
    } else {
        let n = parse_byte_count(s, "ignore-initial");
        (n, n)
    }
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut verbose = false;
    let mut silent = false;
    let mut print_bytes = false;
    let mut skip1: u64 = 0;
    let mut skip2: u64 = 0;
    let mut max_bytes: Option<u64> = None;
    let mut json = false;
    let mut positional: Vec<String> = Vec::new();

    let mut end_of_opts = false;
    let mut i = 1;

    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || (!arg.starts_with('-') || arg == "-") {
            positional.push(arg.clone());
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
            if arg == "--verbose" {
                verbose = true;
            } else if arg == "--silent" || arg == "--quiet" {
                silent = true;
            } else if arg == "--print-bytes" {
                print_bytes = true;
            } else if arg == "--ignore-initial" {
                i += 1;
                if i >= args.len() {
                    eprintln!("cmp: option '--ignore-initial' requires an argument");
                    process::exit(2);
                }
                let (s1, s2) = parse_skip(&args[i]);
                skip1 = s1;
                skip2 = s2;
            } else if let Some(val) = arg.strip_prefix("--ignore-initial=") {
                let (s1, s2) = parse_skip(val);
                skip1 = s1;
                skip2 = s2;
            } else if arg == "--bytes" {
                i += 1;
                if i >= args.len() {
                    eprintln!("cmp: option '--bytes' requires an argument");
                    process::exit(2);
                }
                max_bytes = Some(parse_byte_count(&args[i], "bytes"));
            } else if let Some(val) = arg.strip_prefix("--bytes=") {
                max_bytes = Some(parse_byte_count(val, "bytes"));
            } else if arg == "--json" {
                json = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else {
                eprintln!("cmp: unrecognized option '{arg}'");
                eprintln!("Try 'cmp --help' for more information.");
                process::exit(2);
            }

            i += 1;
            continue;
        }

        // Short options. Some accept a required value.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'l' => verbose = true,
                's' => silent = true,
                'b' => print_bytes = true,
                'i' => {
                    // -i may have value glued on or as next arg.
                    let rest: String = chars[j + 1..].iter().collect();
                    if !rest.is_empty() {
                        let (s1, s2) = parse_skip(&rest);
                        skip1 = s1;
                        skip2 = s2;
                    } else {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("cmp: option '-i' requires an argument");
                            process::exit(2);
                        }
                        let (s1, s2) = parse_skip(&args[i]);
                        skip1 = s1;
                        skip2 = s2;
                    }
                    j = chars.len();
                    continue;
                }
                'n' => {
                    let rest: String = chars[j + 1..].iter().collect();
                    if !rest.is_empty() {
                        max_bytes = Some(parse_byte_count(&rest, "bytes"));
                    } else {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("cmp: option '-n' requires an argument");
                            process::exit(2);
                        }
                        max_bytes = Some(parse_byte_count(&args[i], "bytes"));
                    }
                    j = chars.len();
                    continue;
                }
                other => {
                    eprintln!("cmp: invalid option -- '{other}'");
                    eprintln!("Try 'cmp --help' for more information.");
                    process::exit(2);
                }
            }
            j += 1;
        }

        i += 1;
    }

    if positional.len() != 2 {
        eprintln!("cmp: requires exactly two file arguments");
        eprintln!("Try 'cmp --help' for more information.");
        process::exit(2);
    }

    ParseResult::Run(Config {
        path1: positional[0].clone(),
        path2: positional[1].clone(),
        verbose,
        silent,
        print_bytes,
        skip1,
        skip2,
        max_bytes,
        json,
    })
}

// ============================================================================
// File opening
// ============================================================================

/// Open a file or stdin based on the path string. Returns a boxed reader.
fn open_input(path: &str) -> Result<Box<dyn Read>, String> {
    if path == "-" {
        Ok(Box::new(io::stdin()))
    } else {
        File::open(path)
            .map(|f| Box::new(f) as Box<dyn Read>)
            .map_err(|e| format!("cmp: {path}: {e}"))
    }
}

/// Skip `n` bytes from a reader by reading and discarding chunks.
fn skip_bytes(reader: &mut dyn Read, n: u64) -> Result<(), String> {
    let mut remaining = n;
    let mut buf = [0u8; CHUNK_SIZE];
    while remaining > 0 {
        let to_read = (remaining as usize).min(CHUNK_SIZE);
        let got = reader
            .read(&mut buf[..to_read])
            .map_err(|e| format!("{e}"))?;
        if got == 0 {
            // Reached EOF before finishing the skip -- that's fine,
            // the comparison will handle the shorter file.
            break;
        }
        remaining -= got as u64;
    }
    Ok(())
}

// ============================================================================
// JSON output helpers
// ============================================================================

/// Escape a string for JSON output. Produces a quoted JSON string.
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
            c if c < '\x20' => {
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ============================================================================
// Comparison engine
// ============================================================================

/// A single byte difference found during comparison.
#[derive(Clone)]
struct Difference {
    byte_number: u64,
    line_number: u64,
    val1: u8,
    val2: u8,
}

/// Outcome of the comparison.
enum CmpResult {
    /// Files are identical (over the compared range).
    Identical,
    /// Files differ. Contains all differences if verbose mode, or just the first.
    Different(Vec<Difference>),
    /// One file is shorter than the other. The caller determines which.
    /// Also includes any differences found before EOF.
    Eof {
        byte_number: u64,
        line_number: u64,
        diffs: Vec<Difference>,
    },
}

/// Compare two readers byte by byte. Returns the comparison result.
///
/// Reads both files in parallel using chunk-based I/O. Tracks byte position
/// and line number (newlines in file1 are counted).
fn compare(
    r1: &mut dyn Read,
    r2: &mut dyn Read,
    verbose: bool,
    max_bytes: Option<u64>,
) -> Result<CmpResult, String> {
    let mut buf1 = [0u8; CHUNK_SIZE];
    let mut buf2 = [0u8; CHUNK_SIZE];

    let mut byte_number: u64 = 0; // 1-based position (incremented before use)
    let mut line_number: u64 = 1; // current line (newlines in file1 advance this)
    let mut diffs: Vec<Difference> = Vec::new();
    let mut bytes_left = max_bytes;

    loop {
        // Determine how many bytes to read this iteration.
        let want = match bytes_left {
            Some(0) => break,
            Some(left) => (left as usize).min(CHUNK_SIZE),
            None => CHUNK_SIZE,
        };

        let n1 = read_full(r1, &mut buf1[..want])
            .map_err(|e| format!("read error: {e}"))?;
        let n2 = read_full(r2, &mut buf2[..want])
            .map_err(|e| format!("read error: {e}"))?;

        let common = n1.min(n2);

        // Compare the overlapping portion byte by byte.
        for idx in 0..common {
            byte_number += 1;
            let b1 = buf1[idx];
            let b2 = buf2[idx];

            if b1 != b2 {
                diffs.push(Difference {
                    byte_number,
                    line_number,
                    val1: b1,
                    val2: b2,
                });
                if !verbose {
                    // In non-verbose mode, we only need the first difference.
                    return Ok(CmpResult::Different(diffs));
                }
            }

            // Track newlines in file1 for line number reporting.
            if b1 == b'\n' {
                line_number += 1;
            }
        }

        // Handle differing lengths.
        if n1 != n2 {
            // Count any remaining newlines in the shorter file's last chunk
            // for accurate line reporting at the EOF point.
            let eof_byte = byte_number + 1;
            let eof_line = line_number;

            return Ok(CmpResult::Eof {
                byte_number: eof_byte,
                line_number: eof_line,
                diffs,
            });
        }

        // Both readers returned the same amount. If less than requested, both
        // hit EOF simultaneously.
        if n1 < want {
            break;
        }

        if let Some(ref mut left) = bytes_left {
            *left -= common as u64;
        }
    }

    if diffs.is_empty() {
        Ok(CmpResult::Identical)
    } else {
        Ok(CmpResult::Different(diffs))
    }
}

/// Read exactly as many bytes as possible into `buf`, retrying on short reads.
/// Returns the total number of bytes read (less than `buf.len()` only at EOF).
fn read_full(reader: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(filled)
}

// ============================================================================
// Output formatting
// ============================================================================

/// Format a byte as a printable character for `-b` mode.
/// Non-printable bytes are shown as escaped sequences or as-is if printable ASCII.
fn byte_display(b: u8) -> String {
    if (0x20..0x7f).contains(&b) {
        let c = b as char;
        format!("{c}")
    } else {
        match b {
            0x00 => "\\0".to_string(),
            0x07 => "\\a".to_string(),
            0x08 => "\\b".to_string(),
            0x09 => "\\t".to_string(),
            0x0a => "\\n".to_string(),
            0x0b => "\\v".to_string(),
            0x0c => "\\f".to_string(),
            0x0d => "\\r".to_string(),
            0x1b => "\\e".to_string(),
            _ => format!("\\x{b:02x}"),
        }
    }
}

/// Print the default (first difference) output.
fn print_default(diff: &Difference, path1: &str, path2: &str) {
    let out = io::stdout();
    let mut w = out.lock();
    let _ = writeln!(
        w,
        "{path1} {path2} differ: byte {}, line {}",
        diff.byte_number, diff.line_number,
    );
}

/// Print verbose (`-l`) output for all differences.
fn print_verbose(diffs: &[Difference], print_bytes_flag: bool) {
    let out = io::stdout();
    let mut w = out.lock();
    for d in diffs {
        if print_bytes_flag {
            let _ = writeln!(
                w,
                "{:>5} {:>3o} {:<4} {:>3o} {}",
                d.byte_number,
                d.val1,
                byte_display(d.val1),
                d.val2,
                byte_display(d.val2),
            );
        } else {
            let _ = writeln!(
                w,
                "{:>5} {:>3o} {:>3o}",
                d.byte_number, d.val1, d.val2,
            );
        }
    }
}

/// Print JSON output for the comparison result.
fn print_json_output(
    result: &CmpResult,
    path1: &str,
    path2: &str,
    shorter_name: Option<&str>,
) {
    let out = io::stdout();
    let mut w = out.lock();

    let _ = write!(w, "{{");
    let _ = write!(w, "\"file1\":{},", json_escape(path1));
    let _ = write!(w, "\"file2\":{},", json_escape(path2));

    match result {
        CmpResult::Identical => {
            let _ = write!(w, "\"identical\":true,\"differences\":[]");
        }
        CmpResult::Different(diffs) => {
            let _ = write!(w, "\"identical\":false,\"differences\":[");
            for (idx, d) in diffs.iter().enumerate() {
                if idx > 0 {
                    let _ = write!(w, ",");
                }
                let _ = write!(
                    w,
                    "{{\"byte\":{},\"line\":{},\"file1_value\":{},\"file2_value\":{}}}",
                    d.byte_number, d.line_number, d.val1, d.val2,
                );
            }
            let _ = write!(w, "]");
        }
        CmpResult::Eof { byte_number, line_number, diffs } => {
            let _ = write!(w, "\"identical\":false,");
            let _ = write!(w, "\"differences\":[");
            for (idx, d) in diffs.iter().enumerate() {
                if idx > 0 {
                    let _ = write!(w, ",");
                }
                let _ = write!(
                    w,
                    "{{\"byte\":{},\"line\":{},\"file1_value\":{},\"file2_value\":{}}}",
                    d.byte_number, d.line_number, d.val1, d.val2,
                );
            }
            let _ = write!(w, "],");
            if let Some(name) = shorter_name {
                let _ = write!(
                    w,
                    "\"eof\":{},\"eof_byte\":{},\"eof_line\":{}",
                    json_escape(name),
                    byte_number,
                    line_number,
                );
            }
        }
    }

    let _ = writeln!(w, "}}");
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS cmp v{VERSION}");
    println!();
    println!("Compare two files byte by byte.");
    println!();
    println!("USAGE:");
    println!("  cmp [OPTION]... FILE1 FILE2");
    println!();
    println!("OPTIONS:");
    println!("  -l, --verbose                 Print all differing bytes");
    println!("  -s, --silent, --quiet         Suppress all output; exit status only");
    println!("  -b, --print-bytes             Print differing bytes alongside octal");
    println!("  -i <N>, --ignore-initial=<N>  Skip first N bytes of both files");
    println!("  -i <N1:N2>                    Skip N1 bytes of file1, N2 of file2");
    println!("  -n <N>, --bytes=<N>           Compare at most N bytes");
    println!("      --json                    JSON output");
    println!("      --help                    Display this help and exit");
    println!("      --version                 Output version information and exit");
    println!();
    println!("Use `-` for stdin as one of the files.");
    println!();
    println!("EXIT STATUS:");
    println!("  0  Files are identical");
    println!("  1  Files differ");
    println!("  2  Error occurred");
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
            println!("cmp (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let exit_code = run_cmp(&config);
            process::exit(exit_code);
        }
    }
}

/// Execute the comparison and produce output. Returns the exit code.
fn run_cmp(config: &Config) -> i32 {
    // Open both inputs.
    let mut r1 = match open_input(&config.path1) {
        Ok(r) => r,
        Err(msg) => {
            if !config.silent {
                eprintln!("{msg}");
            }
            return 2;
        }
    };
    let mut r2 = match open_input(&config.path2) {
        Ok(r) => r,
        Err(msg) => {
            if !config.silent {
                eprintln!("{msg}");
            }
            return 2;
        }
    };

    // Skip initial bytes if requested.
    if config.skip1 > 0
        && let Err(msg) = skip_bytes(r1.as_mut(), config.skip1) {
            if !config.silent {
                eprintln!("cmp: {}: {msg}", config.path1);
            }
            return 2;
        }
    if config.skip2 > 0
        && let Err(msg) = skip_bytes(r2.as_mut(), config.skip2) {
            if !config.silent {
                eprintln!("cmp: {}: {msg}", config.path2);
            }
            return 2;
        }

    // Run the comparison.
    let result = match compare(r1.as_mut(), r2.as_mut(), config.verbose, config.max_bytes) {
        Ok(r) => r,
        Err(msg) => {
            if !config.silent {
                eprintln!("cmp: {msg}");
            }
            return 2;
        }
    };

    // Determine which file is shorter for EOF messages.
    // We need to re-check by trying to read from each to see which had data left.
    // But our compare function already detected this -- we just need to fill in
    // the shorter file name.
    match result {
        CmpResult::Identical => {
            if config.json {
                print_json_output(&result, &config.path1, &config.path2, None);
            }
            0
        }
        CmpResult::Different(ref diffs) => {
            if config.silent {
                return 1;
            }
            if config.json {
                print_json_output(&result, &config.path1, &config.path2, None);
            } else if config.verbose {
                print_verbose(diffs, config.print_bytes);
            } else if let Some(first) = diffs.first() {
                if config.print_bytes {
                    let out = io::stdout();
                    let mut w = out.lock();
                    let _ = writeln!(
                        w,
                        "{} {} differ: byte {}, line {} is {:>3o} {} {:>3o} {}",
                        config.path1,
                        config.path2,
                        first.byte_number,
                        first.line_number,
                        first.val1,
                        byte_display(first.val1),
                        first.val2,
                        byte_display(first.val2),
                    );
                } else {
                    print_default(first, &config.path1, &config.path2);
                }
            }
            1
        }
        CmpResult::Eof {
            byte_number,
            line_number,
            ref diffs,
        } => {
            // Determine which file is shorter by probing each reader for
            // remaining data. The compare() function already consumed the
            // common prefix, so whichever reader is exhausted is the shorter.
            let mut probe1 = [0u8; 1];
            let mut probe2 = [0u8; 1];
            let has_more_1 = r1.read(&mut probe1).unwrap_or(0) > 0;
            let has_more_2 = r2.read(&mut probe2).unwrap_or(0) > 0;

            let shorter_name = if !has_more_1 && has_more_2 {
                &config.path1
            } else if has_more_1 && !has_more_2 {
                &config.path2
            } else {
                // Both exhausted at this point but had different chunk sizes
                // earlier. Default to file1 as the shorter one.
                &config.path1
            };

            if config.silent {
                return 1;
            }

            if config.json {
                print_json_output(
                    &CmpResult::Eof {
                        byte_number,
                        line_number,
                        diffs: diffs.clone(),
                    },
                    &config.path1,
                    &config.path2,
                    Some(shorter_name),
                );
            } else {
                // Print any differences found before EOF.
                if config.verbose && !diffs.is_empty() {
                    print_verbose(diffs, config.print_bytes);
                } else if !config.verbose
                    && let Some(first) = diffs.first() {
                        if config.print_bytes {
                            let out = io::stdout();
                            let mut w = out.lock();
                            let _ = writeln!(
                                w,
                                "{} {} differ: byte {}, line {} is {:>3o} {} {:>3o} {}",
                                config.path1,
                                config.path2,
                                first.byte_number,
                                first.line_number,
                                first.val1,
                                byte_display(first.val1),
                                first.val2,
                                byte_display(first.val2),
                            );
                        } else {
                            print_default(first, &config.path1, &config.path2);
                        }
                    }
                eprintln!(
                    "cmp: EOF on {shorter_name} after byte {}, in line {}",
                    byte_number.saturating_sub(1),
                    line_number,
                );
            }
            1
        }
    }
}
