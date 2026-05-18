//! OurOS `join` Utility -- Join Lines of Two Sorted Files on a Common Field
//!
//! Reads two sorted files and joins lines that share a common join field,
//! similar to an SQL inner join. Both input files must be sorted on the
//! join field.
//!
//! # Usage
//!
//! ```text
//! join [OPTION]... FILE1 FILE2
//!
//! For each pair of input lines with identical join fields, write a line to
//! standard output. The default join field is the first, delimited by blanks.
//!
//!   -1 FIELD             Join on field FIELD of file 1 (1-based)
//!   -j1 FIELD            Same as -1 FIELD
//!   -2 FIELD             Join on field FIELD of file 2 (1-based)
//!   -j2 FIELD            Same as -2 FIELD
//!   -j FIELD             Equivalent to -1 FIELD -2 FIELD
//!   -t CHAR              Use CHAR as input and output field separator
//!   -a FILENUM           Also print unpairable lines from file FILENUM
//!   -v FILENUM           Like -a FILENUM, but suppress joined output lines
//!   -e STRING            Replace missing input fields with STRING
//!   -o FORMAT            Obey FORMAT while constructing output line
//!                        FORMAT is comma-separated list of FILENUM.FIELD
//!                        or the word 'auto'
//!   -i, --ignore-case    Ignore differences in case when comparing fields
//!       --check-order    Check that the input is correctly sorted
//!       --nocheck-order  Do not check input sort order
//!       --header         Treat the first line in each file as a header
//!       --json           Output as JSON array
//!       --help           Display this help and exit
//!       --version        Output version information and exit
//!
//! Use '-' as FILE1 or FILE2 to read from standard input.
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

/// A single output-format specifier: file number (1 or 2) and field index
/// (1-based). `0.0` is a special sentinel meaning "the join field".
struct OutputSpec {
    file: u8,
    field: usize,
}

/// Fully parsed command-line configuration.
struct Config {
    file1: String,
    file2: String,
    /// Join field for file1 (1-based, default 1).
    field1: usize,
    /// Join field for file2 (1-based, default 1).
    field2: usize,
    /// Field separator character. `None` means whitespace runs.
    separator: Option<char>,
    /// Also print unpairable lines from file 1.
    print_unpaired1: bool,
    /// Also print unpairable lines from file 2.
    print_unpaired2: bool,
    /// Suppress normal paired output (used with -v).
    suppress_paired: bool,
    /// Replacement string for missing fields.
    empty_filler: Option<String>,
    /// Output format specs. `None` means default (join field, rest of file1,
    /// rest of file2).
    output_format: Option<Vec<OutputSpec>>,
    /// Case-insensitive comparison on join fields.
    ignore_case: bool,
    /// Check input sort order.
    check_order: bool,
    /// Treat first line of each file as header.
    header: bool,
    /// Output as JSON.
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

/// Consume the next argument from `args` at position `*i`, advancing `*i`.
/// Exits on missing argument.
fn next_arg<'a>(args: &'a [String], i: &mut usize, opt_name: &str) -> &'a str {
    *i += 1;
    if *i >= args.len() {
        eprintln!("join: option '{opt_name}' requires an argument");
        process::exit(1);
    }
    &args[*i]
}

/// Parse a 1-based field number. Exits on invalid input.
fn parse_field(s: &str, opt_name: &str) -> usize {
    match s.parse::<usize>() {
        Ok(n) if n >= 1 => n,
        _ => {
            eprintln!("join: invalid field number '{s}' for {opt_name}");
            process::exit(1);
        }
    }
}

/// Parse a file number (must be 1 or 2). Exits on invalid input.
fn parse_filenum(s: &str, opt_name: &str) -> u8 {
    match s {
        "1" => 1,
        "2" => 2,
        _ => {
            eprintln!(
                "join: invalid file number '{s}' for {opt_name} (must be 1 or 2)"
            );
            process::exit(1);
        }
    }
}

/// Parse the `-o` format specification.
fn parse_output_format(s: &str) -> Option<Vec<OutputSpec>> {
    if s == "auto" {
        return None;
    }
    let mut specs = Vec::new();
    for token in s.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        // Accept "0" as shorthand for the join field.
        if token == "0" {
            specs.push(OutputSpec { file: 0, field: 0 });
            continue;
        }
        let parts: Vec<&str> = token.splitn(2, '.').collect();
        if parts.len() != 2 {
            eprintln!("join: invalid field spec '{token}' in -o format");
            process::exit(1);
        }
        let file = match parts[0] {
            "1" => 1_u8,
            "2" => 2_u8,
            _ => {
                eprintln!(
                    "join: invalid file number '{}' in -o spec '{token}'",
                    parts[0]
                );
                process::exit(1);
            }
        };
        let field = match parts[1].parse::<usize>() {
            Ok(n) if n >= 1 => n,
            _ => {
                eprintln!(
                    "join: invalid field number '{}' in -o spec '{token}'",
                    parts[1]
                );
                process::exit(1);
            }
        };
        specs.push(OutputSpec { file, field });
    }
    if specs.is_empty() {
        eprintln!("join: empty -o format specification");
        process::exit(1);
    }
    Some(specs)
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut field1: usize = 1;
    let mut field2: usize = 1;
    let mut separator: Option<char> = None;
    let mut print_unpaired1 = false;
    let mut print_unpaired2 = false;
    let mut suppress_paired = false;
    let mut empty_filler: Option<String> = None;
    let mut output_format: Option<Vec<OutputSpec>> = None;
    let mut ignore_case = false;
    let mut check_order = true;
    let mut header = false;
    let mut json = false;
    let mut positionals: Vec<String> = Vec::new();
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = args[i].clone();

        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            positionals.push(arg);
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
            match arg.as_str() {
                "--help" => return ParseResult::Help,
                "--version" => return ParseResult::Version,
                "--ignore-case" => ignore_case = true,
                "--check-order" => check_order = true,
                "--nocheck-order" => check_order = false,
                "--header" => header = true,
                "--json" => json = true,
                _ => {
                    eprintln!("join: unrecognized option '{arg}'");
                    eprintln!("Try 'join --help' for more information.");
                    process::exit(1);
                }
            }
            i += 1;
            continue;
        }

        // Short options. These are not clusterable due to value-taking opts.
        match arg.as_str() {
            "-1" => {
                let val = next_arg(args, &mut i, "-1");
                field1 = parse_field(val, "-1");
            }
            "-2" => {
                let val = next_arg(args, &mut i, "-2");
                field2 = parse_field(val, "-2");
            }
            "-j" => {
                let val = next_arg(args, &mut i, "-j");
                let f = parse_field(val, "-j");
                field1 = f;
                field2 = f;
            }
            "-t" => {
                let val = next_arg(args, &mut i, "-t");
                let mut chars = val.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => separator = Some(c),
                    _ => {
                        eprintln!(
                            "join: -t separator must be a single character"
                        );
                        process::exit(1);
                    }
                }
            }
            "-a" => {
                let val = next_arg(args, &mut i, "-a");
                match parse_filenum(val, "-a") {
                    1 => print_unpaired1 = true,
                    2 => print_unpaired2 = true,
                    _ => unreachable!(),
                }
            }
            "-v" => {
                let val = next_arg(args, &mut i, "-v");
                suppress_paired = true;
                match parse_filenum(val, "-v") {
                    1 => print_unpaired1 = true,
                    2 => print_unpaired2 = true,
                    _ => unreachable!(),
                }
            }
            "-e" => {
                let val = next_arg(args, &mut i, "-e");
                empty_filler = Some(val.to_string());
            }
            "-o" => {
                let val = next_arg(args, &mut i, "-o");
                output_format = parse_output_format(val);
            }
            "-i" => ignore_case = true,
            _ => {
                // Handle -j1 and -j2 as combined flags.
                if arg.starts_with("-j1") {
                    let rest = &arg[3..];
                    if rest.is_empty() {
                        let val = next_arg(args, &mut i, "-j1");
                        field1 = parse_field(val, "-j1");
                    } else {
                        field1 = parse_field(rest, "-j1");
                    }
                } else if arg.starts_with("-j2") {
                    let rest = &arg[3..];
                    if rest.is_empty() {
                        let val = next_arg(args, &mut i, "-j2");
                        field2 = parse_field(val, "-j2");
                    } else {
                        field2 = parse_field(rest, "-j2");
                    }
                } else {
                    eprintln!("join: unrecognized option '{arg}'");
                    eprintln!("Try 'join --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    if positionals.len() != 2 {
        eprintln!(
            "join: expected 2 file operands, got {}",
            positionals.len()
        );
        eprintln!("Try 'join --help' for more information.");
        process::exit(1);
    }

    if positionals[0] == "-" && positionals[1] == "-" {
        eprintln!("join: only one file operand may be '-' (stdin)");
        process::exit(1);
    }

    ParseResult::Run(Config {
        file1: positionals[0].clone(),
        file2: positionals[1].clone(),
        field1,
        field2,
        separator,
        print_unpaired1,
        print_unpaired2,
        suppress_paired,
        empty_filler,
        output_format,
        ignore_case,
        check_order,
        header,
        json,
    })
}

// ============================================================================
// Field splitting
// ============================================================================

/// Split a line into fields according to the separator. With `None`
/// (whitespace mode), leading/trailing whitespace is ignored and runs of
/// whitespace delimit fields.
fn split_fields<'a>(line: &'a str, sep: Option<char>) -> Vec<&'a str> {
    match sep {
        Some(c) => line.split(c).collect(),
        None => line.split_whitespace().collect(),
    }
}

/// Extract the 1-based join field from a pre-split field list.
/// Returns `""` if the field index is out of range.
fn get_field<'a>(fields: &[&'a str], index: usize) -> &'a str {
    if index >= 1 && index <= fields.len() {
        fields[index - 1]
    } else {
        ""
    }
}

// ============================================================================
// Comparison helpers
// ============================================================================

fn compare_keys(a: &str, b: &str, ignore_case: bool) -> Ordering {
    if ignore_case {
        a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
    } else {
        a.cmp(b)
    }
}

// ============================================================================
// Output helpers
// ============================================================================

/// Write the field separator to the output.
fn write_sep(out: &mut dyn Write, config: &Config) -> io::Result<()> {
    match config.separator {
        None => out.write_all(b" "),
        Some(c) => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            out.write_all(s.as_bytes())
        }
    }
}

/// Get a field value, substituting the empty-filler if the field is missing.
fn field_or_filler<'a>(
    fields: &[&'a str],
    index: usize,
    filler: &'a str,
) -> &'a str {
    if index >= 1 && index <= fields.len() {
        let val = fields[index - 1];
        if val.is_empty() { filler } else { val }
    } else {
        filler
    }
}

/// Write a joined output line in the default format:
///   join_field, remaining fields from file1, remaining fields from file2
fn write_default_line(
    out: &mut dyn Write,
    config: &Config,
    join_key: &str,
    fields1: &[&str],
    fields2: &[&str],
) -> io::Result<()> {
    let filler = config.empty_filler.as_deref().unwrap_or("");
    out.write_all(join_key.as_bytes())?;

    // Remaining fields from file1 (skip the join field).
    for (idx, _) in fields1.iter().enumerate() {
        let field_num = idx + 1; // 1-based
        if field_num == config.field1 {
            continue;
        }
        write_sep(out, config)?;
        let val = field_or_filler(fields1, field_num, filler);
        out.write_all(val.as_bytes())?;
    }

    // Remaining fields from file2 (skip the join field).
    for (idx, _) in fields2.iter().enumerate() {
        let field_num = idx + 1;
        if field_num == config.field2 {
            continue;
        }
        write_sep(out, config)?;
        let val = field_or_filler(fields2, field_num, filler);
        out.write_all(val.as_bytes())?;
    }

    out.write_all(b"\n")
}

/// Write an output line using the explicit `-o` format.
fn write_formatted_line(
    out: &mut dyn Write,
    config: &Config,
    join_key: &str,
    fields1: &[&str],
    fields2: &[&str],
    specs: &[OutputSpec],
) -> io::Result<()> {
    let filler = config.empty_filler.as_deref().unwrap_or("");
    let mut first = true;

    for spec in specs {
        if !first {
            write_sep(out, config)?;
        }
        first = false;

        if spec.file == 0 {
            // The join field.
            out.write_all(join_key.as_bytes())?;
        } else {
            let fields = if spec.file == 1 { fields1 } else { fields2 };
            let val = field_or_filler(fields, spec.field, filler);
            out.write_all(val.as_bytes())?;
        }
    }

    out.write_all(b"\n")
}

/// Write an unpaired line from one of the files.
fn write_unpaired_line(
    out: &mut dyn Write,
    config: &Config,
    fields: &[&str],
    file_num: u8,
    join_field_idx: usize,
) -> io::Result<()> {
    let filler = config.empty_filler.as_deref().unwrap_or("");
    let join_key = field_or_filler(fields, join_field_idx, filler);

    if let Some(ref specs) = config.output_format {
        // With -o, output formatted. Fields from the other file are filler.
        let empty: Vec<&str> = Vec::new();
        let (f1, f2) = if file_num == 1 {
            (fields, empty.as_slice())
        } else {
            (empty.as_slice(), fields)
        };
        write_formatted_line(out, config, join_key, f1, f2, specs)
    } else {
        // Default format: join_key followed by remaining fields.
        out.write_all(join_key.as_bytes())?;
        for (idx, _) in fields.iter().enumerate() {
            let field_num = idx + 1;
            if field_num == join_field_idx {
                continue;
            }
            write_sep(out, config)?;
            let val = field_or_filler(fields, field_num, filler);
            out.write_all(val.as_bytes())?;
        }
        out.write_all(b"\n")
    }
}

/// Write a paired (joined) output line, dispatching to formatted or default.
fn write_paired_line(
    out: &mut dyn Write,
    config: &Config,
    join_key: &str,
    fields1: &[&str],
    fields2: &[&str],
) -> io::Result<()> {
    if let Some(ref specs) = config.output_format {
        write_formatted_line(out, config, join_key, fields1, fields2, specs)
    } else {
        write_default_line(out, config, join_key, fields1, fields2)
    }
}

// ============================================================================
// JSON output
// ============================================================================

/// Escape a string for JSON.
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

/// A collected JSON output entry.
enum JsonEntry {
    Paired { fields1: Vec<String>, fields2: Vec<String>, join_key: String },
    Unpaired { file_num: u8, fields: Vec<String>, join_key: String },
}

fn write_json_output(out: &mut dyn Write, entries: &[JsonEntry]) -> io::Result<()> {
    out.write_all(b"[\n")?;
    let total = entries.len();
    for (idx, entry) in entries.iter().enumerate() {
        match entry {
            JsonEntry::Paired { fields1, fields2, join_key } => {
                let key_e = json_escape(join_key);
                let f1: Vec<String> =
                    fields1.iter().map(|f| json_escape(f)).collect();
                let f2: Vec<String> =
                    fields2.iter().map(|f| json_escape(f)).collect();
                write!(out, "  {{\"type\": \"paired\", \"join_key\": \"{key_e}\"")?;
                write!(out, ", \"file1_fields\": [")?;
                for (fi, f) in f1.iter().enumerate() {
                    if fi > 0 {
                        write!(out, ", ")?;
                    }
                    write!(out, "\"{f}\"")?;
                }
                write!(out, "], \"file2_fields\": [")?;
                for (fi, f) in f2.iter().enumerate() {
                    if fi > 0 {
                        write!(out, ", ")?;
                    }
                    write!(out, "\"{f}\"")?;
                }
                write!(out, "]}}")?;
            }
            JsonEntry::Unpaired { file_num, fields, join_key } => {
                let key_e = json_escape(join_key);
                let fs: Vec<String> =
                    fields.iter().map(|f| json_escape(f)).collect();
                write!(
                    out,
                    "  {{\"type\": \"unpaired\", \"file\": {file_num}, \"join_key\": \"{key_e}\""
                )?;
                write!(out, ", \"fields\": [")?;
                for (fi, f) in fs.iter().enumerate() {
                    if fi > 0 {
                        write!(out, ", ")?;
                    }
                    write!(out, "\"{f}\"")?;
                }
                write!(out, "]}}")?;
            }
        }
        if idx + 1 < total {
            out.write_all(b",")?;
        }
        out.write_all(b"\n")?;
    }
    out.write_all(b"]\n")
}

// ============================================================================
// File reading
// ============================================================================

fn open_input(path: &str) -> io::Result<Box<dyn BufRead>> {
    if path == "-" {
        Ok(Box::new(BufReader::new(io::stdin())))
    } else {
        let file = File::open(path)?;
        Ok(Box::new(BufReader::new(file)))
    }
}

/// Read all lines from a reader into a Vec.
fn read_all_lines(reader: Box<dyn BufRead>) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    for line_result in reader.lines() {
        lines.push(line_result?);
    }
    Ok(lines)
}

// ============================================================================
// Sort order checking
// ============================================================================

/// Verify that adjacent lines are sorted by their join key. Returns an error
/// message if the order is violated.
fn check_sort_order(
    prev_key: &str,
    cur_key: &str,
    file_label: &str,
    ignore_case: bool,
) -> Option<String> {
    if compare_keys(prev_key, cur_key, ignore_case) == Ordering::Greater {
        Some(format!("join: {file_label} is not in sorted order"))
    } else {
        None
    }
}

// ============================================================================
// Core join logic
// ============================================================================

/// Perform the join. Returns an exit code (0 = success, 1 = error/unsorted).
fn run_join(config: &Config) -> i32 {
    // Open files and read all lines.
    let reader1 = match open_input(&config.file1) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("join: {}: {e}", config.file1);
            return 1;
        }
    };
    let reader2 = match open_input(&config.file2) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("join: {}: {e}", config.file2);
            return 1;
        }
    };

    let lines1 = match read_all_lines(reader1) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("join: {}: read error: {e}", config.file1);
            return 1;
        }
    };
    let lines2 = match read_all_lines(reader2) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("join: {}: read error: {e}", config.file2);
            return 1;
        }
    };

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    let exit_code = if config.json {
        run_join_json(config, &lines1, &lines2, &mut out)
    } else {
        run_join_text(config, &lines1, &lines2, &mut out)
    };

    if let Err(e) = out.flush() {
        if e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("join: write error: {e}");
            return 1;
        }
    }

    exit_code
}

/// Text-mode join.
fn run_join_text(
    config: &Config,
    lines1: &[String],
    lines2: &[String],
    out: &mut dyn Write,
) -> i32 {
    let mut exit_code = 0;
    let start1 = if config.header { 1 } else { 0 };
    let start2 = if config.header { 1 } else { 0 };

    // Handle header line: join the headers unconditionally.
    if config.header {
        if !lines1.is_empty() && !lines2.is_empty() {
            let f1 = split_fields(&lines1[0], config.separator);
            let f2 = split_fields(&lines2[0], config.separator);
            let key = get_field(&f1, config.field1);
            if let Err(e) = write_paired_line(out, config, key, &f1, &f2) {
                if e.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("join: write error: {e}");
                }
                return 1;
            }
        } else if !lines1.is_empty() && config.print_unpaired1 {
            let f1 = split_fields(&lines1[0], config.separator);
            if let Err(e) =
                write_unpaired_line(out, config, &f1, 1, config.field1)
            {
                if e.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("join: write error: {e}");
                }
                return 1;
            }
        } else if !lines2.is_empty() && config.print_unpaired2 {
            let f2 = split_fields(&lines2[0], config.separator);
            if let Err(e) =
                write_unpaired_line(out, config, &f2, 2, config.field2)
            {
                if e.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("join: write error: {e}");
                }
                return 1;
            }
        }
    }

    let mut i = start1;
    let mut j = start2;
    let mut prev_key1: Option<String> = None;
    let mut prev_key2: Option<String> = None;

    while i < lines1.len() && j < lines2.len() {
        let fields1 = split_fields(&lines1[i], config.separator);
        let fields2 = split_fields(&lines2[j], config.separator);
        let key1 = get_field(&fields1, config.field1);
        let key2 = get_field(&fields2, config.field2);

        // Check sort order.
        if config.check_order {
            if let Some(ref pk) = prev_key1 {
                if let Some(msg) =
                    check_sort_order(pk, key1, "file 1", config.ignore_case)
                {
                    eprintln!("{msg}");
                    exit_code = 1;
                }
            }
            if let Some(ref pk) = prev_key2 {
                if let Some(msg) =
                    check_sort_order(pk, key2, "file 2", config.ignore_case)
                {
                    eprintln!("{msg}");
                    exit_code = 1;
                }
            }
        }

        match compare_keys(key1, key2, config.ignore_case) {
            Ordering::Less => {
                if config.print_unpaired1 {
                    if let Err(e) = write_unpaired_line(
                        out,
                        config,
                        &fields1,
                        1,
                        config.field1,
                    ) {
                        if e.kind() != io::ErrorKind::BrokenPipe {
                            eprintln!("join: write error: {e}");
                        }
                        return 1;
                    }
                }
                prev_key1 = Some(key1.to_string());
                i += 1;
            }
            Ordering::Greater => {
                if config.print_unpaired2 {
                    if let Err(e) = write_unpaired_line(
                        out,
                        config,
                        &fields2,
                        2,
                        config.field2,
                    ) {
                        if e.kind() != io::ErrorKind::BrokenPipe {
                            eprintln!("join: write error: {e}");
                        }
                        return 1;
                    }
                }
                prev_key2 = Some(key2.to_string());
                j += 1;
            }
            Ordering::Equal => {
                // Find the range of matching lines in both files. Join keys
                // can repeat (many-to-many join).
                let match_key = key1.to_string();
                let i_start = i;
                let j_start = j;

                while i < lines1.len() {
                    let f = split_fields(&lines1[i], config.separator);
                    let k = get_field(&f, config.field1);
                    if compare_keys(k, &match_key, config.ignore_case)
                        != Ordering::Equal
                    {
                        break;
                    }
                    i += 1;
                }
                while j < lines2.len() {
                    let f = split_fields(&lines2[j], config.separator);
                    let k = get_field(&f, config.field2);
                    if compare_keys(k, &match_key, config.ignore_case)
                        != Ordering::Equal
                    {
                        break;
                    }
                    j += 1;
                }

                // Produce the cross product of matching ranges.
                if !config.suppress_paired {
                    for ii in i_start..i {
                        let f1 =
                            split_fields(&lines1[ii], config.separator);
                        for jj in j_start..j {
                            let f2 = split_fields(
                                &lines2[jj],
                                config.separator,
                            );
                            let jk = get_field(&f1, config.field1);
                            if let Err(e) = write_paired_line(
                                out, config, jk, &f1, &f2,
                            ) {
                                if e.kind() != io::ErrorKind::BrokenPipe {
                                    eprintln!("join: write error: {e}");
                                }
                                return 1;
                            }
                        }
                    }
                }

                prev_key1 = Some(match_key.clone());
                prev_key2 = Some(match_key);
            }
        }
    }

    // Drain remaining lines from file1.
    while i < lines1.len() {
        if config.print_unpaired1 {
            let fields1 = split_fields(&lines1[i], config.separator);
            let key1 = get_field(&fields1, config.field1);
            if config.check_order {
                if let Some(ref pk) = prev_key1 {
                    if let Some(msg) = check_sort_order(
                        pk,
                        key1,
                        "file 1",
                        config.ignore_case,
                    ) {
                        eprintln!("{msg}");
                        exit_code = 1;
                    }
                }
            }
            if let Err(e) =
                write_unpaired_line(out, config, &fields1, 1, config.field1)
            {
                if e.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("join: write error: {e}");
                }
                return 1;
            }
            prev_key1 = Some(key1.to_string());
        }
        i += 1;
    }

    // Drain remaining lines from file2.
    while j < lines2.len() {
        if config.print_unpaired2 {
            let fields2 = split_fields(&lines2[j], config.separator);
            let key2 = get_field(&fields2, config.field2);
            if config.check_order {
                if let Some(ref pk) = prev_key2 {
                    if let Some(msg) = check_sort_order(
                        pk,
                        key2,
                        "file 2",
                        config.ignore_case,
                    ) {
                        eprintln!("{msg}");
                        exit_code = 1;
                    }
                }
            }
            if let Err(e) =
                write_unpaired_line(out, config, &fields2, 2, config.field2)
            {
                if e.kind() != io::ErrorKind::BrokenPipe {
                    eprintln!("join: write error: {e}");
                }
                return 1;
            }
            prev_key2 = Some(key2.to_string());
        }
        j += 1;
    }

    exit_code
}

/// JSON-mode join: collect entries then write.
fn run_join_json(
    config: &Config,
    lines1: &[String],
    lines2: &[String],
    out: &mut dyn Write,
) -> i32 {
    let mut exit_code = 0;
    let mut entries: Vec<JsonEntry> = Vec::new();

    let start1 = if config.header { 1 } else { 0 };
    let start2 = if config.header { 1 } else { 0 };

    // Header line.
    if config.header && !lines1.is_empty() && !lines2.is_empty() {
        let f1 = split_fields(&lines1[0], config.separator);
        let f2 = split_fields(&lines2[0], config.separator);
        let key = get_field(&f1, config.field1).to_string();
        entries.push(JsonEntry::Paired {
            fields1: f1.iter().map(|s| s.to_string()).collect(),
            fields2: f2.iter().map(|s| s.to_string()).collect(),
            join_key: key,
        });
    }

    let mut i = start1;
    let mut j = start2;
    let mut prev_key1: Option<String> = None;
    let mut prev_key2: Option<String> = None;

    while i < lines1.len() && j < lines2.len() {
        let fields1 = split_fields(&lines1[i], config.separator);
        let fields2 = split_fields(&lines2[j], config.separator);
        let key1 = get_field(&fields1, config.field1);
        let key2 = get_field(&fields2, config.field2);

        if config.check_order {
            if let Some(ref pk) = prev_key1 {
                if let Some(msg) =
                    check_sort_order(pk, key1, "file 1", config.ignore_case)
                {
                    eprintln!("{msg}");
                    exit_code = 1;
                }
            }
            if let Some(ref pk) = prev_key2 {
                if let Some(msg) =
                    check_sort_order(pk, key2, "file 2", config.ignore_case)
                {
                    eprintln!("{msg}");
                    exit_code = 1;
                }
            }
        }

        match compare_keys(key1, key2, config.ignore_case) {
            Ordering::Less => {
                if config.print_unpaired1 {
                    entries.push(JsonEntry::Unpaired {
                        file_num: 1,
                        fields: fields1.iter().map(|s| s.to_string()).collect(),
                        join_key: key1.to_string(),
                    });
                }
                prev_key1 = Some(key1.to_string());
                i += 1;
            }
            Ordering::Greater => {
                if config.print_unpaired2 {
                    entries.push(JsonEntry::Unpaired {
                        file_num: 2,
                        fields: fields2.iter().map(|s| s.to_string()).collect(),
                        join_key: key2.to_string(),
                    });
                }
                prev_key2 = Some(key2.to_string());
                j += 1;
            }
            Ordering::Equal => {
                let match_key = key1.to_string();
                let i_start = i;
                let j_start = j;

                while i < lines1.len() {
                    let f = split_fields(&lines1[i], config.separator);
                    let k = get_field(&f, config.field1);
                    if compare_keys(k, &match_key, config.ignore_case)
                        != Ordering::Equal
                    {
                        break;
                    }
                    i += 1;
                }
                while j < lines2.len() {
                    let f = split_fields(&lines2[j], config.separator);
                    let k = get_field(&f, config.field2);
                    if compare_keys(k, &match_key, config.ignore_case)
                        != Ordering::Equal
                    {
                        break;
                    }
                    j += 1;
                }

                if !config.suppress_paired {
                    for ii in i_start..i {
                        let f1 =
                            split_fields(&lines1[ii], config.separator);
                        for jj in j_start..j {
                            let f2 = split_fields(
                                &lines2[jj],
                                config.separator,
                            );
                            let jk = get_field(&f1, config.field1).to_string();
                            entries.push(JsonEntry::Paired {
                                fields1: f1
                                    .iter()
                                    .map(|s| s.to_string())
                                    .collect(),
                                fields2: f2
                                    .iter()
                                    .map(|s| s.to_string())
                                    .collect(),
                                join_key: jk,
                            });
                        }
                    }
                }

                prev_key1 = Some(match_key.clone());
                prev_key2 = Some(match_key);
            }
        }
    }

    // Drain remaining.
    while i < lines1.len() {
        if config.print_unpaired1 {
            let fields1 = split_fields(&lines1[i], config.separator);
            let key1 = get_field(&fields1, config.field1);
            if config.check_order {
                if let Some(ref pk) = prev_key1 {
                    if let Some(msg) = check_sort_order(
                        pk,
                        key1,
                        "file 1",
                        config.ignore_case,
                    ) {
                        eprintln!("{msg}");
                        exit_code = 1;
                    }
                }
            }
            entries.push(JsonEntry::Unpaired {
                file_num: 1,
                fields: fields1.iter().map(|s| s.to_string()).collect(),
                join_key: key1.to_string(),
            });
            prev_key1 = Some(key1.to_string());
        }
        i += 1;
    }
    while j < lines2.len() {
        if config.print_unpaired2 {
            let fields2 = split_fields(&lines2[j], config.separator);
            let key2 = get_field(&fields2, config.field2);
            if config.check_order {
                if let Some(ref pk) = prev_key2 {
                    if let Some(msg) = check_sort_order(
                        pk,
                        key2,
                        "file 2",
                        config.ignore_case,
                    ) {
                        eprintln!("{msg}");
                        exit_code = 1;
                    }
                }
            }
            entries.push(JsonEntry::Unpaired {
                file_num: 2,
                fields: fields2.iter().map(|s| s.to_string()).collect(),
                join_key: key2.to_string(),
            });
            prev_key2 = Some(key2.to_string());
        }
        j += 1;
    }

    if let Err(e) = write_json_output(out, &entries) {
        if e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("join: write error: {e}");
        }
        return 1;
    }

    exit_code
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS join v{VERSION}");
    println!();
    println!("Join lines of two sorted files on a common field.");
    println!();
    println!("USAGE:");
    println!("  join [OPTION]... FILE1 FILE2");
    println!();
    println!("For each pair of input lines with identical join fields, write a");
    println!("line to standard output. The default join field is the first,");
    println!("delimited by blanks.");
    println!();
    println!("FIELD SELECTION:");
    println!("  -1 FIELD             Join on this field of file 1 (1-based)");
    println!("  -j1 FIELD            Same as -1 FIELD");
    println!("  -2 FIELD             Join on this field of file 2 (1-based)");
    println!("  -j2 FIELD            Same as -2 FIELD");
    println!("  -j FIELD             Equivalent to -1 FIELD -2 FIELD");
    println!();
    println!("FORMATTING:");
    println!("  -t CHAR              Use CHAR as input and output field separator");
    println!("  -o FORMAT            Obey FORMAT while constructing output line");
    println!("                       FORMAT is comma-separated FILENUM.FIELD specs");
    println!("                       or 'auto'. Use 0 for the join field.");
    println!("  -e STRING            Replace missing (empty) output fields with STRING");
    println!();
    println!("PAIRING CONTROL:");
    println!("  -a FILENUM           Also print unpairable lines from FILENUM (1 or 2)");
    println!("  -v FILENUM           Like -a but suppress joined output lines");
    println!();
    println!("COMPARISON:");
    println!("  -i, --ignore-case    Case-insensitive comparison on join fields");
    println!("      --check-order    Check that input is correctly sorted (default)");
    println!("      --nocheck-order  Do not check input sort order");
    println!();
    println!("OUTPUT:");
    println!("      --header         Treat the first line of each file as a header");
    println!("      --json           Output as JSON array");
    println!();
    println!("OTHER:");
    println!("      --help           Display this help and exit");
    println!("      --version        Output version information and exit");
    println!();
    println!("Use '-' as FILE1 or FILE2 to read from standard input.");
    println!();
    println!("EXAMPLES:");
    println!("  join file1 file2              Inner join on first field");
    println!("  join -t, file1.csv file2.csv  Join CSV files on first field");
    println!("  join -1 2 -2 1 a.txt b.txt   Join field 2 of a.txt with field 1 of b.txt");
    println!("  join -a 1 file1 file2         Left outer join");
    println!("  join -v 2 file1 file2         Lines in file2 with no match in file1");
    println!("  join -o 1.1,2.2,1.3 f1 f2    Custom output format");
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
            println!("join (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run_join(&config);
            process::exit(code);
        }
    }
}
