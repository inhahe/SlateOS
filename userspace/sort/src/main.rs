//! OurOS `sort` Utility -- Sort Lines of Text
//!
//! Reads lines from files (or stdin) and writes them to stdout (or a file) in
//! sorted order. Modeled after GNU coreutils `sort` with the same flag set.
//!
//! # Usage
//!
//! ```text
//! sort [OPTION]... [FILE]...
//!
//! Write sorted concatenation of all FILE(s) to standard output.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -r, --reverse               Reverse the result of comparisons
//!   -n, --numeric-sort           Compare according to string numeric value
//!   -h, --human-numeric-sort     Compare human-readable numbers (e.g., 2K, 1G)
//!   -f, --ignore-case            Fold lower case to upper case characters
//!   -d, --dictionary-order       Consider only blanks and alphanumeric characters
//!   -b, --ignore-leading-blanks  Ignore leading blanks
//!   -u, --unique                 With -c, check for strict ordering;
//!                                without -c, output only the first of an equal run
//!   -k, --key=KEYDEF             Sort via a key; KEYDEF gives location and type
//!   -t, --field-separator=SEP    Use SEP instead of non-blank to blank transition
//!   -o, --output=FILE            Write result to FILE instead of standard output
//!   -s, --stable                 Stabilize sort by disabling last-resort comparison
//!   -c, --check                  Check for sorted input; do not sort
//!   -m, --merge                  Merge already sorted files; do not sort
//!   -V, --version-sort           Natural sort of (version) numbers within text
//!       --help                   Display this help and exit
//!       --version                Output version information and exit
//! ```

use std::cmp::Ordering;
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Sort mode flags
// ============================================================================

/// Per-key sort modifiers. These can be set globally or per key definition.
#[derive(Clone, Debug)]
struct SortModifiers {
    numeric: bool,
    human_numeric: bool,
    ignore_case: bool,
    dictionary_order: bool,
    ignore_leading_blanks: bool,
    reverse: bool,
    version_sort: bool,
}

impl SortModifiers {
    fn new() -> Self {
        Self {
            numeric: false,
            human_numeric: false,
            ignore_case: false,
            dictionary_order: false,
            ignore_leading_blanks: false,
            reverse: false,
            version_sort: false,
        }
    }
}

// ============================================================================
// Key definition
// ============================================================================

/// A key specification from `-k START[OPTS][,END[OPTS]]`.
#[derive(Clone, Debug)]
struct KeyDef {
    /// 1-based start field number.
    start_field: usize,
    /// 1-based start character position within the field (0 = entire field).
    start_char: usize,
    /// Optional 1-based end field number. `None` means end of line.
    end_field: Option<usize>,
    /// 1-based end character position within the end field (0 = end of field).
    end_char: usize,
    /// Modifiers for this specific key.
    modifiers: SortModifiers,
}

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Input file paths. "-" means stdin.
    file_paths: Vec<String>,
    /// Global sort modifiers (applied when a key has no per-key modifiers).
    global_mods: SortModifiers,
    /// Key definitions from -k options.
    keys: Vec<KeyDef>,
    /// Field separator character. `None` means whitespace runs (GNU default).
    field_separator: Option<char>,
    /// Output file path. `None` means stdout.
    output_file: Option<String>,
    /// Use stable sort.
    stable: bool,
    /// Only output unique lines (after sorting).
    unique: bool,
    /// Check if input is sorted instead of sorting.
    check: bool,
    /// Merge already-sorted files.
    merge: bool,
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

/// Parse a key definition string like "2,2" or "3.2n,5r" or "2n".
fn parse_keydef(spec: &str, global_mods: &SortModifiers) -> Result<KeyDef, String> {
    let parts: Vec<&str> = spec.splitn(2, ',').collect();

    let (start_field, start_char, start_mods) = parse_field_spec(parts[0])?;

    let (end_field, end_char, end_mods) = if parts.len() > 1 {
        let (f, c, m) = parse_field_spec(parts[1])?;
        (Some(f), c, m)
    } else {
        (None, 0, SortModifiers::new())
    };

    // Merge modifiers: start opts override global, end opts override start.
    let mut modifiers = global_mods.clone();
    apply_modifiers(&mut modifiers, &start_mods);
    apply_modifiers(&mut modifiers, &end_mods);

    if start_field == 0 {
        return Err("field number must be positive".to_string());
    }
    if let Some(ef) = end_field
        && ef == 0 {
            return Err("field number must be positive".to_string());
        }

    Ok(KeyDef {
        start_field,
        start_char,
        end_field,
        end_char,
        modifiers,
    })
}

/// Parse "FIELD[.CHAR][OPTS]" into (field, char_pos, modifiers).
fn parse_field_spec(s: &str) -> Result<(usize, usize, SortModifiers), String> {
    let mut mods = SortModifiers::new();
    let mut has_any_mod = false;

    // Find where the trailing option letters start: scan from end backwards
    // through known modifier letters.
    let bytes = s.as_bytes();
    let mut num_end = bytes.len();
    while num_end > 0 {
        match bytes[num_end - 1] {
            b'n' => { mods.numeric = true; has_any_mod = true; }
            b'r' => { mods.reverse = true; has_any_mod = true; }
            b'f' => { mods.ignore_case = true; has_any_mod = true; }
            b'b' => { mods.ignore_leading_blanks = true; has_any_mod = true; }
            b'h' => { mods.human_numeric = true; has_any_mod = true; }
            b'd' => { mods.dictionary_order = true; has_any_mod = true; }
            b'V' => { mods.version_sort = true; has_any_mod = true; }
            _ => break,
        }
        num_end -= 1;
    }

    // If no modifiers were found, reset to empty so global applies.
    if !has_any_mod {
        mods = SortModifiers::new();
    }

    let num_part = &s[..num_end];

    // Split on '.' for field.char
    let (field, char_pos) = if let Some(dot_idx) = num_part.find('.') {
        let field_str = &num_part[..dot_idx];
        let char_str = &num_part[dot_idx + 1..];
        let field = field_str
            .parse::<usize>()
            .map_err(|_| format!("invalid field number: '{field_str}'"))?;
        let char_pos = if char_str.is_empty() {
            0
        } else {
            char_str
                .parse::<usize>()
                .map_err(|_| format!("invalid character position: '{char_str}'"))?
        };
        (field, char_pos)
    } else {
        let field = num_part
            .parse::<usize>()
            .map_err(|_| format!("invalid field number: '{num_part}'"))?;
        (field, 0)
    };

    Ok((field, char_pos, mods))
}

/// Apply non-default modifiers from `src` onto `dst`.
fn apply_modifiers(dst: &mut SortModifiers, src: &SortModifiers) {
    if src.numeric {
        dst.numeric = true;
    }
    if src.human_numeric {
        dst.human_numeric = true;
    }
    if src.ignore_case {
        dst.ignore_case = true;
    }
    if src.dictionary_order {
        dst.dictionary_order = true;
    }
    if src.ignore_leading_blanks {
        dst.ignore_leading_blanks = true;
    }
    if src.reverse {
        dst.reverse = true;
    }
    if src.version_sort {
        dst.version_sort = true;
    }
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut file_paths: Vec<String> = Vec::new();
    let mut global_mods = SortModifiers::new();
    let mut keys: Vec<KeyDef> = Vec::new();
    let mut field_separator: Option<char> = None;
    let mut output_file: Option<String> = None;
    let mut stable = false;
    let mut unique = false;
    let mut check = false;
    let mut merge = false;
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
            if arg == "--reverse" {
                global_mods.reverse = true;
            } else if arg == "--numeric-sort" {
                global_mods.numeric = true;
            } else if arg == "--human-numeric-sort" {
                global_mods.human_numeric = true;
            } else if arg == "--ignore-case" {
                global_mods.ignore_case = true;
            } else if arg == "--dictionary-order" {
                global_mods.dictionary_order = true;
            } else if arg == "--ignore-leading-blanks" {
                global_mods.ignore_leading_blanks = true;
            } else if arg == "--unique" {
                unique = true;
            } else if arg == "--stable" {
                stable = true;
            } else if arg == "--check" {
                check = true;
            } else if arg == "--merge" {
                merge = true;
            } else if arg == "--version-sort" {
                global_mods.version_sort = true;
            } else if arg == "--help" {
                return ParseResult::Help;
            } else if arg == "--version" {
                return ParseResult::Version;
            } else if arg == "--key" || arg.starts_with("--key=") {
                let spec = if let Some(eq_val) = arg.strip_prefix("--key=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("sort: option '--key' requires an argument");
                        process::exit(2);
                    }
                    args[i].clone()
                };
                match parse_keydef(&spec, &global_mods) {
                    Ok(kd) => keys.push(kd),
                    Err(e) => {
                        eprintln!("sort: invalid key definition '{spec}': {e}");
                        process::exit(2);
                    }
                }
            } else if arg == "--field-separator" || arg.starts_with("--field-separator=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--field-separator=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("sort: option '--field-separator' requires an argument");
                        process::exit(2);
                    }
                    args[i].clone()
                };
                let mut chars = val.chars();
                match chars.next() {
                    Some(c) => field_separator = Some(c),
                    None => {
                        eprintln!("sort: empty field separator");
                        process::exit(2);
                    }
                }
            } else if arg == "--output" || arg.starts_with("--output=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--output=") {
                    eq_val.to_string()
                } else {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("sort: option '--output' requires an argument");
                        process::exit(2);
                    }
                    args[i].clone()
                };
                output_file = Some(val);
            } else {
                eprintln!("sort: unrecognized option '{arg}'");
                eprintln!("Try 'sort --help' for more information.");
                process::exit(2);
            }

            i += 1;
            continue;
        }

        // Short options. Some take arguments (-k, -t, -o).
        let arg_bytes = arg.as_bytes();
        let mut j = 1;
        while j < arg_bytes.len() {
            match arg_bytes[j] {
                b'r' => global_mods.reverse = true,
                b'n' => global_mods.numeric = true,
                b'h' => global_mods.human_numeric = true,
                b'f' => global_mods.ignore_case = true,
                b'd' => global_mods.dictionary_order = true,
                b'b' => global_mods.ignore_leading_blanks = true,
                b'u' => unique = true,
                b's' => stable = true,
                b'c' => check = true,
                b'm' => merge = true,
                b'V' => global_mods.version_sort = true,
                b'k' => {
                    // -k takes the rest of this arg or the next arg.
                    let rest = &arg[j + 1..];
                    let spec = if rest.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("sort: option '-k' requires an argument");
                            process::exit(2);
                        }
                        args[i].clone()
                    } else {
                        rest.to_string()
                    };
                    match parse_keydef(&spec, &global_mods) {
                        Ok(kd) => keys.push(kd),
                        Err(e) => {
                            eprintln!("sort: invalid key definition '{spec}': {e}");
                            process::exit(2);
                        }
                    }
                    // Consumed rest of this arg cluster.
                    j = arg_bytes.len();
                    continue;
                }
                b't' => {
                    let rest = &arg[j + 1..];
                    let val = if rest.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("sort: option '-t' requires an argument");
                            process::exit(2);
                        }
                        args[i].clone()
                    } else {
                        rest.to_string()
                    };
                    let mut chars = val.chars();
                    match chars.next() {
                        Some(c) => field_separator = Some(c),
                        None => {
                            eprintln!("sort: empty field separator");
                            process::exit(2);
                        }
                    }
                    j = arg_bytes.len();
                    continue;
                }
                b'o' => {
                    let rest = &arg[j + 1..];
                    let val = if rest.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("sort: option '-o' requires an argument");
                            process::exit(2);
                        }
                        args[i].clone()
                    } else {
                        rest.to_string()
                    };
                    output_file = Some(val);
                    j = arg_bytes.len();
                    continue;
                }
                _ => {
                    let ch = arg_bytes[j] as char;
                    eprintln!("sort: invalid option -- '{ch}'");
                    eprintln!("Try 'sort --help' for more information.");
                    process::exit(2);
                }
            }
            j += 1;
        }

        i += 1;
    }

    if file_paths.is_empty() {
        file_paths.push("-".to_string());
    }

    ParseResult::Run(Config {
        file_paths,
        global_mods,
        keys,
        field_separator,
        output_file,
        stable,
        unique,
        check,
        merge,
    })
}

// ============================================================================
// Input reading
// ============================================================================

/// Read all lines from the given sources into a single Vec.
fn read_all_lines(paths: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();

    for path in paths {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("sort: {path}: {e}");
                    continue;
                }
            }
        };

        let buf = BufReader::new(reader);
        for line_result in buf.lines() {
            match line_result {
                Ok(l) => lines.push(l),
                Err(e) => {
                    eprintln!("sort: {path}: read error: {e}");
                    break;
                }
            }
        }
    }

    lines
}

// ============================================================================
// Field extraction
// ============================================================================

/// Split a line into fields using the given separator.
///
/// With a separator character, fields are separated by exactly that character.
/// Without a separator (whitespace mode), runs of whitespace delimit fields,
/// and leading whitespace is skipped.
fn split_fields(line: &str, sep: Option<char>) -> Vec<&str> {
    match sep {
        Some(c) => line.split(c).collect(),
        None => line.split_whitespace().collect(),
    }
}

/// Extract the key substring from a line given a key definition.
fn extract_key<'a>(line: &'a str, key: &KeyDef, sep: Option<char>) -> &'a str {
    let fields = split_fields(line, sep);

    if fields.is_empty() {
        return "";
    }

    // Start field (1-based). If out of range, return empty.
    let sf = key.start_field.saturating_sub(1);
    if sf >= fields.len() {
        return "";
    }

    // End field. If not specified, goes to end of line.
    let ef = match key.end_field {
        Some(e) => {
            let idx = e.saturating_sub(1);
            if idx >= fields.len() {
                fields.len() - 1
            } else {
                idx
            }
        }
        None => fields.len() - 1,
    };

    if sf > ef {
        return "";
    }

    // For single-field keys with no char positions, just return the field.
    if sf == ef && key.start_char == 0 && key.end_char == 0 {
        return fields[sf];
    }

    // For multi-field or char-positioned keys, work with byte offsets in the
    // original line to return a proper substring.
    let start_field_str = fields[sf];
    let end_field_str = fields[ef];

    // Calculate byte offsets relative to the line.
    // SAFETY justification: both field str slices are substrings of `line`,
    // so pointer arithmetic gives valid byte offsets.
    let line_start = line.as_ptr() as usize;
    let field_start_offset = start_field_str.as_ptr() as usize - line_start;
    let field_end_offset =
        end_field_str.as_ptr() as usize - line_start + end_field_str.len();

    // Apply character positions within start/end fields.
    let start_byte = if key.start_char > 0 {
        let skip = key.start_char.saturating_sub(1);
        let mut byte_off = field_start_offset;
        let mut chars_skipped = 0;
        for (idx, _) in start_field_str.char_indices() {
            if chars_skipped >= skip {
                byte_off = field_start_offset + idx;
                break;
            }
            chars_skipped += 1;
            byte_off = field_start_offset + idx;
        }
        if chars_skipped < skip {
            // Character position is beyond the field; start at end.
            field_start_offset + start_field_str.len()
        } else {
            byte_off
        }
    } else {
        field_start_offset
    };

    let end_byte = if key.end_char > 0 {
        // end_char is inclusive: include up to and including that character.
        let target = key.end_char;
        let mut byte_off = field_end_offset;
        let mut count = 0;
        for (idx, ch) in end_field_str.char_indices() {
            count += 1;
            if count >= target {
                let end_of_field_start = end_field_str.as_ptr() as usize - line_start;
                byte_off = end_of_field_start + idx + ch.len_utf8();
                break;
            }
        }
        byte_off
    } else {
        field_end_offset
    };

    if start_byte >= line.len() {
        return "";
    }
    let actual_end = end_byte.min(line.len());
    if start_byte >= actual_end {
        return "";
    }
    &line[start_byte..actual_end]
}

// ============================================================================
// Comparison helpers
// ============================================================================

/// Parse the leading numeric value from a string for -n comparison.
/// Returns 0.0 for non-numeric strings (matching GNU sort behavior).
fn parse_leading_number(s: &str) -> f64 {
    let trimmed = s.trim_start();
    if trimmed.is_empty() {
        return 0.0;
    }

    let mut end = 0;
    let bytes = trimmed.as_bytes();

    // Optional sign.
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }

    let mut saw_dot = false;
    while end < bytes.len() {
        if bytes[end].is_ascii_digit() {
            end += 1;
        } else if bytes[end] == b'.' && !saw_dot {
            saw_dot = true;
            end += 1;
        } else {
            break;
        }
    }

    if end == 0 || (end == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) {
        return 0.0;
    }

    trimmed[..end].parse::<f64>().unwrap_or(0.0)
}

/// Parse a human-readable number like "3.5K", "10M", "2G".
/// Returns the value with suffix multiplier applied.
fn parse_human_number(s: &str) -> f64 {
    let trimmed = s.trim_start();
    if trimmed.is_empty() {
        return 0.0;
    }

    let mut end = 0;
    let bytes = trimmed.as_bytes();

    // Optional sign.
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }

    let mut saw_dot = false;
    while end < bytes.len() {
        if bytes[end].is_ascii_digit() {
            end += 1;
        } else if bytes[end] == b'.' && !saw_dot {
            saw_dot = true;
            end += 1;
        } else {
            break;
        }
    }

    if end == 0 || (end == 1 && (bytes[0] == b'-' || bytes[0] == b'+')) {
        return 0.0;
    }

    let base: f64 = trimmed[..end].parse().unwrap_or(0.0);

    // Check for suffix.
    let suffix_multiplier = if end < bytes.len() {
        match bytes[end] {
            b'K' | b'k' => 1_000.0,
            b'M' => 1_000_000.0,
            b'G' => 1_000_000_000.0,
            b'T' => 1_000_000_000_000.0,
            b'P' => 1_000_000_000_000_000.0,
            b'E' => 1_000_000_000_000_000_000.0,
            _ => 1.0,
        }
    } else {
        1.0
    };

    base * suffix_multiplier
}

/// Compare two strings as version numbers. Splits each string into alternating
/// runs of non-digit and digit segments, comparing non-digit runs
/// lexicographically and digit runs numerically.
fn compare_version(a: &str, b: &str) -> Ordering {
    let segs_a = version_segments(a);
    let segs_b = version_segments(b);

    let max_len = segs_a.len().max(segs_b.len());
    for i in 0..max_len {
        let seg_a = segs_a.get(i);
        let seg_b = segs_b.get(i);

        match (seg_a, seg_b) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(sa), Some(sb)) => {
                let ord = compare_version_segment(sa, sb);
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
    Ordering::Equal
}

/// A segment is either a run of digits or a run of non-digits.
#[derive(Debug)]
enum VersionSegment<'a> {
    Text(&'a str),
    Digits(&'a str),
}

/// Split a string into alternating text/digit segments.
fn version_segments(s: &str) -> Vec<VersionSegment<'_>> {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            result.push(VersionSegment::Digits(&s[start..i]));
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_digit() {
                i += 1;
            }
            result.push(VersionSegment::Text(&s[start..i]));
        }
    }

    result
}

/// Compare two version segments.
fn compare_version_segment(a: &VersionSegment<'_>, b: &VersionSegment<'_>) -> Ordering {
    match (a, b) {
        (VersionSegment::Digits(da), VersionSegment::Digits(db)) => {
            // Compare numerically. Strip leading zeros for comparison.
            let na = da.trim_start_matches('0');
            let nb = db.trim_start_matches('0');
            // Longer number (after stripping zeros) is larger.
            match na.len().cmp(&nb.len()) {
                Ordering::Equal => na.cmp(nb),
                other => other,
            }
        }
        (VersionSegment::Text(ta), VersionSegment::Text(tb)) => ta.cmp(tb),
        // Digits sort after text (GNU sort convention for version-sort).
        (VersionSegment::Digits(_), VersionSegment::Text(_)) => Ordering::Greater,
        (VersionSegment::Text(_), VersionSegment::Digits(_)) => Ordering::Less,
    }
}

/// Apply dictionary-order filtering: keep only blanks and alphanumerics.
fn dictionary_filter(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect()
}

/// Compare two strings according to the given modifiers.
fn compare_with_modifiers(a: &str, b: &str, mods: &SortModifiers) -> Ordering {
    let mut a_work: String;
    let mut b_work: String;
    let mut a_ref: &str = a;
    let mut b_ref: &str = b;

    // Ignore leading blanks.
    if mods.ignore_leading_blanks {
        a_ref = a_ref.trim_start();
        b_ref = b_ref.trim_start();
    }

    // Dictionary order: filter to blanks + alphanumerics.
    if mods.dictionary_order {
        a_work = dictionary_filter(a_ref);
        b_work = dictionary_filter(b_ref);
        a_ref = &a_work;
        b_ref = &b_work;
    }

    // Fold case.
    if mods.ignore_case {
        a_work = a_ref.to_uppercase();
        b_work = b_ref.to_uppercase();
        a_ref = &a_work;
        b_ref = &b_work;
    }

    let cmp = if mods.human_numeric {
        let na = parse_human_number(a_ref);
        let nb = parse_human_number(b_ref);
        na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
    } else if mods.numeric {
        let na = parse_leading_number(a_ref);
        let nb = parse_leading_number(b_ref);
        na.partial_cmp(&nb).unwrap_or(Ordering::Equal)
    } else if mods.version_sort {
        compare_version(a_ref, b_ref)
    } else {
        a_ref.cmp(b_ref)
    };

    if mods.reverse { cmp.reverse() } else { cmp }
}

// ============================================================================
// Main comparison function
// ============================================================================

/// Compare two lines according to all key definitions.
fn compare_lines(
    a: &str,
    b: &str,
    keys: &[KeyDef],
    global_mods: &SortModifiers,
    sep: Option<char>,
    stable: bool,
) -> Ordering {
    if keys.is_empty() {
        // No -k: compare entire lines with global modifiers.
        let cmp = compare_with_modifiers(a, b, global_mods);
        if cmp != Ordering::Equal {
            return cmp;
        }
    } else {
        // Compare by each key in order.
        for key in keys {
            let key_a = extract_key(a, key, sep);
            let key_b = extract_key(b, key, sep);
            let cmp = compare_with_modifiers(key_a, key_b, &key.modifiers);
            if cmp != Ordering::Equal {
                return cmp;
            }
        }
    }

    // Last-resort comparison: raw byte comparison of the whole line, unless
    // stable sort is requested (which suppresses last-resort to preserve
    // original order of equal elements).
    if stable {
        Ordering::Equal
    } else {
        a.cmp(b)
    }
}

// ============================================================================
// Check mode
// ============================================================================

/// Check if the input is already sorted. Returns 0 if sorted, 1 if not.
fn check_sorted(
    lines: &[String],
    keys: &[KeyDef],
    global_mods: &SortModifiers,
    sep: Option<char>,
    stable: bool,
    unique: bool,
) -> i32 {
    for i in 1..lines.len() {
        let cmp = compare_lines(&lines[i - 1], &lines[i], keys, global_mods, sep, stable);
        let disordered = if unique {
            // With -u, strict ordering required (no equal neighbors).
            cmp != Ordering::Less
        } else {
            cmp == Ordering::Greater
        };
        if disordered {
            let line_num = i + 1;
            eprintln!("sort: -:{}:{}: disorder: {}", line_num, line_num, &lines[i]);
            return 1;
        }
    }
    0
}

// ============================================================================
// Merge mode
// ============================================================================

/// (path, line iterator) for a single input source in merge mode.
type MergeReader = (String, io::Lines<BufReader<Box<dyn Read>>>);

/// Merge already-sorted files by reading one line at a time from each and
/// outputting the smallest.
fn merge_files(
    paths: &[String],
    keys: &[KeyDef],
    global_mods: &SortModifiers,
    sep: Option<char>,
    stable: bool,
    unique: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    // Open readers for all input files.
    let mut readers: Vec<MergeReader> = Vec::new();

    for path in paths {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("sort: {path}: {e}");
                    continue;
                }
            }
        };
        readers.push((path.clone(), BufReader::new(reader).lines()));
    }

    // Head buffer: next line from each reader.
    let mut heads: Vec<Option<String>> = Vec::with_capacity(readers.len());
    for (_path, lines_iter) in &mut readers {
        let next = lines_iter.next().and_then(|r| r.ok());
        heads.push(next);
    }

    let mut prev_output: Option<String> = None;

    loop {
        // Find the minimum head.
        let mut min_idx: Option<usize> = None;
        for (idx, head) in heads.iter().enumerate() {
            if let Some(line) = head {
                match min_idx {
                    None => min_idx = Some(idx),
                    Some(current_min) => {
                        if let Some(current_line) = &heads[current_min] {
                            let cmp = compare_lines(
                                line,
                                current_line,
                                keys,
                                global_mods,
                                sep,
                                stable,
                            );
                            if cmp == Ordering::Less {
                                min_idx = Some(idx);
                            }
                        }
                    }
                }
            }
        }

        let Some(idx) = min_idx else {
            break;
        };

        // Output the minimum line.
        let line = heads[idx].take();
        if let Some(line) = line {
            let should_output = if unique {
                match &prev_output {
                    Some(prev) => {
                        compare_lines(&line, prev, keys, global_mods, sep, stable)
                            != Ordering::Equal
                    }
                    None => true,
                }
            } else {
                true
            };

            if should_output {
                writeln!(out, "{line}")?;
                prev_output = Some(line);
            }

            // Advance that reader.
            let next = readers[idx].1.next().and_then(|r| r.ok());
            heads[idx] = next;
        }
    }

    Ok(())
}

// ============================================================================
// Output
// ============================================================================

/// Write sorted lines, applying -u deduplication.
fn write_output(
    lines: &[String],
    unique: bool,
    keys: &[KeyDef],
    global_mods: &SortModifiers,
    sep: Option<char>,
    stable: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    let mut prev: Option<&String> = None;

    for line in lines {
        if unique
            && let Some(p) = prev
                && compare_lines(p, line, keys, global_mods, sep, stable) == Ordering::Equal {
                    continue;
                }
        writeln!(out, "{line}")?;
        prev = Some(line);
    }

    Ok(())
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("OurOS sort v{VERSION}");
    println!();
    println!("Write sorted concatenation of all FILE(s) to standard output.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  sort [OPTION]... [FILE]...");
    println!();
    println!("ORDERING OPTIONS:");
    println!("  -r, --reverse               Reverse the result of comparisons");
    println!("  -n, --numeric-sort           Compare according to string numeric value");
    println!(
        "  -h, --human-numeric-sort     Compare human-readable numbers (e.g., 2K, 1G)"
    );
    println!("  -f, --ignore-case            Fold lower case to upper case characters");
    println!(
        "  -d, --dictionary-order       Consider only blanks and alphanumeric characters"
    );
    println!("  -b, --ignore-leading-blanks  Ignore leading blanks in sort keys");
    println!("  -V, --version-sort           Natural sort of (version) numbers within text");
    println!();
    println!("KEY OPTIONS:");
    println!("  -k, --key=KEYDEF             Sort via a key; KEYDEF gives location and type");
    println!("  -t, --field-separator=SEP    Use SEP instead of non-blank to blank transition");
    println!();
    println!("OUTPUT OPTIONS:");
    println!(
        "  -u, --unique                 Output only the first of an equal run"
    );
    println!("  -o, --output=FILE            Write result to FILE instead of standard output");
    println!("  -s, --stable                 Stabilize sort by disabling last-resort comparison");
    println!();
    println!("OTHER OPTIONS:");
    println!(
        "  -c, --check                  Check for sorted input; do not sort"
    );
    println!("  -m, --merge                  Merge already sorted files; do not sort");
    println!("      --help                   Display this help and exit");
    println!("      --version                Output version information and exit");
    println!();
    println!("KEYDEF is START[OPTS][,END[OPTS]] where START and END are field numbers");
    println!("(1-based) with optional .CHAR character positions. OPTS is one or more of");
    println!("[nbdfrhV] and overrides global ordering options for that key.");
    println!();
    println!("EXAMPLES:");
    println!("  sort file.txt                Sort lines alphabetically");
    println!("  sort -n numbers.txt          Sort numerically");
    println!("  sort -k 2,2 data.txt         Sort by second field");
    println!("  sort -t: -k 3n /etc/passwd   Sort passwd by UID (field 3, numeric)");
    println!("  sort -V versions.txt         Sort version numbers naturally");
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
            println!("sort (OurOS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let code = run(&config);
            process::exit(code);
        }
    }
}

/// Execute the sort/check/merge operation. Returns the process exit code.
fn run(config: &Config) -> i32 {
    // Open output destination.
    let stdout;
    let mut out: Box<dyn Write> = match &config.output_file {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(io::BufWriter::new(f)),
            Err(e) => {
                eprintln!("sort: {path}: {e}");
                return 2;
            }
        },
        None => {
            stdout = io::stdout();
            Box::new(io::BufWriter::new(stdout.lock()))
        }
    };

    if config.merge {
        if let Err(e) = merge_files(
            &config.file_paths,
            &config.keys,
            &config.global_mods,
            config.field_separator,
            config.stable,
            config.unique,
            &mut *out,
        )
            && e.kind() != io::ErrorKind::BrokenPipe {
                eprintln!("sort: write error: {e}");
                return 2;
            }
        return 0;
    }

    let mut lines = read_all_lines(&config.file_paths);

    if config.check {
        return check_sorted(
            &lines,
            &config.keys,
            &config.global_mods,
            config.field_separator,
            config.stable,
            config.unique,
        );
    }

    // Sort.
    let keys = &config.keys;
    let global_mods = &config.global_mods;
    let sep = config.field_separator;
    let stable = config.stable;

    if config.stable {
        lines.sort_by(|a, b| compare_lines(a, b, keys, global_mods, sep, stable));
    } else {
        lines.sort_unstable_by(|a, b| compare_lines(a, b, keys, global_mods, sep, stable));
    }

    if let Err(e) = write_output(
        &lines,
        config.unique,
        keys,
        global_mods,
        sep,
        stable,
        &mut *out,
    )
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("sort: write error: {e}");
            return 2;
        }

    // Flush output.
    if let Err(e) = out.flush()
        && e.kind() != io::ErrorKind::BrokenPipe {
            eprintln!("sort: write error: {e}");
            return 2;
        }

    0
}
