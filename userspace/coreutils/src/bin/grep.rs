//! grep -- search for a pattern in files.
//!
//! Usage: grep [-i] [-v] [-c] [-n] [-r] PATTERN [FILE...]
//!   -i  case-insensitive matching
//!   -v  invert match (select non-matching lines)
//!   -c  print only a count of matching lines per file
//!   -n  prefix each line with its line number
//!   -r  recursively search directories
//!   If no FILE, read from standard input.
//!
//! Uses substring matching (no regex). PATTERN is a literal string.

use std::env;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    ignore_case: bool,
    invert: bool,
    count_only: bool,
    line_numbers: bool,
    recursive: bool,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct GrepArgs {
    opts: Options,
    pattern: String,
    files: Vec<String>,
}

/// Parse grep's argv.  Clusters of single-letter flags are supported.
/// Returns an error for unknown flags or a missing PATTERN.  Bare "-"
/// is treated as a filename (stdin), not a flag cluster.
fn parse_args(args: &[String]) -> Result<GrepArgs, String> {
    let mut opts = Options::default();
    let mut positional: Vec<String> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 && arg != "-" {
            let rest = arg.get(1..).unwrap_or("");
            for c in rest.chars() {
                match c {
                    'i' => opts.ignore_case = true,
                    'v' => opts.invert = true,
                    'c' => opts.count_only = true,
                    'n' => opts.line_numbers = true,
                    'r' => opts.recursive = true,
                    other => return Err(format!("unknown option: -{other}")),
                }
            }
        } else {
            positional.push(arg.clone());
        }
    }

    if positional.is_empty() {
        return Err("missing PATTERN".to_string());
    }

    let pattern = positional.first().cloned().unwrap_or_default();
    let mut files: Vec<String> = positional.get(1..).unwrap_or(&[]).to_vec();
    if files.is_empty() {
        files.push("-".to_string());
    }

    Ok(GrepArgs { opts, pattern, files })
}

/// Decide whether `line` matches the pattern under the given options.
/// Returns the "selected" flag (already accounting for `-v`).
fn line_selected(line: &str, pattern_cmp: &str, opts: &Options) -> bool {
    let hay = if opts.ignore_case {
        line.to_lowercase()
    } else {
        line.to_string()
    };
    let matched = hay.contains(pattern_cmp);
    if opts.invert { !matched } else { matched }
}

/// Build the prefix shown before each matching line.  Returns the empty
/// string when neither filename nor line-number is requested.
fn line_prefix(filename: &str, line_idx_zero_based: usize, show_filename: bool, line_numbers: bool) -> String {
    let mut prefix = String::new();
    if show_filename {
        prefix.push_str(filename);
        prefix.push(':');
    }
    if line_numbers {
        // Convert from zero-based to one-based for display.
        prefix.push_str(&(line_idx_zero_based.saturating_add(1)).to_string());
        prefix.push(':');
    }
    prefix
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("grep: {e}");
            process::exit(2);
        }
    };

    let mut files = parsed.files;

    // Expand directories when -r is set.
    if parsed.opts.recursive {
        let mut expanded: Vec<String> = Vec::new();
        for f in &files {
            let path = Path::new(f);
            if path.is_dir() {
                collect_files_recursive(path, &mut expanded);
            } else {
                expanded.push(f.clone());
            }
        }
        files = expanded;
    }

    let show_filename = files.len() > 1;
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut any_match = false;

    for path in &files {
        let reader: Box<dyn Read> = if path == "-" {
            Box::new(io::stdin())
        } else {
            if Path::new(path).is_dir() {
                if !parsed.opts.recursive {
                    eprintln!("grep: {path}: Is a directory");
                }
                continue;
            }
            match File::open(path) {
                Ok(f) => Box::new(f),
                Err(e) => {
                    eprintln!("grep: {path}: {e}");
                    continue;
                }
            }
        };

        let matched = search_stream(&mut out, reader, &parsed.pattern, path, show_filename, &parsed.opts);
        if matched {
            any_match = true;
        }
    }

    if !any_match {
        process::exit(1);
    }
}

fn search_stream(
    out: &mut impl Write,
    reader: impl Read,
    pattern: &str,
    filename: &str,
    show_filename: bool,
    opts: &Options,
) -> bool {
    let buf = BufReader::new(reader);
    let pattern_cmp = if opts.ignore_case {
        pattern.to_lowercase()
    } else {
        pattern.to_string()
    };

    let mut match_count: usize = 0;

    for (line_idx, line) in buf.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("grep: {filename}: {e}");
                break;
            }
        };

        if line_selected(&line, &pattern_cmp, opts) {
            match_count = match_count.saturating_add(1);
            if !opts.count_only {
                let prefix = line_prefix(filename, line_idx, show_filename, opts.line_numbers);
                let _ = writeln!(out, "{prefix}{line}");
            }
        }
    }

    if opts.count_only {
        if show_filename {
            let _ = writeln!(out, "{filename}:{match_count}");
        } else {
            let _ = writeln!(out, "{match_count}");
        }
    }

    match_count > 0
}

fn collect_files_recursive(dir: &Path, result: &mut Vec<String>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("grep: {}: {e}", dir.display());
            return;
        }
    };

    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    paths.sort();

    for path in paths {
        if path.is_dir() {
            collect_files_recursive(&path, result);
        } else {
            result.push(path.to_string_lossy().into_owned());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing PATTERN"));
    }

    #[test]
    fn parse_pattern_only_reads_stdin() {
        let a = parse_args(&s(&["foo"])).unwrap();
        assert_eq!(a.pattern, "foo");
        assert_eq!(a.files, vec!["-"]);
        assert_eq!(a.opts, Options::default());
    }

    #[test]
    fn parse_pattern_and_files() {
        let a = parse_args(&s(&["foo", "a.txt", "b.txt"])).unwrap();
        assert_eq!(a.pattern, "foo");
        assert_eq!(a.files, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn parse_dash_i_ignore_case() {
        let a = parse_args(&s(&["-i", "FOO"])).unwrap();
        assert!(a.opts.ignore_case);
        assert_eq!(a.pattern, "FOO");
    }

    #[test]
    fn parse_clustered_flags() {
        let a = parse_args(&s(&["-ivcnr", "foo"])).unwrap();
        assert!(a.opts.ignore_case);
        assert!(a.opts.invert);
        assert!(a.opts.count_only);
        assert!(a.opts.line_numbers);
        assert!(a.opts.recursive);
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse_args(&s(&["-Z", "foo"])).unwrap_err();
        assert!(err.contains("unknown option"));
        assert!(err.contains('Z'));
    }

    #[test]
    fn parse_bare_dash_is_a_filename() {
        let a = parse_args(&s(&["foo", "-"])).unwrap();
        assert_eq!(a.files, vec!["-"]);
    }

    #[test]
    fn parse_flag_after_pattern_still_a_flag() {
        // Our parser is order-insensitive: flags can appear anywhere.
        let a = parse_args(&s(&["foo", "-v", "x.txt"])).unwrap();
        assert!(a.opts.invert);
        assert_eq!(a.pattern, "foo");
        assert_eq!(a.files, vec!["x.txt"]);
    }

    // ---------------- line_selected ----------------

    #[test]
    fn select_substring_match() {
        let opts = Options::default();
        assert!(line_selected("hello world", "world", &opts));
        assert!(line_selected("hello world", "hello", &opts));
        assert!(!line_selected("hello world", "WORLD", &opts));
    }

    #[test]
    fn select_case_insensitive() {
        let opts = Options { ignore_case: true, ..Options::default() };
        // Caller must lowercase the pattern when ignore_case is set.
        assert!(line_selected("Hello World", "world", &opts));
        assert!(line_selected("HELLO", "hello", &opts));
    }

    #[test]
    fn select_invert() {
        let opts = Options { invert: true, ..Options::default() };
        assert!(!line_selected("hello", "ll", &opts));
        assert!(line_selected("hello", "zz", &opts));
    }

    #[test]
    fn select_invert_with_ignore_case() {
        let opts = Options { invert: true, ignore_case: true, ..Options::default() };
        assert!(!line_selected("HELLO", "hello", &opts));
        assert!(line_selected("HELLO", "zz", &opts));
    }

    #[test]
    fn select_empty_pattern_matches_everything() {
        let opts = Options::default();
        assert!(line_selected("anything", "", &opts));
        assert!(line_selected("", "", &opts));
    }

    #[test]
    fn select_empty_line_no_match_for_non_empty_pat() {
        let opts = Options::default();
        assert!(!line_selected("", "foo", &opts));
    }

    // ---------------- line_prefix ----------------

    #[test]
    fn prefix_none() {
        assert_eq!(line_prefix("f", 0, false, false), "");
    }

    #[test]
    fn prefix_filename_only() {
        assert_eq!(line_prefix("a.txt", 0, true, false), "a.txt:");
    }

    #[test]
    fn prefix_line_number_only() {
        // line index 0 → "1:" (one-based).
        assert_eq!(line_prefix("ignored", 0, false, true), "1:");
        assert_eq!(line_prefix("ignored", 41, false, true), "42:");
    }

    #[test]
    fn prefix_filename_and_line_number() {
        assert_eq!(line_prefix("a.txt", 9, true, true), "a.txt:10:");
    }

    // ---------------- search_stream ----------------

    fn run_search(input: &[u8], pattern: &str, opts: Options, filename: &str, show_filename: bool) -> (String, bool) {
        let mut out: Vec<u8> = Vec::new();
        let matched = search_stream(&mut out, input, pattern, filename, show_filename, &opts);
        (String::from_utf8(out).unwrap(), matched)
    }

    #[test]
    fn search_basic_match() {
        let (out, matched) = run_search(b"foo\nbar\nfoobar\n", "foo", Options::default(), "f", false);
        assert!(matched);
        assert_eq!(out, "foo\nfoobar\n");
    }

    #[test]
    fn search_no_match_returns_false() {
        let (out, matched) = run_search(b"abc\ndef\n", "xyz", Options::default(), "f", false);
        assert!(!matched);
        assert_eq!(out, "");
    }

    #[test]
    fn search_count_only() {
        let opts = Options { count_only: true, ..Options::default() };
        let (out, matched) = run_search(b"a\nab\nabc\n", "a", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "3\n");
    }

    #[test]
    fn search_count_only_with_filename() {
        let opts = Options { count_only: true, ..Options::default() };
        let (out, _) = run_search(b"a\nab\nabc\n", "a", opts, "x.txt", true);
        assert_eq!(out, "x.txt:3\n");
    }

    #[test]
    fn search_line_numbers() {
        let opts = Options { line_numbers: true, ..Options::default() };
        let (out, _) = run_search(b"x\nfoo\nbar\nfoo\n", "foo", opts, "f", false);
        assert_eq!(out, "2:foo\n4:foo\n");
    }

    #[test]
    fn search_invert() {
        let opts = Options { invert: true, ..Options::default() };
        let (out, matched) = run_search(b"a\nb\nc\n", "b", opts, "f", false);
        assert!(matched);
        assert_eq!(out, "a\nc\n");
    }

    #[test]
    fn search_ignore_case() {
        let opts = Options { ignore_case: true, ..Options::default() };
        let (out, _) = run_search(b"FOO\nbar\nFooBar\n", "foo", opts, "f", false);
        assert_eq!(out, "FOO\nFooBar\n");
    }

    #[test]
    fn search_show_filename_prefix() {
        let (out, _) = run_search(b"foo\n", "foo", Options::default(), "x.txt", true);
        assert_eq!(out, "x.txt:foo\n");
    }
}
