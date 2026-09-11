//! Slate OS `expand` / `unexpand` Utility -- Tab/Space Conversion
//!
//! Converts tabs to spaces (`expand`) or spaces to tabs (`unexpand`),
//! depending on the name used to invoke the program (argv[0]).
//!
//! # Usage
//!
//! ```text
//! expand [OPTION]... [FILE]...
//!
//!   Convert tabs in each FILE to spaces, writing to standard output.
//!   With no FILE, or when FILE is -, read standard input.
//!
//!   -i, --initial        Only convert leading tabs
//!   -t, --tabs=N         Set tab stops every N columns (default 8)
//!   -t, --tabs=N1,N2,... Comma-separated list of tab stop positions
//!       --help           Display this help and exit
//!       --version        Output version information and exit
//!
//! unexpand [OPTION]... [FILE]...
//!
//!   Convert spaces in each FILE to tabs, writing to standard output.
//!
//!   -a, --all            Convert all sequences of spaces, not just leading
//!   -t, --tabs=N         Set tab stops every N columns (default 8)
//!       --first-only     Only convert leading spaces (default)
//!       --help           Display this help and exit
//!       --version        Output version information and exit
//! ```

use quoting::{quoteaf_os, quotef_os};
use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";

// ============================================================================
// Types
// ============================================================================

/// Whether we are running as `expand` or `unexpand`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Expand,
    Unexpand,
}

/// Tab stop specification.
#[derive(Clone)]
enum TabStops {
    /// Regular interval (e.g. every 8 columns).
    Regular(std::num::NonZeroUsize),
    /// Explicit list of 1-based column positions.
    List(Vec<usize>),
}

/// Fully parsed command-line configuration.
struct Config {
    mode: Mode,
    /// Input file paths. `-` means stdin.
    file_paths: Vec<String>,
    /// Tab stop specification.
    tab_stops: TabStops,
    /// For expand: only convert leading tabs.
    /// For unexpand: only convert leading spaces (default true).
    initial_only: bool,
}

/// Result of argument parsing.
enum ParseResult {
    Run(Config),
    Help(Mode),
    Version(Mode),
}

// ============================================================================
// Tab stop helpers
// ============================================================================

/// Parse a tab stop argument. Accepts either a single number or a
/// comma-separated list of positions. Returns `None` on parse error.
fn parse_tab_stops(s: &str) -> Option<TabStops> {
    if s.contains(',') {
        // Comma-separated list of positions.
        let mut positions: Vec<usize> = Vec::new();
        for part in s.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let n: usize = part.parse().ok()?;
            if n == 0 {
                return None;
            }
            positions.push(n);
        }
        if positions.is_empty() {
            return None;
        }
        // Positions must be strictly ascending.
        if positions.windows(2).any(|w| w.first() >= w.last()) {
            return None;
        }
        Some(TabStops::List(positions))
    } else {
        // `NonZeroUsize::parse` rejects 0 for us, which is the same
        // answer the explicit check gave and is now the type's job.
        let n: std::num::NonZeroUsize = s.trim().parse().ok()?;
        Some(TabStops::Regular(n))
    }
}

/// Given the current column (0-based) and a tab stop specification, return the
/// column of the next tab stop. Returns `None` if there are no more tab stops
/// (only possible with an explicit list).
fn next_tab_stop(col: usize, stops: &TabStops) -> Option<usize> {
    match stops {
        TabStops::Regular(interval) => {
            // Next multiple of `interval` that is strictly greater than `col`.
            Some(
                (col / *interval)
                    .saturating_add(1)
                    .saturating_mul(interval.get()),
            )
        }
        TabStops::List(positions) => {
            // Find the first position (1-based) that is strictly greater than
            // `col + 1` (converting 0-based col to 1-based).
            let col_1based = col.saturating_add(1);
            for &pos in positions {
                if pos > col_1based {
                    return Some(pos.saturating_sub(1)); // Back to 0-based.
                }
            }
            // Past the last explicit tab stop -- no more stops.
            None
        }
    }
}

// ============================================================================
// Argument parsing
// ============================================================================

/// Detect mode from argv[0].
fn detect_mode(argv0: &str) -> Mode {
    // Extract the base name, stripping any directory prefix and extension.
    let base = argv0.rsplit(['/', '\\']).next().unwrap_or(argv0);
    let base_lower = base.to_ascii_lowercase();
    if base_lower.starts_with("unexpand") {
        Mode::Unexpand
    } else {
        Mode::Expand
    }
}

fn parse_args(args: &[String]) -> ParseResult {
    let mode = if args.is_empty() {
        Mode::Expand
    } else {
        detect_mode(args.first().map_or("", String::as_str))
    };

    let mut file_paths: Vec<String> = Vec::new();
    let mut tab_stops_str: Option<String> = None;
    let mut initial_flag_set = false;
    let mut all_flag_set = false;
    let mut end_of_opts = false;

    let prog_name = if mode == Mode::Expand {
        "expand"
    } else {
        "unexpand"
    };

    let mut i = 1;
    while let Some(arg) = args.get(i) {
        if end_of_opts || !arg.starts_with('-') || arg == "-" {
            file_paths.push(arg.clone());
            i = i.saturating_add(1);
            continue;
        }

        if arg == "--" {
            end_of_opts = true;
            i = i.saturating_add(1);
            continue;
        }

        // Long options.
        if arg.starts_with("--") {
            if arg == "--help" {
                return ParseResult::Help(mode);
            } else if arg == "--version" {
                return ParseResult::Version(mode);
            } else if arg == "--initial" {
                initial_flag_set = true;
            } else if arg == "--all" && mode == Mode::Unexpand {
                all_flag_set = true;
            } else if arg == "--first-only" && mode == Mode::Unexpand {
                // Explicit leading-only (the default for unexpand).
                initial_flag_set = true;
            } else if arg == "--tabs" || arg.starts_with("--tabs=") {
                let val = if let Some(eq_val) = arg.strip_prefix("--tabs=") {
                    eq_val.to_string()
                } else {
                    i = i.saturating_add(1);
                    let Some(next) = args.get(i) else {
                        eprintln!("{prog_name}: option '--tabs' requires an argument");
                        process::exit(1);
                    };
                    next.clone()
                };
                tab_stops_str = Some(val);
            } else {
                eprintln!("{prog_name}: unrecognized option {}", quoteaf_os(arg));
                eprintln!("Try '{prog_name} --help' for more information.");
                process::exit(1);
            }

            i = i.saturating_add(1);
            continue;
        }

        // Short options.
        let short = &arg[1..];
        let mut chars = short.chars();
        while let Some(ch) = chars.next() {
            match ch {
                'i' => initial_flag_set = true,
                'a' if mode == Mode::Unexpand => all_flag_set = true,
                't' => {
                    // Everything remaining in this argument is the tab spec
                    // (allows `-t4` without a space). If nothing remains,
                    // consume the next argument.
                    let remainder: String = chars.collect();
                    let val = if remainder.is_empty() {
                        i = i.saturating_add(1);
                        let Some(next) = args.get(i) else {
                            eprintln!("{prog_name}: option '-t' requires an argument");
                            process::exit(1);
                        };
                        next.clone()
                    } else {
                        remainder
                    };
                    tab_stops_str = Some(val);
                    break;
                }
                _ => {
                    eprintln!(
                        "{prog_name}: invalid option -- {}",
                        quoteaf_os(ch.to_string())
                    );
                    eprintln!("Try '{prog_name} --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i = i.saturating_add(1);
    }

    // Default to stdin if no files given.
    if file_paths.is_empty() {
        file_paths.push("-".to_string());
    }

    let tab_stops = match tab_stops_str {
        Some(s) => match parse_tab_stops(&s) {
            Some(ts) => ts,
            None => {
                eprintln!(
                    "{prog_name}: invalid tab stop specification: {}",
                    quoteaf_os(&s)
                );
                process::exit(1);
            }
        },
        // 8 is expand's default tab width and is not zero.
        None => {
            TabStops::Regular(std::num::NonZeroUsize::new(8).unwrap_or(std::num::NonZeroUsize::MIN))
        }
    };

    // Determine initial_only:
    // - expand: default false, -i sets true
    // - unexpand: default true (leading only), -a sets false
    let initial_only = match mode {
        Mode::Expand => initial_flag_set,
        Mode::Unexpand => !all_flag_set || initial_flag_set,
    };

    ParseResult::Run(Config {
        mode,
        file_paths,
        tab_stops,
        initial_only,
    })
}

// ============================================================================
// Expand: tabs -> spaces
// ============================================================================

/// Process a single line, converting tabs to spaces.
fn expand_line(line: &str, stops: &TabStops, initial_only: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col: usize = 0;
    let mut past_leading = false;

    for ch in line.chars() {
        if ch == '\t' && !(initial_only && past_leading) {
            // Replace tab with spaces up to the next tab stop.
            match next_tab_stop(col, stops) {
                Some(next_col) => {
                    let spaces = next_col.saturating_sub(col);
                    for _ in 0..spaces {
                        out.push(' ');
                    }
                    col = next_col;
                }
                None => {
                    // Past the last explicit tab stop -- insert a single space
                    // (matches GNU expand behavior).
                    out.push(' ');
                    col = col.saturating_add(1);
                }
            }
        } else {
            if ch != ' ' && ch != '\t' {
                past_leading = true;
            }
            out.push(ch);
            if ch == '\n' {
                col = 0;
                past_leading = false;
            } else {
                col = col.saturating_add(1);
            }
        }
    }

    out
}

/// Run the expand operation on all input files.
fn run_expand(config: &Config) -> io::Result<i32> {
    let stdin_handle = io::stdin();
    let stdout_handle = io::stdout();
    let mut out = stdout_handle.lock();
    let mut exit_code = 0;

    for path in &config.file_paths {
        let reader: Box<dyn BufRead> = if path == "-" {
            Box::new(stdin_handle.lock())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(BufReader::new(f)),
                Err(e) => {
                    eprintln!("expand: {}: {e}", quotef_os(path));
                    exit_code = 1;
                    continue;
                }
            }
        };

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    let expanded = expand_line(&line, &config.tab_stops, config.initial_only);
                    out.write_all(expanded.as_bytes())?;
                    out.write_all(b"\n")?;
                }
                Err(e) => {
                    eprintln!("expand: {}: {e}", quotef_os(path));
                    exit_code = 1;
                    break;
                }
            }
        }
    }

    out.flush()?;
    Ok(exit_code)
}

// ============================================================================
// Unexpand: spaces -> tabs
// ============================================================================

/// Process a single line, converting spaces to tabs.
fn unexpand_line(line: &str, stops: &TabStops, initial_only: bool) -> String {
    let mut out = String::with_capacity(line.len());
    let mut col: usize = 0;
    let mut space_run_start: Option<usize> = None;
    let mut past_leading = false;

    for ch in line.chars() {
        if ch == ' ' && !(initial_only && past_leading) {
            if space_run_start.is_none() {
                space_run_start = Some(col);
            }
            col = col.saturating_add(1);

            // Check if we have reached a tab stop. If so, emit a tab for the
            // accumulated spaces.
            if let Some(next) = next_tab_stop(col.saturating_sub(1), stops)
                && col == next
            {
                // We are exactly at a tab stop -- emit a tab for the
                // spaces accumulated since space_run_start.
                out.push('\t');
                space_run_start = None;
            }
        } else {
            // Flush any remaining spaces that did not reach a tab stop.
            if let Some(start) = space_run_start {
                for _ in start..col {
                    out.push(' ');
                }
                space_run_start = None;
            }

            if ch != ' ' {
                past_leading = true;
            }

            out.push(ch);

            if ch == '\t' {
                // A literal tab -- advance column to next tab stop.
                if let Some(next) = next_tab_stop(col, stops) {
                    col = next;
                } else {
                    col = col.saturating_add(1);
                }
            } else if ch == '\n' {
                col = 0;
                past_leading = false;
            } else {
                col = col.saturating_add(1);
            }
        }
    }

    // Flush trailing spaces.
    if let Some(start) = space_run_start {
        for _ in start..col {
            out.push(' ');
        }
    }

    out
}

/// Run the unexpand operation on all input files.
fn run_unexpand(config: &Config) -> io::Result<i32> {
    let stdin_handle = io::stdin();
    let stdout_handle = io::stdout();
    let mut out = stdout_handle.lock();
    let mut exit_code = 0;

    for path in &config.file_paths {
        let reader: Box<dyn BufRead> = if path == "-" {
            Box::new(stdin_handle.lock())
        } else {
            match File::open(path) {
                Ok(f) => Box::new(BufReader::new(f)),
                Err(e) => {
                    eprintln!("unexpand: {}: {e}", quotef_os(path));
                    exit_code = 1;
                    continue;
                }
            }
        };

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    let unexpanded = unexpand_line(&line, &config.tab_stops, config.initial_only);
                    out.write_all(unexpanded.as_bytes())?;
                    out.write_all(b"\n")?;
                }
                Err(e) => {
                    eprintln!("unexpand: {}: {e}", quotef_os(path));
                    exit_code = 1;
                    break;
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

fn print_expand_help() {
    println!("Slate OS expand v{VERSION}");
    println!();
    println!("Convert tabs in each FILE to spaces, writing to standard output.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  expand [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -i, --initial        Only convert leading tabs");
    println!("  -t, --tabs=N         Set tab stops every N columns (default 8)");
    println!("  -t, --tabs=N1,N2,..  Comma-separated list of tab stop column positions");
    println!("      --help           Display this help and exit");
    println!("      --version        Output version information and exit");
    println!();
    println!("EXAMPLES:");
    println!("  expand file.txt              Replace tabs with spaces (8-column stops)");
    println!("  expand -t4 file.txt          Use 4-column tab stops");
    println!("  expand -t 1,5,9 file.txt     Explicit tab stops at columns 1, 5, 9");
}

fn print_unexpand_help() {
    println!("Slate OS unexpand v{VERSION}");
    println!();
    println!("Convert spaces in each FILE to tabs, writing to standard output.");
    println!("With no FILE, or when FILE is -, read standard input.");
    println!();
    println!("USAGE:");
    println!("  unexpand [OPTION]... [FILE]...");
    println!();
    println!("OPTIONS:");
    println!("  -a, --all            Convert all sequences of spaces, not just leading");
    println!("  -t, --tabs=N         Set tab stops every N columns (default 8)");
    println!("      --first-only     Only convert leading spaces (default)");
    println!("      --help           Display this help and exit");
    println!("      --version        Output version information and exit");
    println!();
    println!("EXAMPLES:");
    println!("  unexpand file.txt            Replace leading spaces with tabs");
    println!("  unexpand -a file.txt         Replace all space runs with tabs");
    println!("  unexpand -t4 file.txt        Use 4-column tab stops");
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    match parse_args(&args) {
        ParseResult::Help(mode) => {
            match mode {
                Mode::Expand => print_expand_help(),
                Mode::Unexpand => print_unexpand_help(),
            }
            process::exit(0);
        }
        ParseResult::Version(mode) => {
            let name = match mode {
                Mode::Expand => "expand",
                Mode::Unexpand => "unexpand",
            };
            println!("{name} (Slate OS) {VERSION}");
            process::exit(0);
        }
        ParseResult::Run(config) => {
            let result = match config.mode {
                Mode::Expand => run_expand(&config),
                Mode::Unexpand => run_unexpand(&config),
            };

            match result {
                Ok(code) => process::exit(code),
                Err(e) => {
                    let name = match config.mode {
                        Mode::Expand => "expand",
                        Mode::Unexpand => "unexpand",
                    };
                    eprintln!("{name}: {e}");
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
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::*;

    fn regular(n: usize) -> TabStops {
        TabStops::Regular(std::num::NonZeroUsize::new(n).unwrap())
    }

    // -- Tab stop parsing --

    #[test]
    fn a_zero_interval_is_rejected_by_the_type() {
        // `TabStops::Regular` holds a `NonZeroUsize`, so `col / interval`
        // cannot divide by zero however the value arrived. The parse used to
        // reject 0 with an explicit check and the division four functions
        // away had to trust it; now the type carries the invariant.
        assert!(parse_tab_stops("0").is_none());
        assert!(parse_tab_stops("8").is_some());
    }

    #[test]
    fn a_tab_stop_too_large_to_represent_is_rejected() {
        // GNU says "tab stop is too large"; the answer that matters is that
        // it is refused rather than wrapped into something small.
        assert!(parse_tab_stops("99999999999999999999").is_none());
        assert!(parse_tab_stops("-1").is_none());
        assert!(parse_tab_stops("").is_none());
    }

    #[test]
    fn an_explicit_list_must_ascend_strictly() {
        assert!(parse_tab_stops("1,3,5").is_some());
        // Equal is not ascending: two stops at one column would make
        // `next_tab_stop` return a column it had already passed.
        assert!(parse_tab_stops("1,3,3").is_none());
        assert!(parse_tab_stops("5,3").is_none());
    }

    // -- Where the next stop is --

    #[test]
    fn a_regular_interval_lands_on_the_next_multiple() {
        let t = regular(8);
        assert_eq!(next_tab_stop(0, &t), Some(8));
        assert_eq!(next_tab_stop(7, &t), Some(8));
        // Strictly greater: a column already on a stop moves to the NEXT one,
        // or a tab at column 8 would expand to nothing.
        assert_eq!(next_tab_stop(8, &t), Some(16));
    }

    #[test]
    fn a_huge_interval_saturates_instead_of_wrapping() {
        // `-t` is bounded only by `usize`, and the product used to be a plain
        // multiply. Saturating keeps the answer monotonic, which is what the
        // caller's `next_col - col` depends on.
        let t = regular(usize::MAX);
        let stop = next_tab_stop(1, &t).unwrap();
        assert!(stop > 1, "a stop must be past the column it starts from");
    }

    #[test]
    fn past_the_last_explicit_stop_there_is_none() {
        let t = TabStops::List(vec![3, 5]);
        assert_eq!(next_tab_stop(0, &t), Some(2));
        assert_eq!(next_tab_stop(2, &t), Some(4));
        // Nothing beyond the list; the caller emits a single space.
        assert_eq!(next_tab_stop(9, &t), None);
    }

    // -- Expansion --

    #[test]
    fn tabs_become_spaces_to_the_next_stop() {
        let t = regular(8);
        assert_eq!(expand_line("a	b", &t, false), "a       b");
        assert_eq!(expand_line("	a", &t, false), "        a");
        assert_eq!(expand_line("no tabs", &t, false), "no tabs");
    }

    #[test]
    fn a_newline_resets_the_column() {
        // Without the reset, the second line's tab would expand against the
        // first line's width.
        let t = regular(8);
        assert_eq!(
            expand_line(
                "ab
cd	e", &t, false
            ),
            "ab
cd      e"
        );
    }

    #[test]
    fn initial_only_stops_at_the_first_non_blank() {
        let t = regular(8);
        assert_eq!(expand_line("	a	b", &t, true), "        a	b");
    }

    // -- Argument parsing --

    #[test]
    fn an_empty_argv_does_not_panic() {
        // `execve` takes the argument vector from its caller and nothing
        // requires a program name in it. This crate already guarded argv[0],
        // unlike `uname`, and the test pins that it stays guarded.
        match parse_args(&[]) {
            ParseResult::Run(_) | ParseResult::Help(_) | ParseResult::Version(_) => {}
        }
    }
}
