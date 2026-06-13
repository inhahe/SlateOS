//! Slate OS `column` Utility -- Columnate Text Formatter
//!
//! Formats input text into neatly aligned columns. Supports two primary modes:
//!
//! - **Fill mode** (default): arrange input words into columns that fill the
//!   terminal width, similar to the output of `ls`.
//! - **Table mode** (`-t`): parse delimited input into an aligned table with
//!   auto-detected column widths.
//!
//! # Usage
//!
//! ```text
//! column [OPTION]... [FILE]...
//!
//! Columnate lists or create aligned tables from delimited input.
//! With no FILE, or when FILE is -, read standard input.
//!
//!   -t, --table                 Create a table (determine columns from input)
//!   -s, --separator=CHARS       Input delimiter characters for table mode
//!   -o, --output-separator=STR  Output column separator (default: 2 spaces)
//!   -c, --columns=N             Terminal width (default: 80)
//!   -x, --fillrows              Fill rows before columns
//!   -n, --no-merge              Don't merge multiple adjacent delimiters
//!   -e, --empty                 Don't ignore empty lines
//!   -N, --table-columns=NAMES   Comma-separated column header names
//!   -H, --table-hide=COLS       Hide specified columns (comma-separated indices)
//!   -R, --table-right=COLS      Right-align specified columns (comma-separated)
//!   -J, --json                  Output as JSON array of objects
//!       --help                  Display this help and exit
//!       --version               Output version information and exit
//! ```

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const DEFAULT_TERM_WIDTH: usize = 80;
const DEFAULT_OUTPUT_SEP: &str = "  ";

// ============================================================================
// Unicode display width helpers
// ============================================================================

/// Return the display width of a single character.
///
/// East Asian wide and fullwidth characters occupy two columns. Most other
/// printable characters occupy one column. Control characters and zero-width
/// code points occupy zero columns.
fn char_display_width(ch: char) -> usize {
    let cp = ch as u32;

    // Zero-width characters.
    if cp == 0 {
        return 0;
    }
    // C0/C1 control characters (except tab which we treat as 1 for simplicity
    // in column-formatted output -- the caller should expand tabs first).
    if cp < 0x20 || (0x7F..=0x9F).contains(&cp) {
        return 0;
    }
    // Combining characters (a simplified range covering the common cases).
    if (0x0300..=0x036F).contains(&cp)    // Combining Diacritical Marks
        || (0x1AB0..=0x1AFF).contains(&cp) // Combining Diacritical Marks Extended
        || (0x1DC0..=0x1DFF).contains(&cp) // Combining Diacritical Marks Supplement
        || (0x20D0..=0x20FF).contains(&cp) // Combining Diacritical Marks for Symbols
        || (0xFE00..=0xFE0F).contains(&cp) // Variation Selectors
        || (0xFE20..=0xFE2F).contains(&cp) // Combining Half Marks
    {
        return 0;
    }
    // Soft hyphen.
    if cp == 0x00AD {
        return 0;
    }
    // Zero-width space, joiner, non-joiner, word joiner.
    if cp == 0x200B || cp == 0x200C || cp == 0x200D || cp == 0x2060 || cp == 0xFEFF {
        return 0;
    }

    // East Asian wide and fullwidth characters.
    if is_east_asian_wide(cp) {
        return 2;
    }

    1
}

/// Check whether a code point is East Asian Wide or Fullwidth per Unicode
/// East Asian Width property (simplified ranges covering CJK and related).
fn is_east_asian_wide(cp: u32) -> bool {
    // CJK Radicals Supplement .. Enclosed CJK Letters and Months
    (0x2E80..=0x33FF).contains(&cp)
    // CJK Compatibility
    || (0x3400..=0x4DBF).contains(&cp)
    // CJK Unified Ideographs
    || (0x4E00..=0x9FFF).contains(&cp)
    // Yi Syllables .. Yi Radicals
    || (0xA000..=0xA4CF).contains(&cp)
    // CJK Compatibility Ideographs
    || (0xF900..=0xFAFF).contains(&cp)
    // Fullwidth Forms (Fullwidth ASCII variants, Halfwidth Katakana are
    // narrow but Fullwidth Latin/symbols are wide).
    || (0xFF01..=0xFF60).contains(&cp)
    || (0xFFE0..=0xFFE6).contains(&cp)
    // CJK Unified Ideographs Extension B .. Extension I
    || (0x20000..=0x323AF).contains(&cp)
    // CJK Compatibility Ideographs Supplement
    || (0x2F800..=0x2FA1F).contains(&cp)
    // Hangul Syllables
    || (0xAC00..=0xD7AF).contains(&cp)
    // Hangul Jamo Extended-B
    || (0xD7B0..=0xD7FF).contains(&cp)
    // CJK Symbols and Punctuation, Hiragana, Katakana, Bopomofo
    || (0x3000..=0x312F).contains(&cp)
    // Katakana Phonetic Extensions
    || (0x31F0..=0x31FF).contains(&cp)
    // Enclosed CJK Letters continuation
    || (0x3200..=0x32FF).contains(&cp)
    // Kangxi Radicals
    || (0x2F00..=0x2FDF).contains(&cp)
}

/// Compute the display width of a string in terminal columns.
fn display_width(s: &str) -> usize {
    s.chars().map(char_display_width).sum()
}

// ============================================================================
// Parsed configuration
// ============================================================================

/// Fully parsed command-line configuration.
struct Config {
    /// Input file paths. `-` means stdin.
    file_paths: Vec<String>,
    /// Table mode (parse delimited columns).
    table: bool,
    /// Input separator characters for table mode.
    separator: Option<String>,
    /// Output separator string.
    output_separator: String,
    /// Terminal width for fill mode.
    term_width: usize,
    /// Fill rows before columns (default: fill columns first).
    fill_rows: bool,
    /// Don't merge adjacent delimiters.
    no_merge: bool,
    /// Don't ignore empty lines.
    keep_empty: bool,
    /// Explicit column header names.
    column_names: Option<Vec<String>>,
    /// Columns to hide (0-based indices).
    hide_columns: Vec<usize>,
    /// Columns to right-align (0-based indices).
    right_columns: Vec<usize>,
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

/// Parse a comma-separated list of 1-based column indices into 0-based indices.
/// Returns an error message on failure.
fn parse_column_indices(s: &str) -> Result<Vec<usize>, String> {
    let mut indices = Vec::new();
    for part in s.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        match trimmed.parse::<usize>() {
            Ok(0) => return Err("column index must be >= 1, got '0'".to_string()),
            Ok(n) => indices.push(n.saturating_sub(1)),
            Err(_) => return Err(format!("invalid column index: '{trimmed}'")),
        }
    }
    Ok(indices)
}

/// Consume the value for an option that takes an argument. Handles both
/// `--option=value` (returns the part after `=`) and `--option value` (returns
/// `args[i+1]` and increments `*idx`).
///
/// `flag_name` is used in error messages.
fn consume_option_value<'a>(
    args: &'a [String],
    idx: &mut usize,
    eq_value: Option<&'a str>,
    flag_name: &str,
) -> Result<&'a str, String> {
    if let Some(val) = eq_value {
        return Ok(val);
    }
    *idx += 1;
    if *idx >= args.len() {
        return Err(format!("column: option '{flag_name}' requires an argument"));
    }
    Ok(&args[*idx])
}

fn parse_args(args: &[String]) -> ParseResult {
    let mut file_paths: Vec<String> = Vec::new();
    let mut table = false;
    let mut separator: Option<String> = None;
    let mut output_separator: Option<String> = None;
    let mut term_width: usize = DEFAULT_TERM_WIDTH;
    let mut fill_rows = false;
    let mut no_merge = false;
    let mut keep_empty = false;
    let mut column_names: Option<Vec<String>> = None;
    let mut hide_columns: Vec<usize> = Vec::new();
    let mut right_columns: Vec<usize> = Vec::new();
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
            let (opt, eq_val) = match arg.find('=') {
                Some(pos) => (&arg[..pos], Some(&arg[pos + 1..])),
                None => (arg.as_str(), None),
            };

            match opt {
                "--table" => table = true,
                "--fillrows" => fill_rows = true,
                "--no-merge" => no_merge = true,
                "--empty" => keep_empty = true,
                "--json" => json = true,
                "--help" => return ParseResult::Help,
                "--version" => return ParseResult::Version,
                "--separator" => {
                    match consume_option_value(args, &mut i, eq_val, "--separator") {
                        Ok(val) => separator = Some(val.to_string()),
                        Err(msg) => {
                            eprintln!("{msg}");
                            process::exit(1);
                        }
                    }
                }
                "--output-separator" => {
                    match consume_option_value(args, &mut i, eq_val, "--output-separator") {
                        Ok(val) => output_separator = Some(val.to_string()),
                        Err(msg) => {
                            eprintln!("{msg}");
                            process::exit(1);
                        }
                    }
                }
                "--columns" => {
                    match consume_option_value(args, &mut i, eq_val, "--columns") {
                        Ok(val) => match val.parse::<usize>() {
                            Ok(n) if n > 0 => term_width = n,
                            _ => {
                                eprintln!("column: invalid column count: '{val}'");
                                process::exit(1);
                            }
                        },
                        Err(msg) => {
                            eprintln!("{msg}");
                            process::exit(1);
                        }
                    }
                }
                "--table-columns" => {
                    match consume_option_value(args, &mut i, eq_val, "--table-columns") {
                        Ok(val) => {
                            column_names =
                                Some(val.split(',').map(|s| s.trim().to_string()).collect());
                        }
                        Err(msg) => {
                            eprintln!("{msg}");
                            process::exit(1);
                        }
                    }
                }
                "--table-hide" => {
                    match consume_option_value(args, &mut i, eq_val, "--table-hide") {
                        Ok(val) => match parse_column_indices(val) {
                            Ok(indices) => hide_columns = indices,
                            Err(msg) => {
                                eprintln!("column: {msg}");
                                process::exit(1);
                            }
                        },
                        Err(msg) => {
                            eprintln!("{msg}");
                            process::exit(1);
                        }
                    }
                }
                "--table-right" => {
                    match consume_option_value(args, &mut i, eq_val, "--table-right") {
                        Ok(val) => match parse_column_indices(val) {
                            Ok(indices) => right_columns = indices,
                            Err(msg) => {
                                eprintln!("column: {msg}");
                                process::exit(1);
                            }
                        },
                        Err(msg) => {
                            eprintln!("{msg}");
                            process::exit(1);
                        }
                    }
                }
                _ => {
                    eprintln!("column: unrecognized option '{arg}'");
                    eprintln!("Try 'column --help' for more information.");
                    process::exit(1);
                }
            }

            i += 1;
            continue;
        }

        // Short options. Options that take arguments consume the rest of the
        // current argv element or the next argv element.
        let short = &arg[1..];
        let mut chars = short.chars();
        while let Some(ch) = chars.next() {
            match ch {
                't' => table = true,
                'x' => fill_rows = true,
                'n' => no_merge = true,
                'e' => keep_empty = true,
                'J' => json = true,
                's' => {
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("column: option '-s' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    separator = Some(val);
                    break;
                }
                'o' => {
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("column: option '-o' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    output_separator = Some(val);
                    break;
                }
                'c' => {
                    let remainder: String = chars.collect();
                    let val_str = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("column: option '-c' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    match val_str.parse::<usize>() {
                        Ok(n) if n > 0 => term_width = n,
                        _ => {
                            eprintln!("column: invalid column count: '{val_str}'");
                            process::exit(1);
                        }
                    }
                    break;
                }
                'N' => {
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("column: option '-N' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    column_names = Some(val.split(',').map(|s| s.trim().to_string()).collect());
                    break;
                }
                'H' => {
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("column: option '-H' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    match parse_column_indices(&val) {
                        Ok(indices) => hide_columns = indices,
                        Err(msg) => {
                            eprintln!("column: {msg}");
                            process::exit(1);
                        }
                    }
                    break;
                }
                'R' => {
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i += 1;
                        if i >= args.len() {
                            eprintln!("column: option '-R' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    match parse_column_indices(&val) {
                        Ok(indices) => right_columns = indices,
                        Err(msg) => {
                            eprintln!("column: {msg}");
                            process::exit(1);
                        }
                    }
                    break;
                }
                _ => {
                    eprintln!("column: invalid option -- '{ch}'");
                    eprintln!("Try 'column --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    if file_paths.is_empty() {
        file_paths.push("-".to_string());
    }

    ParseResult::Run(Config {
        file_paths,
        table,
        separator,
        output_separator: output_separator.unwrap_or_else(|| DEFAULT_OUTPUT_SEP.to_string()),
        term_width,
        fill_rows,
        no_merge,
        keep_empty,
        column_names,
        hide_columns,
        right_columns,
        json,
    })
}

// ============================================================================
// Input reading
// ============================================================================

/// Read all lines from the specified inputs.
fn read_all_lines(file_paths: &[String], keep_empty: bool) -> io::Result<Vec<String>> {
    let mut lines = Vec::new();
    let stdin = io::stdin();

    for path in file_paths {
        let reader: Box<dyn BufRead> = if path == "-" {
            Box::new(stdin.lock())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(BufReader::new(f)),
                Err(e) => {
                    eprintln!("column: {path}: {e}");
                    continue;
                }
            }
        };

        for line_result in reader.lines() {
            let line = line_result?;
            if !keep_empty && line.is_empty() {
                continue;
            }
            lines.push(line);
        }
    }

    Ok(lines)
}

// ============================================================================
// Fill mode
// ============================================================================

/// Collect all words from input lines (splitting on whitespace).
fn collect_words(lines: &[String]) -> Vec<String> {
    let mut words = Vec::new();
    for line in lines {
        for word in line.split_whitespace() {
            if !word.is_empty() {
                words.push(word.to_string());
            }
        }
    }
    words
}

/// Fill-columns mode (default): arrange words into columns, filling down each
/// column before moving right.
fn fill_columns(words: &[String], term_width: usize, output_sep: &str) -> Vec<String> {
    if words.is_empty() {
        return Vec::new();
    }

    let sep_width = display_width(output_sep);

    // Binary search for the maximum number of columns that fits.
    let max_cols = words.len();
    let mut best_ncols = 1usize;

    // Try increasing number of columns until it doesn't fit.
    for ncols in 2..=max_cols {
        let nrows = words.len().div_ceil(ncols);
        let mut col_widths = vec![0usize; ncols];

        // Determine each column's width.
        for (idx, word) in words.iter().enumerate() {
            let col = idx / nrows;
            if col >= ncols {
                break;
            }
            let w = display_width(word);
            if w > col_widths[col] {
                col_widths[col] = w;
            }
        }

        // Total width: sum of column widths + separators between them.
        let total: usize = col_widths.iter().sum::<usize>()
            + if ncols > 1 { sep_width * (ncols - 1) } else { 0 };

        if total <= term_width {
            best_ncols = ncols;
        } else {
            break;
        }
    }

    let ncols = best_ncols;
    let nrows = words.len().div_ceil(ncols);

    // Compute column widths for the chosen layout.
    let mut col_widths = vec![0usize; ncols];
    for (idx, word) in words.iter().enumerate() {
        let col = idx / nrows;
        if col >= ncols {
            break;
        }
        let w = display_width(word);
        if w > col_widths[col] {
            col_widths[col] = w;
        }
    }

    // Build output lines.
    let mut output = Vec::with_capacity(nrows);
    for row in 0..nrows {
        let mut line = String::new();
        for (col, target) in col_widths.iter().copied().enumerate().take(ncols) {
            let idx = col * nrows + row;
            if idx >= words.len() {
                break;
            }
            if col > 0 {
                line.push_str(output_sep);
            }
            let word = &words[idx];
            line.push_str(word);
            // Pad to column width (except for the last column).
            if col + 1 < ncols {
                let w = display_width(word);
                if w < target {
                    for _ in 0..(target - w) {
                        line.push(' ');
                    }
                }
            }
        }
        output.push(line);
    }

    output
}

/// Fill-rows mode (`-x`): arrange words into columns, filling across each row
/// before moving down.
fn fill_rows(words: &[String], term_width: usize, output_sep: &str) -> Vec<String> {
    if words.is_empty() {
        return Vec::new();
    }

    let sep_width = display_width(output_sep);

    // Try increasing number of columns until it doesn't fit.
    let max_cols = words.len();
    let mut best_ncols = 1usize;

    for ncols in 2..=max_cols {
        let nrows = words.len().div_ceil(ncols);
        let mut col_widths = vec![0usize; ncols];

        // With fill-rows, word at position idx goes to row = idx / ncols,
        // col = idx % ncols.
        for (idx, word) in words.iter().enumerate() {
            let col = idx % ncols;
            let w = display_width(word);
            if w > col_widths[col] {
                col_widths[col] = w;
            }
        }

        let total: usize = col_widths.iter().sum::<usize>()
            + if ncols > 1 { sep_width * (ncols - 1) } else { 0 };

        if total <= term_width {
            best_ncols = ncols;
        } else {
            break;
        }
        // If all words fit on one row, stop.
        if nrows == 1 {
            break;
        }
    }

    let ncols = best_ncols;
    let nrows = words.len().div_ceil(ncols);

    // Compute column widths for the chosen layout.
    let mut col_widths = vec![0usize; ncols];
    for (idx, word) in words.iter().enumerate() {
        let col = idx % ncols;
        let w = display_width(word);
        if w > col_widths[col] {
            col_widths[col] = w;
        }
    }

    // Build output lines.
    let mut output = Vec::with_capacity(nrows);
    for row in 0..nrows {
        let mut line = String::new();
        for (col, target) in col_widths.iter().copied().enumerate().take(ncols) {
            let idx = row * ncols + col;
            if idx >= words.len() {
                break;
            }
            if col > 0 {
                line.push_str(output_sep);
            }
            let word = &words[idx];
            line.push_str(word);
            // Pad to column width (except for the last column on the row).
            let is_last_in_row = col + 1 >= ncols || (row * ncols + col + 1) >= words.len();
            if !is_last_in_row {
                let w = display_width(word);
                if w < target {
                    for _ in 0..(target - w) {
                        line.push(' ');
                    }
                }
            }
        }
        output.push(line);
    }

    output
}

/// Run fill mode: read all input, collect words, arrange into columns.
fn run_fill_mode(config: &Config) -> io::Result<i32> {
    let lines = read_all_lines(&config.file_paths, config.keep_empty)?;
    let words = collect_words(&lines);

    if words.is_empty() {
        return Ok(0);
    }

    let output_lines = if config.fill_rows {
        fill_rows(&words, config.term_width, &config.output_separator)
    } else {
        fill_columns(&words, config.term_width, &config.output_separator)
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in &output_lines {
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
    }
    out.flush()?;

    Ok(0)
}

// ============================================================================
// Table mode
// ============================================================================

/// Split a line into fields using the given separator characters.
///
/// When `merge` is true (the default), adjacent delimiters are treated as a
/// single delimiter. When false (`--no-merge`), each delimiter produces a field
/// boundary (empty fields are preserved).
fn split_fields(line: &str, sep: &Option<String>, merge: bool) -> Vec<String> {
    match sep {
        Some(sep_chars) if !sep_chars.is_empty() => {
            if merge {
                // Split on runs of any separator character.
                let mut fields = Vec::new();
                let mut current = String::new();
                let mut in_delim = true;

                for ch in line.chars() {
                    if sep_chars.contains(ch) {
                        if !in_delim {
                            fields.push(current.clone());
                            current.clear();
                            in_delim = true;
                        }
                        // Skip additional consecutive delimiters.
                    } else {
                        in_delim = false;
                        current.push(ch);
                    }
                }
                if !in_delim {
                    fields.push(current);
                }
                fields
            } else {
                // No merge: every delimiter produces a boundary.
                let mut fields = Vec::new();
                let mut current = String::new();

                for ch in line.chars() {
                    if sep_chars.contains(ch) {
                        fields.push(current.clone());
                        current.clear();
                    } else {
                        current.push(ch);
                    }
                }
                fields.push(current);
                fields
            }
        }
        _ => {
            // Default: split on whitespace, merge adjacent.
            if merge {
                line.split_whitespace().map(|s| s.to_string()).collect()
            } else {
                // Split on individual whitespace characters without merging.
                let mut fields = Vec::new();
                let mut current = String::new();

                for ch in line.chars() {
                    if ch.is_whitespace() {
                        fields.push(current.clone());
                        current.clear();
                    } else {
                        current.push(ch);
                    }
                }
                fields.push(current);
                fields
            }
        }
    }
}

/// Pad a string to a given display width, respecting alignment.
fn pad_field(field: &str, target_width: usize, right_align: bool) -> String {
    let current_width = display_width(field);
    if current_width >= target_width {
        return field.to_string();
    }
    let padding = target_width - current_width;
    let pad_str: String = std::iter::repeat_n(' ', padding).collect();
    if right_align {
        let mut result = pad_str;
        result.push_str(field);
        result
    } else {
        let mut result = field.to_string();
        result.push_str(&pad_str);
        result
    }
}

/// Run table mode: parse input into fields, compute column widths, output
/// aligned table.
fn run_table_mode(config: &Config) -> io::Result<i32> {
    let lines = read_all_lines(&config.file_paths, config.keep_empty)?;
    let merge = !config.no_merge;

    // Parse each line into fields.
    let mut rows: Vec<Vec<String>> = Vec::new();
    for line in &lines {
        let fields = split_fields(line, &config.separator, merge);
        if fields.is_empty() && !config.keep_empty {
            continue;
        }
        rows.push(fields);
    }

    if rows.is_empty() {
        return Ok(0);
    }

    // Determine the number of columns.
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if ncols == 0 {
        return Ok(0);
    }

    // Prepare header row if column names were specified.
    let has_header = config.column_names.is_some();
    let mut all_rows: Vec<Vec<String>> = Vec::new();

    if let Some(ref names) = config.column_names {
        let mut header = names.clone();
        // Pad or truncate header to match column count.
        while header.len() < ncols {
            header.push(String::new());
        }
        all_rows.push(header);
    }
    all_rows.extend(rows);

    // Ensure all rows have the same number of columns (pad with empty strings).
    let total_cols = all_rows.iter().map(|r| r.len()).max().unwrap_or(0);
    for row in &mut all_rows {
        while row.len() < total_cols {
            row.push(String::new());
        }
    }

    if config.json {
        return output_json(config, &all_rows, total_cols, has_header);
    }

    // Compute column display widths (considering hidden columns are excluded).
    let visible_cols: Vec<usize> = (0..total_cols)
        .filter(|c| !config.hide_columns.contains(c))
        .collect();

    let mut col_widths = vec![0usize; total_cols];
    for row in &all_rows {
        for (col_idx, field) in row.iter().enumerate() {
            if config.hide_columns.contains(&col_idx) {
                continue;
            }
            let w = display_width(field);
            if w > col_widths[col_idx] {
                col_widths[col_idx] = w;
            }
        }
    }

    // Output the table.
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for row in &all_rows {
        let mut first = true;
        for (vi, &col_idx) in visible_cols.iter().enumerate() {
            if !first {
                out.write_all(config.output_separator.as_bytes())?;
            }
            first = false;

            let field = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
            let is_last_visible = vi + 1 >= visible_cols.len();
            let right_align = config.right_columns.contains(&col_idx);

            if is_last_visible {
                // Don't pad the last column (no trailing whitespace), unless
                // right-aligned.
                if right_align {
                    let padded = pad_field(field, col_widths[col_idx], true);
                    out.write_all(padded.as_bytes())?;
                } else {
                    out.write_all(field.as_bytes())?;
                }
            } else {
                let padded = pad_field(field, col_widths[col_idx], right_align);
                out.write_all(padded.as_bytes())?;
            }
        }
        out.write_all(b"\n")?;
    }

    out.flush()?;
    Ok(0)
}

// ============================================================================
// JSON output
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

/// Output the table as JSON. If column names are provided, output as array of
/// objects. Otherwise output as array of arrays.
fn output_json(
    config: &Config,
    all_rows: &[Vec<String>],
    total_cols: usize,
    has_header: bool,
) -> io::Result<i32> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let visible_cols: Vec<usize> = (0..total_cols)
        .filter(|c| !config.hide_columns.contains(c))
        .collect();

    // Determine header keys. If explicit names were given, use the first row as
    // keys and start data from row 1. Otherwise, generate generic keys.
    let (keys, data_start): (Vec<String>, usize) = if has_header {
        let header_row = &all_rows[0];
        let keys: Vec<String> = visible_cols
            .iter()
            .map(|&ci| {
                header_row
                    .get(ci)
                    .cloned()
                    .unwrap_or_else(|| format!("column{}", ci + 1))
            })
            .collect();
        (keys, 1)
    } else {
        let keys: Vec<String> = visible_cols
            .iter()
            .map(|&ci| format!("column{}", ci + 1))
            .collect();
        (keys, 0)
    };

    out.write_all(b"[\n")?;

    let data_rows = &all_rows[data_start..];
    for (row_idx, row) in data_rows.iter().enumerate() {
        out.write_all(b"  {")?;
        for (ki, key) in keys.iter().enumerate() {
            if ki > 0 {
                out.write_all(b", ")?;
            }
            write_json_string(&mut out, key)?;
            out.write_all(b": ")?;
            let col_idx = visible_cols[ki];
            let val = row.get(col_idx).map(|s| s.as_str()).unwrap_or("");
            write_json_string(&mut out, val)?;
        }
        out.write_all(b"}")?;
        if row_idx + 1 < data_rows.len() {
            out.write_all(b",")?;
        }
        out.write_all(b"\n")?;
    }

    out.write_all(b"]\n")?;
    out.flush()?;
    Ok(0)
}

// ============================================================================
// Help text
// ============================================================================

fn print_help() {
    println!("Slate OS column v{VERSION}");
    println!();
    println!("Columnate lists or create aligned tables from delimited input.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  column [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -t, --table                 Create a table from delimited input");
    println!("  -s, --separator=CHARS       Input delimiter character(s) (default: whitespace)");
    println!("  -o, --output-separator=STR  Output column separator (default: 2 spaces)");
    println!("  -c, --columns=N             Terminal width override (default: {DEFAULT_TERM_WIDTH})");
    println!("  -x, --fillrows              Fill rows before columns (default: columns first)");
    println!("  -n, --no-merge              Don't merge multiple adjacent delimiters");
    println!("  -e, --empty                 Don't ignore empty lines");
    println!("  -N, --table-columns=NAMES   Comma-separated column header names");
    println!("  -H, --table-hide=COLS       Hide specified columns (1-based, comma-separated)");
    println!("  -R, --table-right=COLS      Right-align specified columns (1-based)");
    println!("  -J, --json                  Output as JSON array of objects");
    println!("      --help                  Display this help and exit");
    println!("      --version               Output version information and exit");
    println!();
    println!("MODES:");
    println!("  Default (fill): Reads input words and arranges them into columns");
    println!("  that fill the terminal width, similar to `ls` output.");
    println!();
    println!("  Table (-t): Parses each input line into fields using the delimiter,");
    println!("  determines column widths, and outputs an aligned table.");
    println!();
    println!("EXAMPLES:");
    println!("  ls | column                  Columnate ls output");
    println!("  column -t /etc/fstab         Format fstab as an aligned table");
    println!("  echo 'a:b:c' | column -t -s ':'  Table with colon separator");
    println!("  column -t -N 'Name,Size,Type' -R 2  Table with headers, col 2 right-aligned");
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
            println!("column (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let result = if config.table || config.json {
                run_table_mode(&config)
            } else {
                run_fill_mode(&config)
            };

            match result {
                Ok(code) => process::exit(code),
                Err(e) => {
                    eprintln!("column: {e}");
                    process::exit(1);
                }
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Display width tests ---

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("a b c"), 5);
    }

    #[test]
    fn test_display_width_cjk() {
        // Each CJK ideograph is 2 columns wide.
        assert_eq!(display_width("\u{4e16}\u{754c}"), 4); // "world" in Chinese
        assert_eq!(display_width("a\u{4e16}b"), 4); // a(1) + CJK(2) + b(1)
    }

    #[test]
    fn test_display_width_combining() {
        // 'e' + combining acute accent = 1 column.
        assert_eq!(display_width("e\u{0301}"), 1);
    }

    #[test]
    fn test_display_width_zero_width() {
        // Zero-width space should contribute nothing.
        assert_eq!(display_width("a\u{200B}b"), 2);
    }

    #[test]
    fn test_display_width_fullwidth() {
        // Fullwidth 'A' (U+FF21) is 2 columns wide.
        assert_eq!(display_width("\u{FF21}"), 2);
    }

    // --- Fill mode tests ---

    #[test]
    fn test_fill_columns_basic() {
        let words: Vec<String> = vec!["a", "b", "c", "d", "e", "f"]
            .into_iter()
            .map(String::from)
            .collect();
        let result = fill_columns(&words, 80, "  ");
        // With width 80 and 1-char words, all should fit on one line.
        assert_eq!(result.len(), 1);
        assert!(result[0].contains("a"));
        assert!(result[0].contains("f"));
    }

    #[test]
    fn test_fill_columns_narrow_width() {
        let words: Vec<String> = vec!["alpha", "beta", "gamma", "delta"]
            .into_iter()
            .map(String::from)
            .collect();
        // Width of 10: "alpha" is 5, "beta" is 4. Two cols would need at least
        // 5+2+5 = 12 > 10, so we get 1 column.
        let result = fill_columns(&words, 10, "  ");
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "alpha");
        assert_eq!(result[1], "beta");
        assert_eq!(result[2], "gamma");
        assert_eq!(result[3], "delta");
    }

    #[test]
    fn test_fill_columns_two_columns() {
        let words: Vec<String> = vec!["aaa", "bbb", "ccc", "ddd"]
            .into_iter()
            .map(String::from)
            .collect();
        // Width 12: two cols => 3+2+3 = 8, fits. Three cols => 3+2+3+2+3 = 13 > 12.
        let result = fill_columns(&words, 12, "  ");
        // 4 words, 2 cols => 2 rows. Column-first order: col0=[aaa,bbb], col1=[ccc,ddd].
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "aaa  ccc");
        assert_eq!(result[1], "bbb  ddd");
    }

    #[test]
    fn test_fill_columns_empty() {
        let words: Vec<String> = Vec::new();
        let result = fill_columns(&words, 80, "  ");
        assert!(result.is_empty());
    }

    #[test]
    fn test_fill_rows_basic() {
        let words: Vec<String> = vec!["aaa", "bbb", "ccc", "ddd"]
            .into_iter()
            .map(String::from)
            .collect();
        // Width 12: two cols => 3+2+3 = 8, fits.
        let result = fill_rows(&words, 12, "  ");
        // Row-first order: row0=[aaa,bbb], row1=[ccc,ddd].
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "aaa  bbb");
        assert_eq!(result[1], "ccc  ddd");
    }

    #[test]
    fn test_fill_rows_single_column() {
        let words: Vec<String> = vec!["longword1", "longword2"]
            .into_iter()
            .map(String::from)
            .collect();
        // Width 10: "longword1" is 9. Two cols => 9+2+9 = 20 > 10. One col.
        let result = fill_rows(&words, 10, "  ");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "longword1");
        assert_eq!(result[1], "longword2");
    }

    #[test]
    fn test_fill_rows_empty() {
        let words: Vec<String> = Vec::new();
        let result = fill_rows(&words, 80, "  ");
        assert!(result.is_empty());
    }

    #[test]
    fn test_fill_columns_uneven() {
        // 5 words in 2 cols => 3 rows. col0 gets 3, col1 gets 2.
        let words: Vec<String> = vec!["a", "b", "c", "d", "e"]
            .into_iter()
            .map(String::from)
            .collect();
        let _result = fill_columns(&words, 80, "  ");
        // With such tiny words, they likely all fit on one row. Let's use
        // narrow width to force 2 cols.
        let result2 = fill_columns(&words, 6, "  ");
        // 1-char words: 2 cols need 1+2+1 = 4, fits in 6.
        // 3 cols need 1+2+1+2+1 = 7 > 6, so 2 cols.
        // 2 cols, 5 words => 3 rows. Col-first: col0=[a,b,c], col1=[d,e].
        assert_eq!(result2.len(), 3);
        assert_eq!(result2[0], "a  d");
        assert_eq!(result2[1], "b  e");
        assert_eq!(result2[2], "c");
    }

    // --- Table mode / field splitting tests ---

    #[test]
    fn test_split_fields_whitespace_merge() {
        let fields = split_fields("  hello   world  ", &None, true);
        assert_eq!(fields, vec!["hello", "world"]);
    }

    #[test]
    fn test_split_fields_whitespace_no_merge() {
        let fields = split_fields("a  b", &None, false);
        // 'a', '', 'b' -- two spaces produce an empty field between.
        assert_eq!(fields, vec!["a", "", "b"]);
    }

    #[test]
    fn test_split_fields_custom_separator_merge() {
        let fields = split_fields("a::b::c", &Some(":".to_string()), true);
        assert_eq!(fields, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_split_fields_custom_separator_no_merge() {
        let fields = split_fields("a::b", &Some(":".to_string()), false);
        assert_eq!(fields, vec!["a", "", "b"]);
    }

    #[test]
    fn test_split_fields_multi_char_separator() {
        let fields = split_fields("a,b;c", &Some(",;".to_string()), true);
        assert_eq!(fields, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_split_fields_empty_line() {
        let fields = split_fields("", &None, true);
        assert!(fields.is_empty());
    }

    #[test]
    fn test_split_fields_only_delimiters() {
        let fields = split_fields(":::", &Some(":".to_string()), true);
        assert!(fields.is_empty());

        let fields_no_merge = split_fields(":::", &Some(":".to_string()), false);
        assert_eq!(fields_no_merge, vec!["", "", "", ""]);
    }

    // --- Padding / alignment tests ---

    #[test]
    fn test_pad_field_left() {
        assert_eq!(pad_field("hi", 5, false), "hi   ");
    }

    #[test]
    fn test_pad_field_right() {
        assert_eq!(pad_field("hi", 5, true), "   hi");
    }

    #[test]
    fn test_pad_field_exact() {
        assert_eq!(pad_field("hello", 5, false), "hello");
    }

    #[test]
    fn test_pad_field_wider_than_target() {
        assert_eq!(pad_field("toolong", 3, false), "toolong");
    }

    // --- Column index parsing tests ---

    #[test]
    fn test_parse_column_indices_valid() {
        let result = parse_column_indices("1,3,5").unwrap();
        assert_eq!(result, vec![0, 2, 4]); // 1-based -> 0-based
    }

    #[test]
    fn test_parse_column_indices_zero_invalid() {
        assert!(parse_column_indices("0").is_err());
    }

    #[test]
    fn test_parse_column_indices_non_numeric() {
        assert!(parse_column_indices("a,b").is_err());
    }

    #[test]
    fn test_parse_column_indices_empty_parts() {
        let result = parse_column_indices("1,,2,").unwrap();
        assert_eq!(result, vec![0, 1]);
    }

    // --- Argument parsing tests ---

    #[test]
    fn test_parse_args_defaults() {
        let args: Vec<String> = vec!["column".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert!(!config.table);
                assert!(!config.fill_rows);
                assert!(!config.no_merge);
                assert!(!config.keep_empty);
                assert!(!config.json);
                assert_eq!(config.term_width, 80);
                assert_eq!(config.output_separator, "  ");
                assert!(config.separator.is_none());
                assert_eq!(config.file_paths, vec!["-"]);
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_table_mode() {
        let args: Vec<String> = vec!["column".into(), "-t".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => assert!(config.table),
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_long_table() {
        let args: Vec<String> = vec!["column".into(), "--table".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => assert!(config.table),
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_separator() {
        let args: Vec<String> = vec!["column".into(), "-t".into(), "-s".into(), ":".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert!(config.table);
                assert_eq!(config.separator, Some(":".to_string()));
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_separator_attached() {
        let args: Vec<String> = vec!["column".into(), "-ts:".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert!(config.table);
                assert_eq!(config.separator, Some(":".to_string()));
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_output_separator_long() {
        let args: Vec<String> =
            vec!["column".into(), "--output-separator= | ".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert_eq!(config.output_separator, " | ");
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_columns() {
        let args: Vec<String> = vec!["column".into(), "-c".into(), "120".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => assert_eq!(config.term_width, 120),
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_fillrows() {
        let args: Vec<String> = vec!["column".into(), "-x".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => assert!(config.fill_rows),
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_file_paths() {
        let args: Vec<String> =
            vec!["column".into(), "-t".into(), "file1".into(), "file2".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert_eq!(config.file_paths, vec!["file1", "file2"]);
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_dash_as_stdin() {
        let args: Vec<String> = vec!["column".into(), "-".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert_eq!(config.file_paths, vec!["-"]);
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_help() {
        let args: Vec<String> = vec!["column".into(), "--help".into()];
        assert!(matches!(parse_args(&args), ParseResult::Help));
    }

    #[test]
    fn test_parse_args_version() {
        let args: Vec<String> = vec!["column".into(), "--version".into()];
        assert!(matches!(parse_args(&args), ParseResult::Version));
    }

    #[test]
    fn test_parse_args_double_dash() {
        // After --, everything is a file path even if it starts with '-'.
        let args: Vec<String> = vec!["column".into(), "--".into(), "-t".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert!(!config.table);
                assert_eq!(config.file_paths, vec!["-t"]);
            }
            _ => panic!("Expected Run"),
        }
    }

    // --- JSON output tests ---

    #[test]
    fn test_json_string_escape() {
        let mut buf = Vec::new();
        write_json_string(&mut buf, "hello \"world\"").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "\"hello \\\"world\\\"\"");
    }

    #[test]
    fn test_json_string_special_chars() {
        let mut buf = Vec::new();
        write_json_string(&mut buf, "a\tb\nc\\d").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "\"a\\tb\\nc\\\\d\"");
    }

    #[test]
    fn test_json_string_control_char() {
        let mut buf = Vec::new();
        write_json_string(&mut buf, "\x01").unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "\"\\u0001\"");
    }

    // --- Collect words tests ---

    #[test]
    fn test_collect_words_basic() {
        let lines = vec!["hello world".to_string(), "  foo  bar  ".to_string()];
        let words = collect_words(&lines);
        assert_eq!(words, vec!["hello", "world", "foo", "bar"]);
    }

    #[test]
    fn test_collect_words_empty() {
        let lines: Vec<String> = Vec::new();
        let words = collect_words(&lines);
        assert!(words.is_empty());
    }

    #[test]
    fn test_collect_words_blank_lines() {
        let lines = vec!["".to_string(), "  ".to_string(), "word".to_string()];
        let words = collect_words(&lines);
        assert_eq!(words, vec!["word"]);
    }

    // --- East Asian width in fill mode ---

    #[test]
    fn test_fill_columns_with_cjk() {
        let words: Vec<String> = vec![
            "\u{4e16}\u{754c}".to_string(), // width 4
            "ab".to_string(),                // width 2
            "\u{4e16}".to_string(),          // width 2
        ];
        // Width 12: col widths [4, 2, 2] + 2 seps * 2 = 12. Fits in 3 cols.
        let result = fill_columns(&words, 12, "  ");
        // 3 words, 3 cols, 1 row.
        assert_eq!(result.len(), 1);
    }

    // --- Table mode integration tests ---

    #[test]
    fn test_table_basic_alignment() {
        // Simulate table mode manually: split + pad.
        let lines = vec![
            "Name Age City".to_string(),
            "Alice 30 NYC".to_string(),
            "Bob 25 LA".to_string(),
        ];
        let merge = true;
        let mut rows: Vec<Vec<String>> = Vec::new();
        for line in &lines {
            rows.push(split_fields(line, &None, merge));
        }

        let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        assert_eq!(ncols, 3);

        // Compute column widths.
        let mut widths = vec![0usize; ncols];
        for row in &rows {
            for (ci, field) in row.iter().enumerate() {
                let w = display_width(field);
                if w > widths[ci] {
                    widths[ci] = w;
                }
            }
        }
        assert_eq!(widths, vec![5, 3, 4]); // Alice=5, Age=3, City=4

        // Pad first row.
        let padded0 = pad_field(&rows[0][0], widths[0], false);
        assert_eq!(padded0, "Name ");
    }

    #[test]
    fn test_table_right_alignment() {
        let field = "42";
        let padded = pad_field(field, 6, true);
        assert_eq!(padded, "    42");
    }

    #[test]
    fn test_char_width_hangul() {
        // Hangul syllable (U+AC00) should be 2 columns.
        assert_eq!(char_display_width('\u{AC00}'), 2);
    }

    #[test]
    fn test_char_width_tab() {
        // Tab is < 0x20 so it returns 0 (control character).
        assert_eq!(char_display_width('\t'), 0);
    }

    #[test]
    fn test_char_width_normal() {
        assert_eq!(char_display_width('A'), 1);
        assert_eq!(char_display_width(' '), 1);
    }

    #[test]
    fn test_single_word_fill() {
        let words = vec!["hello".to_string()];
        let result = fill_columns(&words, 80, "  ");
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn test_single_word_fill_rows() {
        let words = vec!["hello".to_string()];
        let result = fill_rows(&words, 80, "  ");
        assert_eq!(result, vec!["hello"]);
    }

    #[test]
    fn test_fill_columns_custom_separator() {
        let words: Vec<String> = vec!["a", "b", "c", "d"]
            .into_iter()
            .map(String::from)
            .collect();
        // With " | " (3 chars) separator, 2 cols need 1+3+1 = 5.
        let result = fill_columns(&words, 5, " | ");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "a | c");
        assert_eq!(result[1], "b | d");
    }

    #[test]
    fn test_split_fields_trailing_delimiter() {
        let fields = split_fields("a:b:", &Some(":".to_string()), false);
        assert_eq!(fields, vec!["a", "b", ""]);
    }

    #[test]
    fn test_split_fields_leading_delimiter_no_merge() {
        let fields = split_fields(":a:b", &Some(":".to_string()), false);
        assert_eq!(fields, vec!["", "a", "b"]);
    }

    #[test]
    fn test_parse_args_json_flag() {
        let args: Vec<String> = vec!["column".into(), "-J".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => assert!(config.json),
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_no_merge() {
        let args: Vec<String> = vec!["column".into(), "-tn".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => {
                assert!(config.table);
                assert!(config.no_merge);
            }
            _ => panic!("Expected Run"),
        }
    }

    #[test]
    fn test_parse_args_keep_empty() {
        let args: Vec<String> = vec!["column".into(), "-e".into()];
        match parse_args(&args) {
            ParseResult::Run(config) => assert!(config.keep_empty),
            _ => panic!("Expected Run"),
        }
    }
}
