//! OurOS `expand` / `unexpand` Utility -- Tab/Space Conversion
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
    Regular(usize),
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
        for i in 1..positions.len() {
            if positions[i] <= positions[i - 1] {
                return None;
            }
        }
        Some(TabStops::List(positions))
    } else {
        let n: usize = s.trim().parse().ok()?;
        if n == 0 {
            return None;
        }
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
            Some((col / interval + 1) * interval)
        }
        TabStops::List(positions) => {
            // Find the first position (1-based) that is strictly greater than
            // `col + 1` (converting 0-based col to 1-based).
            let col_1based = col + 1;
            for &pos in positions {
                if pos > col_1based {
                    return Some(pos - 1); // Convert back to 0-based.
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
    let base = argv0
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(argv0);
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
        detect_mode(&args[0])
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
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{prog_name}: option '--tabs' requires an argument");
                        process::exit(1);
                    }
                    args[i].clone()
                };
                tab_stops_str = Some(val);
            } else {
                eprintln!("{prog_name}: unrecognized option '{arg}'");
                eprintln!("Try '{prog_name} --help' for more information.");
                process::exit(1);
            }

            i += 1;
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
                        i += 1;
                        if i >= args.len() {
                            eprintln!("{prog_name}: option '-t' requires an argument");
                            process::exit(1);
                        }
                        args[i].clone()
                    } else {
                        remainder
                    };
                    tab_stops_str = Some(val);
                    break;
                }
                _ => {
                    eprintln!("{prog_name}: invalid option -- '{ch}'");
                    eprintln!("Try '{prog_name} --help' for more information.");
                    process::exit(1);
                }
            }
        }

        i += 1;
    }

    // Default to stdin if no files given.
    if file_paths.is_empty() {
        file_paths.push("-".to_string());
    }

    let tab_stops = match tab_stops_str {
        Some(s) => match parse_tab_stops(&s) {
            Some(ts) => ts,
            None => {
                eprintln!("{prog_name}: invalid tab stop specification: '{s}'");
                process::exit(1);
            }
        },
        None => TabStops::Regular(8),
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
                    let spaces = next_col - col;
                    for _ in 0..spaces {
                        out.push(' ');
                    }
                    col = next_col;
                }
                None => {
                    // Past the last explicit tab stop -- insert a single space
                    // (matches GNU expand behavior).
                    out.push(' ');
                    col += 1;
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
                col += 1;
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
                    eprintln!("expand: {path}: {e}");
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
                    eprintln!("expand: {path}: {e}");
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
            col += 1;

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
                    col += 1;
                }
            } else if ch == '\n' {
                col = 0;
                past_leading = false;
            } else {
                col += 1;
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
                    eprintln!("unexpand: {path}: {e}");
                    exit_code = 1;
                    continue;
                }
            }
        };

        for line_result in reader.lines() {
            match line_result {
                Ok(line) => {
                    let unexpanded =
                        unexpand_line(&line, &config.tab_stops, config.initial_only);
                    out.write_all(unexpanded.as_bytes())?;
                    out.write_all(b"\n")?;
                }
                Err(e) => {
                    eprintln!("unexpand: {path}: {e}");
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
    println!("OurOS expand v{VERSION}");
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
    println!("OurOS unexpand v{VERSION}");
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
            println!("{name} (OurOS) {VERSION}");
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
