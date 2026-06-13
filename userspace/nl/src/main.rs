//! SlateOS `nl` Utility -- Number Lines of Files
//!
//! Numbers lines of files according to configurable numbering styles, section
//! delimiters, and formatting options. Supports logical page sections (header,
//! body, footer) delimited by special markers in the input.
//!
//! # Usage
//!
//! ```text
//! nl [OPTION]... [FILE]...
//!
//! Write each FILE to standard output, with line numbers added.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -b, --body-numbering=STYLE      Use STYLE for numbering body lines (default: t)
//!   -h, --header-numbering=STYLE    Use STYLE for numbering header lines (default: n)
//!   -f, --footer-numbering=STYLE    Use STYLE for numbering footer lines (default: n)
//!   -d, --section-delimiter=CC      Use CC for section delimiters (default: \:)
//!   -i, --line-increment=N          Line number increment (default: 1)
//!   -n, --number-format=FORMAT      Insert line numbers according to FORMAT:
//!                                     ln  left justified, no leading zeros
//!                                     rn  right justified, no leading zeros (default)
//!                                     rz  right justified, leading zeros
//!   -p, --no-renumber               Do not reset line numbers at section boundaries
//!   -s, --number-separator=STRING   Add STRING after line number (default: TAB)
//!   -v, --starting-line-number=N    Start numbering at N (default: 1)
//!   -w, --number-width=N            Use N columns for line numbers (default: 6)
//!       --json                      Output as JSON array
//!       --help                      Display this help and exit
//!       --version                   Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

/// Default width of the line number field.
const DEFAULT_WIDTH: usize = 6;

/// Default line number increment.
const DEFAULT_INCREMENT: i64 = 1;

/// Default starting line number.
const DEFAULT_START: i64 = 1;

// ============================================================================
// Numbering style
// ============================================================================

/// How lines in a section are numbered.
#[derive(Clone)]
enum NumberingStyle {
    /// Number all lines.
    All,
    /// Number only non-empty lines (default for body).
    NonEmpty,
    /// Number no lines.
    None,
    /// Number lines matching a basic regex pattern.
    Regex(String),
}

/// Parse a numbering style string. `p<regex>` means regex; otherwise `a`, `t`,
/// or `n`.
fn parse_style(s: &str, flag_name: &str) -> NumberingStyle {
    match s {
        "a" => NumberingStyle::All,
        "t" => NumberingStyle::NonEmpty,
        "n" => NumberingStyle::None,
        _ if s.starts_with('p') => NumberingStyle::Regex(s[1..].to_string()),
        _ => {
            eprintln!("nl: invalid numbering style '{s}' for {flag_name}");
            process::exit(1);
        }
    }
}

// ============================================================================
// Number format
// ============================================================================

/// How the line number is formatted within its field.
///
/// The shared `Zero` suffix mirrors the BSD/GNU `nl` short-codes (`ln`/`rn`/`rz`),
/// so we keep the names rather than renaming for clippy's `enum_variant_names`.
#[allow(clippy::enum_variant_names)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum NumberFormat {
    /// Left-justified, no leading zeros.
    LeftNoZero,
    /// Right-justified, no leading zeros (default).
    RightNoZero,
    /// Right-justified, leading zeros.
    RightZero,
}

fn parse_number_format(s: &str) -> NumberFormat {
    match s {
        "ln" => NumberFormat::LeftNoZero,
        "rn" => NumberFormat::RightNoZero,
        "rz" => NumberFormat::RightZero,
        _ => {
            eprintln!("nl: invalid line numbering format '{s}'");
            eprintln!("Valid formats are: ln, rn, rz");
            process::exit(1);
        }
    }
}

// ============================================================================
// Section type
// ============================================================================

/// Logical page section, determined by delimiter lines in the input.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Header,
    Body,
    Footer,
}

// ============================================================================
// Configuration
// ============================================================================

struct Config {
    body_style: NumberingStyle,
    header_style: NumberingStyle,
    footer_style: NumberingStyle,
    /// The two-character section delimiter (default `\:`).
    section_delimiter: String,
    increment: i64,
    number_format: NumberFormat,
    no_renumber: bool,
    separator: String,
    start: i64,
    width: usize,
    json: bool,
    file_paths: Vec<String>,
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
    let mut body_style = NumberingStyle::NonEmpty;
    let mut header_style = NumberingStyle::None;
    let mut footer_style = NumberingStyle::None;
    let mut section_delimiter = String::from("\\:");
    let mut increment: i64 = DEFAULT_INCREMENT;
    let mut number_format = NumberFormat::RightNoZero;
    let mut no_renumber = false;
    let mut separator = String::from("\t");
    let mut start: i64 = DEFAULT_START;
    let mut width: usize = DEFAULT_WIDTH;
    let mut json = false;
    let mut file_paths: Vec<String> = Vec::new();
    let mut end_of_opts = false;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];

        if end_of_opts || !arg.starts_with('-') {
            file_paths.push(arg.clone());
            i += 1;
            continue;
        }

        if arg == "-" {
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
            } else if arg == "--json" {
                json = true;
            } else if arg == "--no-renumber" {
                no_renumber = true;
            } else if arg == "--body-numbering" || arg.starts_with("--body-numbering=") {
                let val = long_opt_value(arg, "--body-numbering", args, &mut i);
                body_style = parse_style(&val, "--body-numbering");
            } else if arg == "--header-numbering" || arg.starts_with("--header-numbering=") {
                let val = long_opt_value(arg, "--header-numbering", args, &mut i);
                header_style = parse_style(&val, "--header-numbering");
            } else if arg == "--footer-numbering" || arg.starts_with("--footer-numbering=") {
                let val = long_opt_value(arg, "--footer-numbering", args, &mut i);
                footer_style = parse_style(&val, "--footer-numbering");
            } else if arg == "--section-delimiter" || arg.starts_with("--section-delimiter=") {
                let val = long_opt_value(arg, "--section-delimiter", args, &mut i);
                section_delimiter = val;
            } else if arg == "--line-increment" || arg.starts_with("--line-increment=") {
                let val = long_opt_value(arg, "--line-increment", args, &mut i);
                increment = parse_i64_arg("--line-increment", &val);
            } else if arg == "--number-format" || arg.starts_with("--number-format=") {
                let val = long_opt_value(arg, "--number-format", args, &mut i);
                number_format = parse_number_format(&val);
            } else if arg == "--number-separator" || arg.starts_with("--number-separator=") {
                let val = long_opt_value(arg, "--number-separator", args, &mut i);
                separator = val;
            } else if arg == "--starting-line-number"
                || arg.starts_with("--starting-line-number=")
            {
                let val = long_opt_value(arg, "--starting-line-number", args, &mut i);
                start = parse_i64_arg("--starting-line-number", &val);
            } else if arg == "--number-width" || arg.starts_with("--number-width=") {
                let val = long_opt_value(arg, "--number-width", args, &mut i);
                width = parse_usize_arg("--number-width", &val);
                if width == 0 {
                    eprintln!("nl: invalid line number field width: '0'");
                    process::exit(1);
                }
            } else {
                eprintln!("nl: unrecognized option '{arg}'");
                eprintln!("Try 'nl --help' for more information.");
                process::exit(1);
            }

            i += 1;
            continue;
        }

        // Short options. Some take a value, so we handle them carefully.
        let chars: Vec<char> = arg[1..].chars().collect();
        let mut ci = 0;
        while ci < chars.len() {
            let ch = chars[ci];
            match ch {
                'b' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'b');
                    body_style = parse_style(&val, "-b");
                    ci = chars.len();
                    continue;
                }
                'h' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'h');
                    header_style = parse_style(&val, "-h");
                    ci = chars.len();
                    continue;
                }
                'f' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'f');
                    footer_style = parse_style(&val, "-f");
                    ci = chars.len();
                    continue;
                }
                'd' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'd');
                    section_delimiter = val;
                    ci = chars.len();
                    continue;
                }
                'i' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'i');
                    increment = parse_i64_arg("-i", &val);
                    ci = chars.len();
                    continue;
                }
                'n' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'n');
                    number_format = parse_number_format(&val);
                    ci = chars.len();
                    continue;
                }
                'p' => {
                    no_renumber = true;
                }
                's' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 's');
                    separator = val;
                    ci = chars.len();
                    continue;
                }
                'v' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'v');
                    start = parse_i64_arg("-v", &val);
                    ci = chars.len();
                    continue;
                }
                'w' => {
                    let val = short_opt_value(&chars, ci, args, &mut i, 'w');
                    width = parse_usize_arg("-w", &val);
                    if width == 0 {
                        eprintln!("nl: invalid line number field width: '0'");
                        process::exit(1);
                    }
                    ci = chars.len();
                    continue;
                }
                _ => {
                    eprintln!("nl: invalid option -- '{ch}'");
                    eprintln!("Try 'nl --help' for more information.");
                    process::exit(1);
                }
            }
            ci += 1;
        }

        i += 1;
    }

    ParseResult::Run(Config {
        body_style,
        header_style,
        footer_style,
        section_delimiter,
        increment,
        number_format,
        no_renumber,
        separator,
        start,
        width,
        json,
        file_paths,
    })
}

/// Extract the value for a long option. Handles both `--flag=value` and
/// `--flag value` forms.
fn long_opt_value(arg: &str, prefix: &str, args: &[String], i: &mut usize) -> String {
    let eq_prefix = format!("{prefix}=");
    if let Some(val) = arg.strip_prefix(&eq_prefix) {
        val.to_string()
    } else {
        *i += 1;
        if *i >= args.len() {
            eprintln!("nl: option '{prefix}' requires an argument");
            process::exit(1);
        }
        args[*i].clone()
    }
}

/// For short options that take a value: if there are more characters remaining
/// in the current option group, those characters form the value. Otherwise,
/// consume the next argument.
fn short_opt_value(
    chars: &[char],
    current_idx: usize,
    args: &[String],
    arg_idx: &mut usize,
    opt_char: char,
) -> String {
    if current_idx + 1 < chars.len() {
        chars[current_idx + 1..].iter().collect()
    } else {
        *arg_idx += 1;
        if *arg_idx >= args.len() {
            eprintln!("nl: option requires an argument -- '{opt_char}'");
            process::exit(1);
        }
        args[*arg_idx].clone()
    }
}

fn parse_i64_arg(flag: &str, val: &str) -> i64 {
    match val.parse::<i64>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("nl: invalid number for {flag}: '{val}'");
            process::exit(1);
        }
    }
}

fn parse_usize_arg(flag: &str, val: &str) -> usize {
    match val.parse::<usize>() {
        Ok(n) => n,
        Err(_) => {
            eprintln!("nl: invalid number for {flag}: '{val}'");
            process::exit(1);
        }
    }
}

// ============================================================================
// Section delimiter detection
// ============================================================================

/// Build the three section delimiter lines from the two-character delimiter
/// string. Header = delimiter repeated 3 times, body = 2 times, footer = 1.
struct Delimiters {
    header: String,
    body: String,
    footer: String,
}

fn build_delimiters(delim: &str) -> Delimiters {
    let mut header = String::with_capacity(delim.len() * 3);
    let mut body = String::with_capacity(delim.len() * 2);

    for _ in 0..3 {
        header.push_str(delim);
    }
    for _ in 0..2 {
        body.push_str(delim);
    }

    Delimiters {
        header,
        body,
        footer: delim.to_string(),
    }
}

/// Check whether a line is a section delimiter. Returns the section type if it
/// matches. The line is compared after stripping the trailing newline (which
/// should already be stripped by the caller).
fn detect_section(line: &str, delims: &Delimiters) -> Option<Section> {
    // Check longest match first to avoid false positives (header delimiter
    // contains the body delimiter as a prefix).
    if line == delims.header {
        Some(Section::Header)
    } else if line == delims.body {
        Some(Section::Body)
    } else if line == delims.footer {
        Some(Section::Footer)
    } else {
        Option::None
    }
}

// ============================================================================
// Regex matching (simple, no external crate)
// ============================================================================

/// A minimal regex matcher supporting the subset commonly used with `nl -bp`:
/// - Literal characters
/// - `.` (any character)
/// - `^` (start of line) and `$` (end of line)
/// - `*` (zero or more of the preceding atom)
/// - `+` (one or more of the preceding atom)
/// - `?` (zero or one of the preceding atom)
/// - `[...]` and `[^...]` character classes
/// - `\d`, `\s`, `\w` shorthand classes
///
/// This is intentionally simple. A full regex engine would require an external
/// crate or thousands of lines; this covers the patterns people actually use
/// with `nl`.
fn regex_matches(pattern: &str, text: &str) -> bool {
    // If the pattern starts with `^`, it must match at the start.
    if let Some(rest) = pattern.strip_prefix('^') {
        return regex_match_here(rest, text);
    }

    // Otherwise, try matching at every position.
    for start in 0..=text.len() {
        if regex_match_here(pattern, &text[start..]) {
            return true;
        }
    }
    false
}

/// Try to match `pattern` at the beginning of `text`.
fn regex_match_here(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    // `$` at end of pattern matches end of text.
    if pattern == "$" {
        return text.is_empty();
    }

    let pat_bytes = pattern.as_bytes();
    let (atom_len, quantifier) = parse_atom_and_quantifier(pat_bytes);

    match quantifier {
        Quantifier::None => {
            // Single atom match, then continue.
            let atom_pat = &pattern[..atom_len];
            let rest_pat = &pattern[atom_len..];
            if let Some(consumed) = match_atom(atom_pat, text) {
                regex_match_here(rest_pat, &text[consumed..])
            } else {
                false
            }
        }
        Quantifier::Star => {
            let atom_pat = &pattern[..atom_len];
            let rest_pat = &pattern[atom_len + 1..]; // skip the `*`
            regex_match_star(atom_pat, rest_pat, text)
        }
        Quantifier::Plus => {
            let atom_pat = &pattern[..atom_len];
            let rest_pat = &pattern[atom_len + 1..]; // skip the `+`
            // One or more: must match at least once.
            if let Some(consumed) = match_atom(atom_pat, text) {
                regex_match_star(atom_pat, rest_pat, &text[consumed..])
            } else {
                false
            }
        }
        Quantifier::Question => {
            let atom_pat = &pattern[..atom_len];
            let rest_pat = &pattern[atom_len + 1..]; // skip the `?`
            // Try matching the atom (one occurrence), then try zero.
            if let Some(consumed) = match_atom(atom_pat, text)
                && regex_match_here(rest_pat, &text[consumed..]) {
                    return true;
                }
            regex_match_here(rest_pat, text)
        }
    }
}

/// Match zero or more occurrences of `atom_pat`, followed by `rest_pat`.
fn regex_match_star(atom_pat: &str, rest_pat: &str, text: &str) -> bool {
    // Try matching rest_pat at the current position (zero occurrences).
    let mut pos = 0;
    loop {
        if regex_match_here(rest_pat, &text[pos..]) {
            return true;
        }
        if pos >= text.len() {
            break;
        }
        if let Some(consumed) = match_atom(atom_pat, &text[pos..]) {
            pos += consumed;
        } else {
            break;
        }
    }
    false
}

#[derive(Clone, Copy)]
enum Quantifier {
    None,
    Star,
    Plus,
    Question,
}

/// Parse the next atom from `pat_bytes` and determine if it is followed by a
/// quantifier. Returns `(atom_byte_length, quantifier)`.
fn parse_atom_and_quantifier(pat: &[u8]) -> (usize, Quantifier) {
    if pat.is_empty() {
        return (0, Quantifier::None);
    }

    let atom_len = if pat[0] == b'\\' {
        // Escaped character: two bytes.
        if pat.len() >= 2 { 2 } else { 1 }
    } else if pat[0] == b'[' {
        // Character class: find the closing `]`.
        let mut j = 1;
        // If `]` appears immediately after `[` or `[^`, it is literal.
        if j < pat.len() && pat[j] == b'^' {
            j += 1;
        }
        if j < pat.len() && pat[j] == b']' {
            j += 1;
        }
        while j < pat.len() && pat[j] != b']' {
            j += 1;
        }
        if j < pat.len() {
            j + 1 // include the `]`
        } else {
            pat.len() // unterminated, consume everything
        }
    } else {
        1 // Single character (including `.`).
    };

    let quantifier = if atom_len < pat.len() {
        match pat[atom_len] {
            b'*' => Quantifier::Star,
            b'+' => Quantifier::Plus,
            b'?' => Quantifier::Question,
            _ => Quantifier::None,
        }
    } else {
        Quantifier::None
    };

    (atom_len, quantifier)
}

/// Try to match a single atom at the start of `text`. Returns `Some(bytes_consumed)`
/// on success, `None` on failure. `atom_pat` is the atom portion of the pattern
/// (no quantifier).
fn match_atom(atom_pat: &str, text: &str) -> Option<usize> {
    if text.is_empty() {
        return Option::None;
    }

    let pat_bytes = atom_pat.as_bytes();
    let text_bytes = text.as_bytes();

    if pat_bytes.is_empty() {
        return Option::None;
    }

    if pat_bytes[0] == b'.' {
        // Match any single character (byte).
        Some(1)
    } else if pat_bytes[0] == b'\\' && pat_bytes.len() >= 2 {
        // Shorthand classes.
        let ch = text_bytes[0];
        match pat_bytes[1] {
            b'd' => {
                if ch.is_ascii_digit() {
                    Some(1)
                } else {
                    Option::None
                }
            }
            b's' => {
                if ch.is_ascii_whitespace() {
                    Some(1)
                } else {
                    Option::None
                }
            }
            b'w' => {
                if ch.is_ascii_alphanumeric() || ch == b'_' {
                    Some(1)
                } else {
                    Option::None
                }
            }
            literal => {
                if ch == literal {
                    Some(1)
                } else {
                    Option::None
                }
            }
        }
    } else if pat_bytes[0] == b'[' {
        // Character class.
        match_char_class(atom_pat, text_bytes[0])
    } else {
        // Literal byte.
        if text_bytes[0] == pat_bytes[0] {
            Some(1)
        } else {
            Option::None
        }
    }
}

/// Match a `[...]` or `[^...]` character class against a single byte.
/// Returns `Some(1)` if the byte matches, `None` otherwise.
fn match_char_class(class_pat: &str, ch: u8) -> Option<usize> {
    let bytes = class_pat.as_bytes();
    // Skip the opening `[`.
    let mut pos = 1;
    let negate = if pos < bytes.len() && bytes[pos] == b'^' {
        pos += 1;
        true
    } else {
        false
    };

    let mut matched = false;

    // Handle `]` as a literal if it appears first.
    if pos < bytes.len() && bytes[pos] == b']' {
        if ch == b']' {
            matched = true;
        }
        pos += 1;
    }

    while pos < bytes.len() && bytes[pos] != b']' {
        // Range: a-z
        if pos + 2 < bytes.len() && bytes[pos + 1] == b'-' && bytes[pos + 2] != b']' {
            let lo = bytes[pos];
            let hi = bytes[pos + 2];
            if ch >= lo && ch <= hi {
                matched = true;
            }
            pos += 3;
        } else {
            if bytes[pos] == ch {
                matched = true;
            }
            pos += 1;
        }
    }

    if negate {
        matched = !matched;
    }

    if matched { Some(1) } else { Option::None }
}

// ============================================================================
// Line numbering logic
// ============================================================================

/// Determine whether a line should be numbered under the given style.
fn should_number(line: &str, style: &NumberingStyle) -> bool {
    match style {
        NumberingStyle::All => true,
        NumberingStyle::NonEmpty => !line.is_empty(),
        NumberingStyle::None => false,
        NumberingStyle::Regex(pat) => regex_matches(pat, line),
    }
}

/// Format a line number according to the configured format and width.
fn format_number(num: i64, format: NumberFormat, width: usize) -> String {
    match format {
        NumberFormat::LeftNoZero => format!("{num:<width$}"),
        NumberFormat::RightNoZero => format!("{num:>width$}"),
        NumberFormat::RightZero => format!("{num:>0width$}"),
    }
}

/// Format a blank (unnumbered) prefix: spaces of the same width as the number
/// field, followed by the separator.
fn blank_prefix(width: usize, separator: &str) -> String {
    let mut s = String::with_capacity(width + separator.len());
    for _ in 0..width {
        s.push(' ');
    }
    s.push_str(separator);
    s
}

// ============================================================================
// JSON output helpers
// ============================================================================

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

/// A processed output line, either numbered or unnumbered.
struct OutputLine {
    /// Line number if this line was numbered, `None` otherwise.
    number: Option<i64>,
    /// The original text of the line (without trailing newline).
    text: String,
}

/// Process all input lines from a reader, applying section detection and
/// numbering. Returns the collected output lines.
fn process_lines<R: BufRead>(
    reader: &mut R,
    config: &Config,
    line_number: &mut i64,
) -> io::Result<Vec<OutputLine>> {
    let delims = build_delimiters(&config.section_delimiter);
    let mut section = Section::Body;
    let mut results: Vec<OutputLine> = Vec::new();

    let mut raw_line = String::new();
    loop {
        raw_line.clear();
        let bytes_read = reader.read_line(&mut raw_line)?;
        if bytes_read == 0 {
            break; // EOF
        }

        // Strip trailing newline / carriage-return.
        let line = raw_line.trim_end_matches(['\n', '\r']);

        // Check for section delimiter.
        if let Some(new_section) = detect_section(line, &delims) {
            section = new_section;
            if !config.no_renumber && new_section == Section::Header {
                *line_number = config.start;
            }
            // Delimiter lines are replaced by an empty unnumbered line.
            results.push(OutputLine {
                number: Option::None,
                text: String::new(),
            });
            continue;
        }

        let style = match section {
            Section::Header => &config.header_style,
            Section::Body => &config.body_style,
            Section::Footer => &config.footer_style,
        };

        if should_number(line, style) {
            results.push(OutputLine {
                number: Some(*line_number),
                text: line.to_string(),
            });
            *line_number += config.increment;
        } else {
            results.push(OutputLine {
                number: Option::None,
                text: line.to_string(),
            });
        }
    }

    Ok(results)
}

/// Write output lines in text format.
fn write_text<W: Write>(
    out: &mut W,
    lines: &[OutputLine],
    config: &Config,
) -> io::Result<()> {
    let blank = blank_prefix(config.width, &config.separator);

    for ol in lines {
        match ol.number {
            Some(num) => {
                let num_str = format_number(num, config.number_format, config.width);
                write!(out, "{num_str}{}{}", config.separator, ol.text)?;
                writeln!(out)?;
            }
            Option::None => {
                write!(out, "{blank}{}", ol.text)?;
                writeln!(out)?;
            }
        }
    }

    Ok(())
}

/// Write output lines in JSON format.
fn write_json<W: Write>(
    out: &mut W,
    lines: &[OutputLine],
) -> io::Result<()> {
    writeln!(out, "[")?;
    let total = lines.len();
    for (idx, ol) in lines.iter().enumerate() {
        let escaped = json_escape(&ol.text);
        match ol.number {
            Some(num) => {
                write!(out, "  {{\"number\":{num},\"text\":\"{escaped}\"}}")?;
            }
            Option::None => {
                write!(out, "  {{\"number\":null,\"text\":\"{escaped}\"}}")?;
            }
        }
        if idx + 1 < total {
            writeln!(out, ",")?;
        } else {
            writeln!(out)?;
        }
    }
    writeln!(out, "]")?;
    Ok(())
}

// ============================================================================
// File processing driver
// ============================================================================

/// Process a single source (file or stdin) and return its output lines.
fn process_source(
    path: &str,
    config: &Config,
    line_number: &mut i64,
) -> io::Result<Vec<OutputLine>> {
    if path == "-" {
        let stdin = io::stdin();
        let mut reader = BufReader::new(stdin.lock());
        process_lines(&mut reader, config, line_number)
    } else {
        let file = File::open(path).map_err(|e| {
            io::Error::new(e.kind(), format!("{path}: {e}"))
        })?;
        let mut reader = BufReader::new(file);
        process_lines(&mut reader, config, line_number)
    }
}

/// Process all files and produce output. Returns the exit code.
fn run(config: &Config) -> i32 {
    let sources: Vec<String> = if config.file_paths.is_empty() {
        vec!["-".to_string()]
    } else {
        config.file_paths.clone()
    };

    let mut line_number = config.start;
    let mut all_lines: Vec<OutputLine> = Vec::new();
    let mut had_error = false;

    for path in &sources {
        match process_source(path, config, &mut line_number) {
            Ok(lines) => all_lines.extend(lines),
            Err(e) => {
                eprintln!("nl: {e}");
                had_error = true;
            }
        }
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    let write_result = if config.json {
        write_json(&mut out, &all_lines)
    } else {
        write_text(&mut out, &all_lines, config)
    };

    if let Err(e) = write_result
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("nl: write error: {e}");
            return 1;
        }

    if let Err(e) = out.flush()
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("nl: write error: {e}");
            return 1;
        }

    if had_error { 1 } else { 0 }
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("SlateOS nl v{VERSION}");
    println!();
    println!("Write each FILE to standard output, with line numbers added.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  nl [OPTION]... [FILE]...");
    println!();
    println!("NUMBERING STYLE:");
    println!("  -b, --body-numbering=STYLE      Numbering for body lines (default: t)");
    println!("  -h, --header-numbering=STYLE    Numbering for header lines (default: n)");
    println!("  -f, --footer-numbering=STYLE    Numbering for footer lines (default: n)");
    println!();
    println!("  STYLE is one of:");
    println!("    a      Number all lines");
    println!("    t      Number only non-empty lines");
    println!("    n      Number no lines");
    println!("    pBRE   Number only lines that match the basic regular expression BRE");
    println!();
    println!("FORMAT AND LAYOUT:");
    println!("  -d, --section-delimiter=CC      Section delimiter characters (default: \\:)");
    println!("  -i, --line-increment=N          Line number increment (default: 1)");
    println!("  -n, --number-format=FORMAT      Line number format (default: rn)");
    println!("                                    ln  left justified");
    println!("                                    rn  right justified");
    println!("                                    rz  right justified, leading zeros");
    println!("  -p, --no-renumber               Do not reset line numbers at sections");
    println!("  -s, --number-separator=STRING   String after line number (default: TAB)");
    println!("  -v, --starting-line-number=N    First line number (default: 1)");
    println!("  -w, --number-width=N            Line number field width (default: 6)");
    println!();
    println!("OTHER OPTIONS:");
    println!("      --json                      Output as JSON array");
    println!("      --help                      Display this help and exit");
    println!("      --version                   Output version information and exit");
    println!();
    println!("SECTION DELIMITERS:");
    println!("  Lines consisting solely of the delimiter characters signal logical page");
    println!("  sections. The default delimiter is \\:, making the section markers:");
    println!("    \\:\\:\\:   Start of header");
    println!("    \\:\\:     Start of body");
    println!("    \\:       Start of footer");
    println!("  Delimiter lines are replaced by empty lines in the output.");
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
            println!("nl (SlateOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}
