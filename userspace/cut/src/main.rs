//! Slate OS `cut` Utility -- Remove Sections From Lines of Files
//!
//! Removes sections from each line of input, keeping only selected fields,
//! characters, or bytes. Modeled after POSIX/GNU coreutils `cut`.
//!
//! # Usage
//!
//! ```text
//! cut OPTION... [FILE]...
//!
//! Print selected parts of lines from each FILE to standard output.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -b, --bytes=LIST        Select only these bytes
//!   -c, --characters=LIST   Select only these characters
//!   -f, --fields=LIST       Select only these fields
//!   -d, --delimiter=CHAR    Use CHAR instead of TAB for field delimiter
//!   -s, --only-delimited    Do not print lines not containing delimiters (with -f)
//!       --complement        Complement the set of selected bytes, characters, or fields
//!       --output-delimiter=STRING  Use STRING as the output delimiter
//!       --json              Output results as JSON
//!       --help              Display this help and exit
//!       --version           Output version information and exit
//!
//! LIST is a comma- or space-separated list of ranges. Each range is one of:
//!   N      N'th byte, character, or field, counted from 1
//!   N-     From N'th byte, character, or field, to end of line
//!   N-M    From N'th to M'th (inclusive) byte, character, or field
//!   -M     From first to M'th (inclusive) byte, character, or field
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
// Selection mode
// ============================================================================

/// What kind of selection the user requested.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bytes,
    Characters,
    Fields,
}

// ============================================================================
// Range representation
// ============================================================================

/// An inclusive 1-based range. `end == usize::MAX` means "to end of line."
#[derive(Clone, Copy, PartialEq, Eq)]
struct Range {
    start: usize,
    end: usize,
}

/// Parse a LIST string (e.g. "1-3,7,10-") into a sorted, merged vector of
/// `Range` values. Returns an error message on invalid input.
fn parse_ranges(list: &str) -> Result<Vec<Range>, String> {
    let mut ranges = Vec::new();

    // LIST may be comma- or space-separated (GNU cut also allows spaces).
    for token in list.split(|c: char| c == ',' || c.is_ascii_whitespace()) {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }

        let range = if let Some(rest) = token.strip_prefix('-') {
            // "-M" form: from 1 to M.
            let m: usize = rest
                .parse()
                .map_err(|_| format!("invalid range: '{token}'"))?;
            if m == 0 {
                return Err(format!("fields and positions are numbered from 1: '{token}'"));
            }
            Range { start: 1, end: m }
        } else if let Some(rest) = token.strip_suffix('-') {
            // "N-" form: from N to end.
            let n: usize = rest
                .parse()
                .map_err(|_| format!("invalid range: '{token}'"))?;
            if n == 0 {
                return Err(format!("fields and positions are numbered from 1: '{token}'"));
            }
            Range {
                start: n,
                end: usize::MAX,
            }
        } else if let Some(dash_pos) = token.find('-') {
            // "N-M" form.
            let n: usize = token[..dash_pos]
                .parse()
                .map_err(|_| format!("invalid range: '{token}'"))?;
            let m: usize = token[dash_pos + 1..]
                .parse()
                .map_err(|_| format!("invalid range: '{token}'"))?;
            if n == 0 || m == 0 {
                return Err(format!("fields and positions are numbered from 1: '{token}'"));
            }
            if n > m {
                return Err(format!(
                    "invalid decreasing range: '{token}'"
                ));
            }
            Range { start: n, end: m }
        } else {
            // Single number "N".
            let n: usize = token
                .parse()
                .map_err(|_| format!("invalid range: '{token}'"))?;
            if n == 0 {
                return Err(format!("fields and positions are numbered from 1: '{token}'"));
            }
            Range { start: n, end: n }
        };

        ranges.push(range);
    }

    if ranges.is_empty() {
        return Err("you must specify a list of bytes, characters, or fields".to_string());
    }

    // Sort by start, then by end descending (so larger ranges come first when
    // starts are equal, simplifying the merge).
    ranges.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));

    // Merge overlapping/adjacent ranges.
    let mut merged: Vec<Range> = Vec::with_capacity(ranges.len());
    for r in ranges {
        if let Some(last) = merged.last_mut() {
            // Ranges are 1-based integers; "adjacent" means last.end + 1 >= r.start.
            if last.end == usize::MAX || last.end.saturating_add(1) >= r.start {
                // Extend the current range.
                if r.end > last.end {
                    last.end = r.end;
                }
                continue;
            }
        }
        merged.push(r);
    }

    Ok(merged)
}


/// Build the complement of a set of ranges, for positions 1..=count.
/// If `count` is `None`, the complement extends to `usize::MAX`.
fn complement_ranges(ranges: &[Range], count: Option<usize>) -> Vec<Range> {
    let mut result = Vec::new();
    let mut next_start: usize = 1;

    for r in ranges {
        if r.start > next_start {
            result.push(Range {
                start: next_start,
                end: r.start - 1,
            });
        }
        next_start = if r.end == usize::MAX {
            // The original range covers everything to the end; complement has
            // nothing past here.
            return result;
        } else {
            r.end.saturating_add(1)
        };
    }

    // Remaining positions after the last range.
    let upper = count.unwrap_or(usize::MAX);
    if next_start <= upper {
        result.push(Range {
            start: next_start,
            end: upper,
        });
    }

    result
}

// ============================================================================
// Configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    mode: Mode,
    ranges: Vec<Range>,
    delimiter: char,
    only_delimited: bool,
    complement: bool,
    output_delimiter: Option<String>,
    json_output: bool,
    file_paths: Vec<String>,
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
    let mut mode: Option<Mode> = None;
    let mut range_list: Option<String> = None;
    let mut delimiter: Option<char> = None;
    let mut only_delimited = false;
    let mut complement = false;
    let mut output_delimiter: Option<String> = None;
    let mut json_output = false;
    let mut file_paths: Vec<String> = Vec::new();
    let mut end_of_opts = false;

    /// Helper: set the mode, erroring if a different mode was already set.
    fn set_mode(
        current: &mut Option<Mode>,
        new: Mode,
        range_list: &mut Option<String>,
        list_val: &str,
    ) {
        if let Some(prev) = *current
            && prev != new {
                eprintln!("cut: only one type of list may be specified");
                process::exit(1);
            }
        *current = Some(new);
        *range_list = Some(list_val.to_string());
    }

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
            if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--complement" {
                complement = true;
            } else if arg == "--only-delimited" {
                only_delimited = true;
            } else if arg == "--json" {
                json_output = true;
            } else if arg == "--bytes" || arg.starts_with("--bytes=") {
                let val = long_opt_value(arg, "--bytes", args, &mut i);
                set_mode(&mut mode, Mode::Bytes, &mut range_list, &val);
            } else if arg == "--characters" || arg.starts_with("--characters=") {
                let val = long_opt_value(arg, "--characters", args, &mut i);
                set_mode(&mut mode, Mode::Characters, &mut range_list, &val);
            } else if arg == "--fields" || arg.starts_with("--fields=") {
                let val = long_opt_value(arg, "--fields", args, &mut i);
                set_mode(&mut mode, Mode::Fields, &mut range_list, &val);
            } else if arg == "--delimiter" || arg.starts_with("--delimiter=") {
                let val = long_opt_value(arg, "--delimiter", args, &mut i);
                delimiter = Some(parse_delimiter(&val));
            } else if arg == "--output-delimiter"
                || arg.starts_with("--output-delimiter=")
            {
                let val = long_opt_value(arg, "--output-delimiter", args, &mut i);
                output_delimiter = Some(val);
            } else {
                eprintln!("cut: unrecognized option '{arg}'");
                eprintln!("Try 'cut --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options: may be combined but options taking values consume the
        // rest of the cluster or the next argument.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut j = 0;
        while j < chars.len() {
            match chars[j] {
                'b' => {
                    let val = short_opt_value(&chars, j, args, &mut i);
                    set_mode(&mut mode, Mode::Bytes, &mut range_list, &val);
                    // short_opt_value consumed the rest; break inner loop.
                    j = chars.len();
                    continue;
                }
                'c' => {
                    let val = short_opt_value(&chars, j, args, &mut i);
                    set_mode(&mut mode, Mode::Characters, &mut range_list, &val);
                    j = chars.len();
                    continue;
                }
                'f' => {
                    let val = short_opt_value(&chars, j, args, &mut i);
                    set_mode(&mut mode, Mode::Fields, &mut range_list, &val);
                    j = chars.len();
                    continue;
                }
                'd' => {
                    let val = short_opt_value(&chars, j, args, &mut i);
                    delimiter = Some(parse_delimiter(&val));
                    j = chars.len();
                    continue;
                }
                's' => {
                    only_delimited = true;
                }
                _ => {
                    eprintln!("cut: invalid option -- '{}'", chars[j]);
                    eprintln!("Try 'cut --help' for more information.");
                    process::exit(1);
                }
            }
            j += 1;
        }

        i += 1;
    }

    // Validate required arguments.
    let mode = match mode {
        Some(m) => m,
        None => {
            eprintln!("cut: you must specify a list of bytes, characters, or fields");
            eprintln!("Try 'cut --help' for more information.");
            process::exit(1);
        }
    };

    let range_str = match range_list {
        Some(r) => r,
        None => {
            eprintln!("cut: you must specify a list of bytes, characters, or fields");
            eprintln!("Try 'cut --help' for more information.");
            process::exit(1);
        }
    };

    let ranges = match parse_ranges(&range_str) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cut: {e}");
            process::exit(1);
        }
    };

    // -s only makes sense with -f.
    if only_delimited && mode != Mode::Fields {
        eprintln!("cut: suppressing non-delimited lines makes sense only when operating on fields");
        process::exit(1);
    }

    // -d only makes sense with -f.
    if delimiter.is_some() && mode != Mode::Fields {
        eprintln!("cut: an input delimiter may be specified only when operating on fields");
        process::exit(1);
    }

    ParseResult::Run(Config {
        mode,
        ranges,
        delimiter: delimiter.unwrap_or('\t'),
        only_delimited,
        complement,
        output_delimiter,
        json_output,
        file_paths,
    })
}

/// Extract the value for a long option that may use `--opt=VAL` or `--opt VAL`
/// syntax.
fn long_opt_value(arg: &str, name: &str, args: &[String], i: &mut usize) -> String {
    let prefix = format!("{name}=");
    if let Some(val) = arg.strip_prefix(&prefix) {
        return val.to_string();
    }
    // Value is the next argument.
    *i += 1;
    if *i >= args.len() {
        eprintln!("cut: option '{name}' requires an argument");
        process::exit(1);
    }
    args[*i].clone()
}

/// Extract the value for a short option. If there are remaining characters in
/// the cluster after position `j`, they form the value. Otherwise the next
/// argument is consumed.
fn short_opt_value(chars: &[char], j: usize, args: &[String], i: &mut usize) -> String {
    if j + 1 < chars.len() {
        // Remaining chars in cluster are the value.
        return chars[j + 1..].iter().collect();
    }
    // Value is the next argument.
    *i += 1;
    if *i >= args.len() {
        eprintln!("cut: option '-{}' requires an argument", chars[j]);
        process::exit(1);
    }
    args[*i].clone()
}

/// Parse a delimiter string. Must be exactly one character.
fn parse_delimiter(s: &str) -> char {
    let mut chars = s.chars();
    let ch = match chars.next() {
        Some(c) => c,
        None => {
            eprintln!("cut: delimiter must be a single character");
            process::exit(1);
        }
    };
    if chars.next().is_some() {
        eprintln!("cut: the delimiter must be a single character");
        process::exit(1);
    }
    ch
}

// ============================================================================
// Line processing
// ============================================================================

/// Process a single line in byte mode. Writes the selected bytes to `out`.
fn cut_bytes(line: &str, ranges: &[Range], complement: bool, out: &mut Vec<u8>) {
    let bytes = line.as_bytes();
    let len = bytes.len();

    if complement {
        let comp = complement_ranges(ranges, Some(len));
        emit_byte_ranges(bytes, &comp, out);
    } else {
        emit_byte_ranges(bytes, ranges, out);
    }
}

/// Emit selected byte ranges from a byte slice.
fn emit_byte_ranges(bytes: &[u8], ranges: &[Range], out: &mut Vec<u8>) {
    let len = bytes.len();
    for r in ranges {
        if r.start > len {
            break;
        }
        let start = r.start - 1; // Convert from 1-based to 0-based.
        let end = r.end.min(len); // Clamp to line length.
        if let Some(slice) = bytes.get(start..end) {
            out.extend_from_slice(slice);
        }
    }
}

/// Process a single line in character mode. Writes the selected characters to
/// `out`.
fn cut_characters(line: &str, ranges: &[Range], complement: bool, out: &mut Vec<u8>) {
    let chars: Vec<char> = line.chars().collect();
    let len = chars.len();

    if complement {
        let comp = complement_ranges(ranges, Some(len));
        emit_char_ranges(&chars, &comp, out);
    } else {
        emit_char_ranges(&chars, ranges, out);
    }
}

/// Emit selected character ranges from a char slice.
fn emit_char_ranges(chars: &[char], ranges: &[Range], out: &mut Vec<u8>) {
    let len = chars.len();
    let mut buf = [0u8; 4];
    for r in ranges {
        if r.start > len {
            break;
        }
        let start = r.start - 1;
        let end = r.end.min(len);
        for idx in start..end {
            if let Some(&ch) = chars.get(idx) {
                let encoded = ch.encode_utf8(&mut buf);
                out.extend_from_slice(encoded.as_bytes());
            }
        }
    }
}

/// Process a single line in field mode. Writes the selected fields to `out`.
///
/// Returns `false` if the line has no delimiter and `only_delimited` is set
/// (meaning the line should be suppressed).
fn cut_fields(
    line: &str,
    ranges: &[Range],
    delimiter: char,
    output_delim: &str,
    complement: bool,
    only_delimited: bool,
    out: &mut Vec<u8>,
) -> bool {
    // If the line contains no delimiter, output the entire line unless
    // --only-delimited is set.
    if !line.contains(delimiter) {
        if only_delimited {
            return false;
        }
        out.extend_from_slice(line.as_bytes());
        return true;
    }

    let fields: Vec<&str> = line.split(delimiter).collect();
    let count = fields.len();

    if complement {
        let comp = complement_ranges(ranges, Some(count));
        emit_field_ranges(&fields, &comp, output_delim, out);
    } else {
        emit_field_ranges(&fields, ranges, output_delim, out);
    }

    true
}

/// Emit selected field ranges, joined by the output delimiter.
fn emit_field_ranges(fields: &[&str], ranges: &[Range], output_delim: &str, out: &mut Vec<u8>) {
    let count = fields.len();
    let mut first = true;

    for r in ranges {
        if r.start > count {
            break;
        }
        let start = r.start - 1;
        let end = r.end.min(count);
        for idx in start..end {
            if let Some(field) = fields.get(idx) {
                if !first {
                    out.extend_from_slice(output_delim.as_bytes());
                }
                out.extend_from_slice(field.as_bytes());
                first = false;
            }
        }
    }
}

// ============================================================================
// JSON output
// ============================================================================

/// Escape a string for JSON output. Handles the required JSON escape sequences.
fn json_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if c.is_control() => {
                // Use \uXXXX for other control characters.
                for unit in c.encode_utf16(&mut [0u16; 2]) {
                    escaped.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => escaped.push(c),
        }
    }
    escaped
}

// ============================================================================
// Input sources
// ============================================================================

/// Represents a source of lines -- either a file or stdin.
enum Input {
    Stdin(io::StdinLock<'static>),
    File(BufReader<File>),
}

impl Input {
    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        match self {
            Input::Stdin(lock) => lock.read_line(buf),
            Input::File(reader) => reader.read_line(buf),
        }
    }
}

/// Open an input source. "-" or no files means stdin.
fn open_input(path: &str) -> Result<Input, io::Error> {
    if path == "-" {
        // Leak the stdin handle so it has 'static lifetime. This is
        // intentional: we only ever create one stdin input per process
        // invocation.
        let stdin = Box::leak(Box::new(io::stdin()));
        Ok(Input::Stdin(stdin.lock()))
    } else {
        let file = File::open(path)?;
        Ok(Input::File(BufReader::new(file)))
    }
}

// ============================================================================
// Core processing
// ============================================================================

/// Process all lines from a single input source.
fn process_input(
    input: &mut Input,
    config: &Config,
    stdout: &mut io::StdoutLock<'_>,
    json_rows: &mut Option<&mut Vec<String>>,
) -> io::Result<()> {
    let default_delim = config.delimiter.to_string();
    let output_delim = config
        .output_delimiter
        .as_deref()
        .unwrap_or(&default_delim);

    let mut line_buf = String::new();
    let mut out_buf: Vec<u8> = Vec::with_capacity(256);

    loop {
        line_buf.clear();
        let bytes_read = input.read_line(&mut line_buf)?;
        if bytes_read == 0 {
            break; // EOF
        }

        // Strip trailing newline(s) for processing. We re-add a newline on
        // output.
        let line = line_buf.trim_end_matches(['\n', '\r']);

        out_buf.clear();

        let should_output = match config.mode {
            Mode::Bytes => {
                cut_bytes(line, &config.ranges, config.complement, &mut out_buf);
                true
            }
            Mode::Characters => {
                cut_characters(line, &config.ranges, config.complement, &mut out_buf);
                true
            }
            Mode::Fields => cut_fields(
                line,
                &config.ranges,
                config.delimiter,
                output_delim,
                config.complement,
                config.only_delimited,
                &mut out_buf,
            ),
        };

        if !should_output {
            continue;
        }

        if let Some(rows) = json_rows.as_mut() {
            // For JSON mode, convert the output buffer to a string.
            let s = String::from_utf8_lossy(&out_buf);
            rows.push(json_escape(&s));
        } else {
            stdout.write_all(&out_buf)?;
            stdout.write_all(b"\n")?;
        }
    }

    Ok(())
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS cut v{VERSION}");
    println!();
    println!("Remove sections from each line of files.");
    println!();
    println!("USAGE:");
    println!("  cut OPTION... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -b, --bytes=LIST          Select only these bytes");
    println!("  -c, --characters=LIST     Select only these characters");
    println!("  -f, --fields=LIST         Select only these fields");
    println!("  -d, --delimiter=CHAR      Use CHAR instead of TAB for field delimiter");
    println!("  -s, --only-delimited      Do not print lines without delimiters (with -f)");
    println!("      --complement          Complement the set of selected items");
    println!("      --output-delimiter=STRING");
    println!("                            Use STRING as the output delimiter");
    println!("      --json                Output results as JSON array");
    println!("      --help                Display this help and exit");
    println!("      --version             Output version information and exit");
    println!();
    println!("LIST is a comma-separated list of ranges:");
    println!("  N      N'th byte, character, or field (counted from 1)");
    println!("  N-     From N'th to end of line");
    println!("  N-M    From N'th to M'th (inclusive)");
    println!("  -M     From first to M'th (inclusive)");
    println!();
    println!("With no FILE, or when FILE is -, read standard input.");
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
            println!("cut (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}

/// Run the cut utility with the parsed configuration. Returns the exit code.
fn run(config: &Config) -> i32 {
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();
    let mut exit_code = 0;

    // Collect JSON rows if in JSON mode.
    let mut json_rows: Vec<String> = Vec::new();

    // Determine input sources: if no files specified, read from stdin.
    let paths: Vec<String> = if config.file_paths.is_empty() {
        vec!["-".to_string()]
    } else {
        config.file_paths.clone()
    };

    for path in &paths {
        let mut input = match open_input(path) {
            Ok(inp) => inp,
            Err(e) => {
                eprintln!("cut: {path}: {e}");
                exit_code = 1;
                continue;
            }
        };

        let mut json_ref: Option<&mut Vec<String>> = if config.json_output {
            Some(&mut json_rows)
        } else {
            None
        };

        if let Err(e) = process_input(&mut input, config, &mut stdout_lock, &mut json_ref) {
            // Broken pipe is a normal exit condition when piped to head, etc.
            if e.kind() == io::ErrorKind::BrokenPipe {
                process::exit(0);
            }
            eprintln!("cut: {path}: {e}");
            exit_code = 1;
        }
    }

    // Output JSON if requested.
    if config.json_output {
        let mut json = String::from("[\n");
        for (idx, row) in json_rows.iter().enumerate() {
            json.push_str("  \"");
            json.push_str(row);
            json.push('"');
            if idx + 1 < json_rows.len() {
                json.push(',');
            }
            json.push('\n');
        }
        json.push(']');

        if let Err(e) = stdout_lock.write_all(json.as_bytes()) {
            if e.kind() == io::ErrorKind::BrokenPipe {
                process::exit(0);
            }
            eprintln!("cut: write error: {e}");
            exit_code = 1;
        }
        if let Err(e) = stdout_lock.write_all(b"\n")
            && e.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("cut: write error: {e}");
                exit_code = 1;
            }
    }

    // Flush stdout.
    if let Err(e) = stdout_lock.flush()
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("cut: write error: {e}");
            exit_code = 1;
        }

    exit_code
}
