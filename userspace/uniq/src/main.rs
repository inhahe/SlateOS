//! OurOS `uniq` Utility -- Filter Adjacent Duplicate Lines
//!
//! Reads lines from input (file or stdin) and filters adjacent matching lines,
//! writing the result to output (file or stdout). Typically used with sorted
//! input to remove all duplicates.
//!
//! # Usage
//!
//! ```text
//! uniq [OPTION]... [INPUT [OUTPUT]]
//!
//! Filter adjacent matching lines from INPUT (or standard input),
//! writing to OUTPUT (or standard output).
//!
//!   -c, --count                  Prefix lines by the number of occurrences
//!   -d, --repeated               Only print duplicate lines, one for each group
//!   -D, --all-repeated[=METHOD]  Print all duplicate lines;
//!                                METHOD={none,prepend,separate} (default: none)
//!   -u, --unique                 Only print unique lines
//!   -f, --skip-fields=N          Avoid comparing the first N fields
//!   -s, --skip-chars=N           Avoid comparing the first N characters
//!   -w, --check-chars=N          Compare no more than N characters
//!   -i, --ignore-case            Ignore differences in case when comparing
//!   -z, --zero-terminated        Line delimiter is NUL, not newline
//!       --group[=METHOD]         Show all lines, separating groups with blank
//!                                lines; METHOD={separate,prepend,append,both}
//!       --json                   Output JSON with count and line
//!       --help                   Display this help and exit
//!       --version                Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Configuration types
// ============================================================================

/// Controls how `-D` / `--all-repeated` separates groups.
#[derive(Clone, Copy, PartialEq)]
enum AllRepeatedMethod {
    /// No blank lines between groups.
    None,
    /// Blank line before each group (including the first).
    Prepend,
    /// Blank line between groups (not before the first).
    Separate,
}

/// Controls how `--group` separates groups.
#[derive(Clone, Copy, PartialEq)]
enum GroupMethod {
    /// Blank line between groups (not before first, not after last).
    Separate,
    /// Blank line before each group (including the first).
    Prepend,
    /// Blank line after each group (including the last).
    Append,
    /// Blank line before and after each group.
    Both,
}

/// Output mode -- which lines to print and how.
#[derive(Clone, Copy, PartialEq)]
enum OutputMode {
    /// Default: output first line of each group.
    Default,
    /// `-c`: prefix each line with its count.
    Count,
    /// `-d`: only output groups that appear more than once (first line only).
    Repeated,
    /// `-u`: only output groups that appear exactly once.
    Unique,
    /// `-D`: output every line from groups that appear more than once.
    AllRepeated(AllRepeatedMethod),
    /// `--group`: output every line, with blank-line separators between groups.
    Group(GroupMethod),
    /// `--json`: output JSON objects with count and line.
    Json,
}

/// Fully parsed command-line configuration.
struct Config {
    /// Input file path. `None` means stdin.
    input: Option<String>,
    /// Output file path. `None` means stdout.
    output: Option<String>,
    /// Which lines to print and in what format.
    mode: OutputMode,
    /// Number of fields to skip before comparing.
    skip_fields: usize,
    /// Number of characters to skip before comparing (after field skipping).
    skip_chars: usize,
    /// Maximum number of characters to compare. `None` means all.
    check_chars: Option<usize>,
    /// Case-insensitive comparison.
    ignore_case: bool,
    /// Use NUL as line delimiter instead of newline.
    zero_terminated: bool,
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

fn parse_usize_arg(flag: &str, val: &str) -> usize {
    match val.parse::<usize>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("uniq: invalid number for {flag}: '{val}'");
            process::exit(1);
        }
    }
}

/// Consume the value for a flag that takes an argument. The value may be the
/// remainder of the current short-option cluster, or the next argument.
fn take_value<'a>(
    flag: &str,
    rest: &'a str,
    args: &'a [String],
    i: &mut usize,
) -> &'a str {
    if !rest.is_empty() {
        return rest;
    }
    *i += 1;
    if *i >= args.len() {
        eprintln!("uniq: option '{flag}' requires an argument");
        process::exit(1);
    }
    &args[*i]
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;
    let mut mode: OutputMode = OutputMode::Default;
    let mut skip_fields: usize = 0;
    let mut skip_chars: usize = 0;
    let mut check_chars: Option<usize> = None;
    let mut ignore_case = false;
    let mut zero_terminated = false;
    let mut end_of_opts = false;

    // Track which mode flags have been set to detect conflicts.
    let mut mode_set = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            // Positional arguments: first is input, second is output.
            if input.is_none() {
                input = Some(arg.clone());
            } else if output.is_none() {
                output = Some(arg.clone());
            } else {
                eprintln!("uniq: extra operand '{arg}'");
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

        // Long options.
        if arg.starts_with("--") {
            if arg == "--count" {
                if mode_set {
                    eprintln!("uniq: conflicting output mode flags");
                    process::exit(1);
                }
                mode = OutputMode::Count;
                mode_set = true;
            } else if arg == "--repeated" {
                if mode_set {
                    eprintln!("uniq: conflicting output mode flags");
                    process::exit(1);
                }
                mode = OutputMode::Repeated;
                mode_set = true;
            } else if arg == "--unique" {
                if mode_set {
                    eprintln!("uniq: conflicting output mode flags");
                    process::exit(1);
                }
                mode = OutputMode::Unique;
                mode_set = true;
            } else if arg == "--all-repeated" || arg.starts_with("--all-repeated=") {
                if mode_set {
                    eprintln!("uniq: conflicting output mode flags");
                    process::exit(1);
                }
                let method = if let Some(val) = arg.strip_prefix("--all-repeated=") {
                    parse_all_repeated_method(val)
                } else {
                    AllRepeatedMethod::None
                };
                mode = OutputMode::AllRepeated(method);
                mode_set = true;
            } else if arg == "--group" || arg.starts_with("--group=") {
                if mode_set {
                    eprintln!("uniq: conflicting output mode flags");
                    process::exit(1);
                }
                let method = if let Some(val) = arg.strip_prefix("--group=") {
                    parse_group_method(val)
                } else {
                    GroupMethod::Separate
                };
                mode = OutputMode::Group(method);
                mode_set = true;
            } else if arg == "--json" {
                if mode_set {
                    eprintln!("uniq: conflicting output mode flags");
                    process::exit(1);
                }
                mode = OutputMode::Json;
                mode_set = true;
            } else if arg == "--ignore-case" {
                ignore_case = true;
            } else if arg == "--zero-terminated" {
                zero_terminated = true;
            } else if arg == "--skip-fields" || arg.starts_with("--skip-fields=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--skip-fields=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("uniq: option '--skip-fields' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                skip_fields = parse_usize_arg("--skip-fields", &val);
            } else if arg == "--skip-chars" || arg.starts_with("--skip-chars=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--skip-chars=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("uniq: option '--skip-chars' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                skip_chars = parse_usize_arg("--skip-chars", &val);
            } else if arg == "--check-chars" || arg.starts_with("--check-chars=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--check-chars=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("uniq: option '--check-chars' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                check_chars = Some(parse_usize_arg("--check-chars", &val));
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else {
                eprintln!("uniq: unrecognized option '{arg}'");
                eprintln!("Try 'uniq --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options. Process clustered flags like -cid.
        let arg_bytes = arg.as_bytes();
        let mut j = 1;
        while j < arg_bytes.len() {
            let rest = &arg[j + 1..];
            match arg_bytes[j] {
                b'c' => {
                    if mode_set {
                        eprintln!("uniq: conflicting output mode flags");
                        process::exit(1);
                    }
                    mode = OutputMode::Count;
                    mode_set = true;
                }
                b'd' => {
                    if mode_set {
                        eprintln!("uniq: conflicting output mode flags");
                        process::exit(1);
                    }
                    mode = OutputMode::Repeated;
                    mode_set = true;
                }
                b'D' => {
                    if mode_set {
                        eprintln!("uniq: conflicting output mode flags");
                        process::exit(1);
                    }
                    // -D might optionally consume the rest as the method,
                    // but the standard short form has no inline argument.
                    mode = OutputMode::AllRepeated(AllRepeatedMethod::None);
                    mode_set = true;
                }
                b'u' => {
                    if mode_set {
                        eprintln!("uniq: conflicting output mode flags");
                        process::exit(1);
                    }
                    mode = OutputMode::Unique;
                    mode_set = true;
                }
                b'i' => {
                    ignore_case = true;
                }
                b'z' => {
                    zero_terminated = true;
                }
                b'f' => {
                    let val = take_value("-f", rest, args, &mut i);
                    skip_fields = parse_usize_arg("-f", val);
                    j = arg_bytes.len();
                    continue;
                }
                b's' => {
                    let val = take_value("-s", rest, args, &mut i);
                    skip_chars = parse_usize_arg("-s", val);
                    j = arg_bytes.len();
                    continue;
                }
                b'w' => {
                    let val = take_value("-w", rest, args, &mut i);
                    check_chars = Some(parse_usize_arg("-w", val));
                    j = arg_bytes.len();
                    continue;
                }
                _ => {
                    let ch = arg_bytes[j] as char;
                    eprintln!("uniq: invalid option -- '{ch}'");
                    eprintln!("Try 'uniq --help' for more information.");
                    process::exit(1);
                }
            }
            j += 1;
        }

        i += 1;
    }

    // Validate: --group is incompatible with -c.
    if let OutputMode::Group(_) = mode {
        // --group cannot be combined with -c (GNU uniq rejects this).
    }

    ParseResult::Run(Config {
        input,
        output,
        mode,
        skip_fields,
        skip_chars,
        check_chars,
        ignore_case,
        zero_terminated,
    })
}

fn parse_all_repeated_method(s: &str) -> AllRepeatedMethod {
    match s {
        "none" => AllRepeatedMethod::None,
        "prepend" => AllRepeatedMethod::Prepend,
        "separate" => AllRepeatedMethod::Separate,
        _ => {
            eprintln!("uniq: invalid argument '{s}' for '--all-repeated'");
            eprintln!("Valid arguments are: 'none', 'prepend', 'separate'");
            process::exit(1);
        }
    }
}

fn parse_group_method(s: &str) -> GroupMethod {
    match s {
        "separate" => GroupMethod::Separate,
        "prepend" => GroupMethod::Prepend,
        "append" => GroupMethod::Append,
        "both" => GroupMethod::Both,
        _ => {
            eprintln!("uniq: invalid argument '{s}' for '--group'");
            eprintln!("Valid arguments are: 'separate', 'prepend', 'append', 'both'");
            process::exit(1);
        }
    }
}

// ============================================================================
// Line comparison
// ============================================================================

/// Given a line, return the comparison slice after applying skip-fields,
/// skip-chars, and check-chars transformations.
fn comparison_key(
    line: &str,
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
) -> &str {
    let mut s = line;

    // Skip N whitespace-delimited fields.
    for _ in 0..skip_fields {
        // Skip leading whitespace before the field.
        s = s.trim_start_matches([' ', '\t']);
        // Skip the non-whitespace field content.
        s = s.trim_start_matches(|c: char| c != ' ' && c != '\t');
    }

    // Skip N characters.
    if skip_chars > 0 {
        let byte_offset = s
            .char_indices()
            .nth(skip_chars)
            .map_or(s.len(), |(idx, _)| idx);
        s = &s[byte_offset..];
    }

    // Limit to N characters.
    if let Some(n) = check_chars {
        let byte_end = s
            .char_indices()
            .nth(n)
            .map_or(s.len(), |(idx, _)| idx);
        s = &s[..byte_end];
    }

    s
}

/// Compare two lines for equality using the configured transformations.
fn lines_equal(
    a: &str,
    b: &str,
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
    ignore_case: bool,
) -> bool {
    let ka = comparison_key(a, skip_fields, skip_chars, check_chars);
    let kb = comparison_key(b, skip_fields, skip_chars, check_chars);

    if ignore_case {
        ka.eq_ignore_ascii_case(kb)
    } else {
        ka == kb
    }
}

// ============================================================================
// Input reading
// ============================================================================

/// Read all lines from the input, splitting on the appropriate delimiter.
fn read_lines(
    reader: &mut dyn Read,
    zero_terminated: bool,
) -> io::Result<Vec<String>> {
    if zero_terminated {
        // Split on NUL bytes.
        let mut buf = Vec::new();
        reader.read_to_end(&mut buf)?;
        let lines: Vec<String> = buf
            .split(|&b| b == 0)
            .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
            .collect();
        // Remove trailing empty element if input ended with NUL.
        let mut lines = lines;
        if lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        Ok(lines)
    } else {
        let buf_reader = BufReader::new(reader);
        let mut lines = Vec::new();
        for line_result in buf_reader.lines() {
            lines.push(line_result?);
        }
        Ok(lines)
    }
}

// ============================================================================
// Output formatting helpers
// ============================================================================

/// Write the line terminator (NUL or newline) to the writer.
fn write_terminator(out: &mut dyn Write, zero_terminated: bool) -> io::Result<()> {
    if zero_terminated {
        out.write_all(b"\0")
    } else {
        out.write_all(b"\n")
    }
}

/// Write a blank separator line (only meaningful for newline-terminated mode).
fn write_blank_line(out: &mut dyn Write, zero_terminated: bool) -> io::Result<()> {
    // In zero-terminated mode, an empty NUL-terminated record acts as separator.
    write_terminator(out, zero_terminated)
}

/// Escape a string for safe inclusion in JSON output.
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
                // Control characters as \u00XX.
                let code = c as u32;
                out.push_str(&format!("\\u{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Core processing
// ============================================================================

/// A group of adjacent identical lines.
struct LineGroup {
    /// The first line of the group (used for output).
    line: String,
    /// Number of adjacent occurrences.
    count: usize,
    /// All lines in the group (only populated for modes that need them).
    all_lines: Vec<String>,
}

/// Collect lines into groups of adjacent duplicates.
fn collect_groups(
    lines: Vec<String>,
    skip_fields: usize,
    skip_chars: usize,
    check_chars: Option<usize>,
    ignore_case: bool,
    need_all_lines: bool,
) -> Vec<LineGroup> {
    let mut groups: Vec<LineGroup> = Vec::new();

    for line in lines {
        let matches_prev = groups.last().is_some_and(|g| {
            lines_equal(
                &g.line,
                &line,
                skip_fields,
                skip_chars,
                check_chars,
                ignore_case,
            )
        });

        if matches_prev {
            // Safe: we verified last() is Some via matches_prev.
            if let Some(g) = groups.last_mut() {
                g.count += 1;
                if need_all_lines {
                    g.all_lines.push(line);
                }
            }
        } else {
            let all_lines = if need_all_lines {
                vec![line.clone()]
            } else {
                Vec::new()
            };
            groups.push(LineGroup {
                line,
                count: 1,
                all_lines,
            });
        }
    }

    groups
}

/// Process and output lines according to the configured mode.
fn process(
    config: &Config,
    lines: Vec<String>,
    out: &mut dyn Write,
) -> io::Result<()> {
    let need_all_lines = matches!(
        config.mode,
        OutputMode::AllRepeated(_) | OutputMode::Group(_)
    );

    let groups = collect_groups(
        lines,
        config.skip_fields,
        config.skip_chars,
        config.check_chars,
        config.ignore_case,
        need_all_lines,
    );

    match config.mode {
        OutputMode::Default => {
            for group in &groups {
                out.write_all(group.line.as_bytes())?;
                write_terminator(out, config.zero_terminated)?;
            }
        }

        OutputMode::Count => {
            for group in &groups {
                // GNU uniq uses a 7-character right-aligned count field.
                write!(out, "{:>7} ", group.count)?;
                out.write_all(group.line.as_bytes())?;
                write_terminator(out, config.zero_terminated)?;
            }
        }

        OutputMode::Repeated => {
            for group in &groups {
                if group.count > 1 {
                    out.write_all(group.line.as_bytes())?;
                    write_terminator(out, config.zero_terminated)?;
                }
            }
        }

        OutputMode::Unique => {
            for group in &groups {
                if group.count == 1 {
                    out.write_all(group.line.as_bytes())?;
                    write_terminator(out, config.zero_terminated)?;
                }
            }
        }

        OutputMode::AllRepeated(method) => {
            let mut first_group = true;
            for group in &groups {
                if group.count <= 1 {
                    continue;
                }
                // Separator logic.
                match method {
                    AllRepeatedMethod::None => {}
                    AllRepeatedMethod::Prepend => {
                        write_blank_line(out, config.zero_terminated)?;
                    }
                    AllRepeatedMethod::Separate => {
                        if !first_group {
                            write_blank_line(out, config.zero_terminated)?;
                        }
                    }
                }
                for line in &group.all_lines {
                    out.write_all(line.as_bytes())?;
                    write_terminator(out, config.zero_terminated)?;
                }
                first_group = false;
            }
        }

        OutputMode::Group(method) => {
            let total_groups = groups.len();
            for (idx, group) in groups.iter().enumerate() {
                let is_first = idx == 0;
                let is_last = idx == total_groups - 1;

                // Prepend blank line before this group?
                match method {
                    GroupMethod::Prepend | GroupMethod::Both => {
                        write_blank_line(out, config.zero_terminated)?;
                    }
                    GroupMethod::Separate => {
                        if !is_first {
                            write_blank_line(out, config.zero_terminated)?;
                        }
                    }
                    GroupMethod::Append => {}
                }

                for line in &group.all_lines {
                    out.write_all(line.as_bytes())?;
                    write_terminator(out, config.zero_terminated)?;
                }

                // Append blank line after this group?
                match method {
                    GroupMethod::Append | GroupMethod::Both => {
                        write_blank_line(out, config.zero_terminated)?;
                    }
                    _ => {}
                }

                // For Separate: blank line between groups is handled by the
                // prepend check on the next iteration, so nothing extra here.
                let _ = is_last; // Suppress unused warning; kept for clarity.
            }
        }

        OutputMode::Json => {
            out.write_all(b"[")?;
            write_terminator(out, config.zero_terminated)?;
            let total = groups.len();
            for (idx, group) in groups.iter().enumerate() {
                let escaped = json_escape(&group.line);
                write!(
                    out,
                    "  {{\"count\": {}, \"line\": \"{escaped}\"}}",
                    group.count
                )?;
                if idx + 1 < total {
                    out.write_all(b",")?;
                }
                write_terminator(out, config.zero_terminated)?;
            }
            out.write_all(b"]")?;
            write_terminator(out, config.zero_terminated)?;
        }
    }

    Ok(())
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS uniq v{VERSION}");
    println!();
    println!("Filter adjacent matching lines from INPUT (or standard input),");
    println!("writing to OUTPUT (or standard output).");
    println!();
    println!("USAGE:");
    println!("  uniq [OPTION]... [INPUT [OUTPUT]]");
    println!();
    println!("OUTPUT MODE:");
    println!("  -c, --count                  Prefix lines by the number of occurrences");
    println!("  -d, --repeated               Only print duplicate lines, one for each group");
    println!("  -D, --all-repeated[=METHOD]  Print all duplicate lines");
    println!("                               METHOD={{none,prepend,separate}} (default: none)");
    println!("  -u, --unique                 Only print unique lines");
    println!("      --group[=METHOD]         Show all lines, separating groups with blank lines");
    println!("                               METHOD={{separate,prepend,append,both}} (default: separate)");
    println!("      --json                   Output JSON with count and line");
    println!();
    println!("COMPARISON OPTIONS:");
    println!("  -f, --skip-fields=N          Avoid comparing the first N fields");
    println!("  -s, --skip-chars=N           Avoid comparing the first N characters");
    println!("  -w, --check-chars=N          Compare no more than N characters");
    println!("  -i, --ignore-case            Ignore differences in case when comparing");
    println!();
    println!("OTHER OPTIONS:");
    println!("  -z, --zero-terminated        Line delimiter is NUL, not newline");
    println!("      --help                   Display this help and exit");
    println!("      --version                Output version information and exit");
    println!();
    println!("A field is a run of blanks (usually spaces and/or tabs), then non-blank");
    println!("characters. Fields are skipped before characters.");
    println!();
    println!("EXAMPLES:");
    println!("  sort file | uniq             Remove duplicate lines");
    println!("  sort file | uniq -c          Count occurrences of each line");
    println!("  sort file | uniq -d          Show only duplicated lines");
    println!("  sort file | uniq -u          Show only unique lines");
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
            println!("uniq (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}

/// Execute the uniq operation. Returns the process exit code.
fn run(config: &Config) -> i32 {
    // Open input.
    let mut reader: Box<dyn Read> = match &config.input {
        Some(path) if path != "-" => match File::open(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("uniq: {path}: {e}");
                return 1;
            }
        },
        _ => Box::new(io::stdin()),
    };

    // Read all lines.
    let lines = match read_lines(&mut *reader, config.zero_terminated) {
        Ok(l) => l,
        Err(e) => {
            let source = config.input.as_deref().unwrap_or("stdin");
            eprintln!("uniq: {source}: read error: {e}");
            return 1;
        }
    };

    // Open output.
    let stdout;
    let mut out: Box<dyn Write> = match &config.output {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(io::BufWriter::new(f)),
            Err(e) => {
                eprintln!("uniq: {path}: {e}");
                return 1;
            }
        },
        None => {
            stdout = io::stdout();
            Box::new(io::BufWriter::new(stdout.lock()))
        }
    };

    // Process and write output.
    if let Err(e) = process(config, lines, &mut *out)
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("uniq: write error: {e}");
            return 1;
        }

    // Flush output.
    if let Err(e) = out.flush()
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("uniq: write error: {e}");
            return 1;
        }

    0
}
