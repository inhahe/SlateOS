//! OurOS `comm` Utility -- Compare Two Sorted Files Line by Line
//!
//! Reads two sorted files and produces three-column output:
//! - Column 1: lines unique to file1
//! - Column 2: lines unique to file2
//! - Column 3: lines common to both files
//!
//! # Usage
//!
//! ```text
//! comm [OPTION]... FILE1 FILE2
//!
//! Compare sorted files FILE1 and FILE2 line by line.
//!
//!   -1                       Suppress column 1 (lines unique to FILE1)
//!   -2                       Suppress column 2 (lines unique to FILE2)
//!   -3                       Suppress column 3 (lines common to both)
//!   -i, --case-insensitive   Case-insensitive comparison
//!       --check-order        Check that input is correctly sorted (default)
//!       --nocheck-order      Do not check input sort order
//!       --output-delimiter=STR  Use STR as the output column delimiter
//!   -z, --zero-terminated    Line delimiter is NUL, not newline
//!       --json               Output as JSON array
//!       --help               Display this help and exit
//!       --version            Output version information and exit
//!
//! Use `-` as FILE1 or FILE2 to read from standard input.
//! ```

use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Path for file1. `"-"` means stdin.
    file1: String,
    /// Path for file2. `"-"` means stdin.
    file2: String,
    /// Suppress column 1 (lines unique to file1).
    suppress_col1: bool,
    /// Suppress column 2 (lines unique to file2).
    suppress_col2: bool,
    /// Suppress column 3 (lines common to both).
    suppress_col3: bool,
    /// Case-insensitive comparison.
    case_insensitive: bool,
    /// Whether to verify that inputs are sorted.
    check_order: bool,
    /// Column delimiter string (default: TAB).
    output_delimiter: String,
    /// Use NUL instead of newline as line terminator.
    zero_terminated: bool,
    /// Output as JSON.
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

fn parse_args(args: &[String]) -> ParseResult {
    let mut suppress_col1 = false;
    let mut suppress_col2 = false;
    let mut suppress_col3 = false;
    let mut case_insensitive = false;
    let mut check_order = true;
    let mut output_delimiter: Option<String> = None;
    let mut zero_terminated = false;
    let mut json = false;
    let mut positionals: Vec<String> = Vec::new();
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            positionals.push(arg.clone());
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
            if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--case-insensitive" {
                case_insensitive = true;
            } else if arg == "--check-order" {
                check_order = true;
            } else if arg == "--nocheck-order" {
                check_order = false;
            } else if arg == "--zero-terminated" {
                zero_terminated = true;
            } else if arg == "--json" {
                json = true;
            } else if arg.starts_with("--output-delimiter=") {
                if let Some(val) = arg.strip_prefix("--output-delimiter=") {
                    output_delimiter = Some(val.to_string());
                }
            } else if arg == "--output-delimiter" {
                i += 1;
                if i >= args.len() {
                    eprintln!("comm: option '--output-delimiter' requires an argument");
                    process::exit(1);
                }
                output_delimiter = Some(args[i].clone());
            } else {
                eprintln!("comm: unrecognized option '{arg}'");
                eprintln!("Try 'comm --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options -- may be clustered (e.g., `-12`, `-123i`, `-iz`).
        let arg_bytes = arg.as_bytes();
        let mut j = 1;
        while j < arg_bytes.len() {
            match arg_bytes[j] {
                b'1' => suppress_col1 = true,
                b'2' => suppress_col2 = true,
                b'3' => suppress_col3 = true,
                b'i' => case_insensitive = true,
                b'z' => zero_terminated = true,
                _ => {
                    let ch = arg_bytes[j] as char;
                    eprintln!("comm: invalid option -- '{ch}'");
                    eprintln!("Try 'comm --help' for more information.");
                    process::exit(1);
                }
            }
            j += 1;
        }

        i += 1;
    }

    if positionals.len() != 2 {
        eprintln!(
            "comm: expected 2 file operands, got {}",
            positionals.len()
        );
        eprintln!("Try 'comm --help' for more information.");
        process::exit(1);
    }

    if positionals[0] == "-" && positionals[1] == "-" {
        eprintln!("comm: only one file operand may be '-' (stdin)");
        process::exit(1);
    }

    let delimiter = output_delimiter.unwrap_or_else(|| "\t".to_string());

    ParseResult::Run(Config {
        file1: positionals[0].clone(),
        file2: positionals[1].clone(),
        suppress_col1,
        suppress_col2,
        suppress_col3,
        case_insensitive,
        check_order,
        output_delimiter: delimiter,
        zero_terminated,
        json,
    })
}

// ============================================================================
// Line reading
// ============================================================================

/// A buffered line reader that yields lines split by newline or NUL,
/// tracking the previous line for sort-order checking.
struct LineReader {
    lines: Vec<String>,
    index: usize,
}

impl LineReader {
    /// Create a `LineReader` from a buffered reader.
    fn from_reader(
        reader: Box<dyn BufRead>,
        zero_terminated: bool,
    ) -> io::Result<Self> {
        let mut lines = Vec::new();
        if zero_terminated {
            let mut buf_reader = reader;
            let mut buf = Vec::new();
            loop {
                buf.clear();
                let n = read_until_byte(&mut buf_reader, 0, &mut buf)?;
                if n == 0 {
                    break;
                }
                // Strip the trailing NUL delimiter if present.
                if buf.last() == Some(&0) {
                    buf.pop();
                }
                lines.push(String::from_utf8_lossy(&buf).into_owned());
            }
        } else {
            let buf_reader = reader;
            for line_result in buf_reader.lines() {
                lines.push(line_result?);
            }
        }
        Ok(Self { lines, index: 0 })
    }

    /// Return the next line, or `None` if exhausted.
    fn next_line(&mut self) -> Option<&str> {
        if self.index < self.lines.len() {
            let line = &self.lines[self.index];
            self.index += 1;
            Some(line)
        } else {
            None
        }
    }
}

/// Read bytes from `reader` until `delim` is found or EOF. Appends to `buf`
/// including the delimiter byte. Returns the number of bytes read.
fn read_until_byte(
    reader: &mut dyn BufRead,
    delim: u8,
    buf: &mut Vec<u8>,
) -> io::Result<usize> {
    let mut total = 0;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(total);
        }
        if let Some(pos) = available.iter().position(|&b| b == delim) {
            let count = pos + 1;
            buf.extend_from_slice(&available[..count]);
            reader.consume(count);
            total += count;
            return Ok(total);
        }
        let len = available.len();
        buf.extend_from_slice(available);
        reader.consume(len);
        total += len;
    }
}

// ============================================================================
// Comparison helpers
// ============================================================================

/// Compare two lines, optionally case-insensitive.
fn compare_lines(a: &str, b: &str, case_insensitive: bool) -> Ordering {
    if case_insensitive {
        let a_lower = a.to_ascii_lowercase();
        let b_lower = b.to_ascii_lowercase();
        a_lower.cmp(&b_lower)
    } else {
        a.cmp(b)
    }
}

/// Check if `current` is sorted relative to `prev` (i.e., `prev <= current`).
/// Returns `true` if order is correct.
fn is_sorted_pair(prev: &str, current: &str, case_insensitive: bool) -> bool {
    compare_lines(prev, current, case_insensitive) != Ordering::Greater
}

// ============================================================================
// JSON output helpers
// ============================================================================

/// Escape a string for safe inclusion in JSON.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
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
    out
}

// ============================================================================
// Core merge + output
// ============================================================================

/// Which column a line belongs to.
enum Column {
    /// Unique to file1.
    Only1,
    /// Unique to file2.
    Only2,
    /// Common to both.
    Common,
}

/// Write one line to the appropriate column in the default tab-delimited format.
fn write_line(
    out: &mut dyn Write,
    config: &Config,
    col: &Column,
    line: &str,
) -> io::Result<()> {
    let (suppressed, prefix_count) = match col {
        Column::Only1 => (config.suppress_col1, 0_usize),
        Column::Only2 => (config.suppress_col2, 1_usize),
        Column::Common => (config.suppress_col3, 2_usize),
    };

    if suppressed {
        return Ok(());
    }

    // The prefix count is the column index, but suppressed earlier columns
    // shift everything left. Count how many non-suppressed columns come
    // before this one.
    let mut actual_prefix = 0_usize;
    match col {
        Column::Only1 => {
            // Column 1 is always leftmost, no prefix.
            actual_prefix = 0;
        }
        Column::Only2 => {
            // Preceded by column 1 if not suppressed.
            if !config.suppress_col1 {
                actual_prefix = 1;
            }
        }
        Column::Common => {
            // Preceded by columns 1 and 2 if not suppressed.
            if !config.suppress_col1 {
                actual_prefix += 1;
            }
            if !config.suppress_col2 {
                actual_prefix += 1;
            }
        }
    }
    let _ = prefix_count; // Used conceptual reference only.

    for _ in 0..actual_prefix {
        out.write_all(config.output_delimiter.as_bytes())?;
    }
    out.write_all(line.as_bytes())?;
    if config.zero_terminated {
        out.write_all(b"\0")?;
    } else {
        out.write_all(b"\n")?;
    }

    Ok(())
}

/// Entry for JSON output.
struct JsonEntry {
    column: u8,
    line: String,
}

/// Perform the merge of two sorted iterators and write output.
fn merge_and_output(
    config: &Config,
    reader1: &mut LineReader,
    reader2: &mut LineReader,
    out: &mut dyn Write,
) -> io::Result<i32> {
    let mut exit_code = 0;
    let mut prev1: Option<String> = None;
    let mut prev2: Option<String> = None;

    // For JSON mode, collect all entries first.
    let mut json_entries: Vec<JsonEntry> = Vec::new();

    let mut line1 = reader1.next_line().map(String::from);
    let mut line2 = reader2.next_line().map(String::from);

    loop {
        match (&line1, &line2) {
            (None, None) => break,

            (Some(l1), None) => {
                // Check sort order for file1.
                if config.check_order
                    && let Some(ref p) = prev1
                        && !is_sorted_pair(p, l1, config.case_insensitive) {
                            eprintln!(
                                "comm: file 1 is not in sorted order"
                            );
                            exit_code = 1;
                        }
                if config.json {
                    json_entries.push(JsonEntry { column: 1, line: l1.clone() });
                } else {
                    write_line(out, config, &Column::Only1, l1)?;
                }
                prev1 = Some(l1.clone());
                line1 = reader1.next_line().map(String::from);
            }

            (None, Some(l2)) => {
                // Check sort order for file2.
                if config.check_order
                    && let Some(ref p) = prev2
                        && !is_sorted_pair(p, l2, config.case_insensitive) {
                            eprintln!(
                                "comm: file 2 is not in sorted order"
                            );
                            exit_code = 1;
                        }
                if config.json {
                    json_entries.push(JsonEntry { column: 2, line: l2.clone() });
                } else {
                    write_line(out, config, &Column::Only2, l2)?;
                }
                prev2 = Some(l2.clone());
                line2 = reader2.next_line().map(String::from);
            }

            (Some(l1), Some(l2)) => {
                // Check sort order for both files.
                if config.check_order {
                    if let Some(ref p) = prev1
                        && !is_sorted_pair(p, l1, config.case_insensitive) {
                            eprintln!(
                                "comm: file 1 is not in sorted order"
                            );
                            exit_code = 1;
                        }
                    if let Some(ref p) = prev2
                        && !is_sorted_pair(p, l2, config.case_insensitive) {
                            eprintln!(
                                "comm: file 2 is not in sorted order"
                            );
                            exit_code = 1;
                        }
                }

                match compare_lines(l1, l2, config.case_insensitive) {
                    Ordering::Less => {
                        if config.json {
                            json_entries.push(JsonEntry { column: 1, line: l1.clone() });
                        } else {
                            write_line(out, config, &Column::Only1, l1)?;
                        }
                        prev1 = Some(l1.clone());
                        line1 = reader1.next_line().map(String::from);
                    }
                    Ordering::Greater => {
                        if config.json {
                            json_entries.push(JsonEntry { column: 2, line: l2.clone() });
                        } else {
                            write_line(out, config, &Column::Only2, l2)?;
                        }
                        prev2 = Some(l2.clone());
                        line2 = reader2.next_line().map(String::from);
                    }
                    Ordering::Equal => {
                        if config.json {
                            json_entries.push(JsonEntry { column: 3, line: l1.clone() });
                        } else {
                            write_line(out, config, &Column::Common, l1)?;
                        }
                        prev1 = Some(l1.clone());
                        prev2 = Some(l2.clone());
                        line1 = reader1.next_line().map(String::from);
                        line2 = reader2.next_line().map(String::from);
                    }
                }
            }
        }
    }

    // Write JSON output if in JSON mode.
    if config.json {
        write_json_output(out, config, &json_entries)?;
    }

    Ok(exit_code)
}

/// Write collected entries as a JSON array.
fn write_json_output(
    out: &mut dyn Write,
    config: &Config,
    entries: &[JsonEntry],
) -> io::Result<()> {
    let terminator: &[u8] = if config.zero_terminated { b"\0" } else { b"\n" };

    out.write_all(b"[")?;
    out.write_all(terminator)?;

    let total = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        let col_name = match entry.column {
            1 => "unique_to_file1",
            2 => "unique_to_file2",
            _ => "common",
        };
        let escaped = json_escape(&entry.line);
        write!(
            out,
            "  {{\"column\": \"{col_name}\", \"line\": \"{escaped}\"}}"
        )?;
        if idx + 1 < total {
            out.write_all(b",")?;
        }
        out.write_all(terminator)?;
    }

    out.write_all(b"]")?;
    out.write_all(terminator)?;

    Ok(())
}

// ============================================================================
// File opening
// ============================================================================

/// Open a file for reading, or return stdin if path is `"-"`.
fn open_input(path: &str) -> io::Result<Box<dyn BufRead>> {
    if path == "-" {
        Ok(Box::new(BufReader::new(io::stdin())))
    } else {
        let file = File::open(path)?;
        Ok(Box::new(BufReader::new(file)))
    }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS comm v{VERSION}");
    println!();
    println!("Compare two sorted files line by line.");
    println!();
    println!("USAGE:");
    println!("  comm [OPTION]... FILE1 FILE2");
    println!();
    println!("With no options, produce three-column output:");
    println!("  Column 1: lines unique to FILE1");
    println!("  Column 2: lines unique to FILE2");
    println!("  Column 3: lines common to both files");
    println!();
    println!("COLUMN SUPPRESSION:");
    println!("  -1                       Suppress column 1 (lines unique to FILE1)");
    println!("  -2                       Suppress column 2 (lines unique to FILE2)");
    println!("  -3                       Suppress column 3 (lines common to both)");
    println!();
    println!("COMPARISON OPTIONS:");
    println!("  -i, --case-insensitive   Case-insensitive comparison");
    println!("      --check-order        Check that input is correctly sorted (default)");
    println!("      --nocheck-order      Do not check input sort order");
    println!();
    println!("OUTPUT OPTIONS:");
    println!("      --output-delimiter=STR  Use STR as the output column delimiter");
    println!("  -z, --zero-terminated    Line delimiter is NUL, not newline");
    println!("      --json               Output as JSON array");
    println!();
    println!("OTHER OPTIONS:");
    println!("      --help               Display this help and exit");
    println!("      --version            Output version information and exit");
    println!();
    println!("Use '-' as FILE1 or FILE2 to read standard input.");
    println!();
    println!("EXAMPLES:");
    println!("  comm file1 file2           Show all three columns");
    println!("  comm -12 file1 file2       Show only common lines");
    println!("  comm -23 file1 file2       Show only lines unique to file1");
    println!("  comm -3 file1 file2        Show unique lines from both files");
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
            println!("comm (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}

/// Execute the comm operation. Returns the process exit code.
fn run(config: &Config) -> i32 {
    // Open both input files.
    let reader1_buf = match open_input(&config.file1) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("comm: {}: {e}", config.file1);
            return 1;
        }
    };

    let reader2_buf = match open_input(&config.file2) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("comm: {}: {e}", config.file2);
            return 1;
        }
    };

    // Build line readers.
    let mut reader1 = match LineReader::from_reader(reader1_buf, config.zero_terminated) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("comm: {}: read error: {e}", config.file1);
            return 1;
        }
    };

    let mut reader2 = match LineReader::from_reader(reader2_buf, config.zero_terminated) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("comm: {}: read error: {e}", config.file2);
            return 1;
        }
    };

    // Buffered stdout for output.
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    // Perform the merge.
    let exit_code = match merge_and_output(config, &mut reader1, &mut reader2, &mut out) {
        Ok(code) => code,
        Err(e) => {
            if e.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("comm: write error: {e}");
            }
            1
        }
    };

    // Flush output.
    if let Err(e) = out.flush()
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("comm: write error: {e}");
            return 1;
        }

    exit_code
}
