//! ls — list directory contents.
//!
//! Usage: ls [-l] [-a] [-h] [-1] [PATH...]
//!   -l  long listing format (permissions, size, date, name)
//!   -a  show hidden files (starting with .)
//!   -h  human-readable sizes (K, M, G) in long format
//!   -1  one entry per line (default when output is not a terminal)

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    long: bool,
    all: bool,
    human: bool,
    one_per_line: bool,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct LsArgs {
    opts: Options,
    paths: Vec<String>,
    /// Short flag chars that weren't recognised.  Reported as warnings
    /// by `main()`; the original behaviour was to continue regardless.
    unknown: Vec<char>,
}

/// Parse ls's argv.  Short flags can be clustered (`-la`), long options
/// (`--…`) are passed through to the positional list (the existing code
/// did the same — there's no actual long-option support).  Unknown
/// short flags are collected so the caller can warn but proceed.
fn parse_args(args: &[String]) -> LsArgs {
    let mut opts = Options::default();
    let mut paths: Vec<String> = Vec::new();
    let mut unknown: Vec<char> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            let rest = arg.get(1..).unwrap_or("");
            for c in rest.chars() {
                match c {
                    'l' => opts.long = true,
                    'a' => opts.all = true,
                    'h' => opts.human = true,
                    '1' => opts.one_per_line = true,
                    other => unknown.push(other),
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    LsArgs { opts, paths, unknown }
}

/// True if `name` should be hidden under the current `all` flag.  Names
/// starting with '.' are hidden unless `-a` is set.
fn is_hidden(name: &str, all: bool) -> bool {
    !all && name.starts_with('.')
}

/// Case-insensitive sort key used to order entries within a directory.
fn sort_key(name: &str) -> String {
    name.to_lowercase()
}

/// Join entry names into the simple (non-long, non-one-per-line) output
/// row: names separated by two spaces, with a trailing newline iff at
/// least one entry was emitted.
fn join_simple_row(names: &[String]) -> String {
    if names.is_empty() {
        return String::new();
    }
    let mut out = names.join("  ");
    out.push('\n');
    out
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut parsed = parse_args(&args);

    for c in &parsed.unknown {
        eprintln!("ls: unknown option: -{c}");
    }

    if parsed.paths.is_empty() {
        parsed.paths.push(".".to_string());
    }

    let show_dir_name = parsed.paths.len() > 1;
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for (i, path) in parsed.paths.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(out);
        }
        if show_dir_name {
            let _ = writeln!(out, "{path}:");
        }
        list_dir(&mut out, path, &parsed.opts);
    }
}

fn list_dir(out: &mut impl Write, path: &str, opts: &Options) {
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            // Maybe it's a file, not a directory
            if Path::new(path).is_file() {
                show_entry(out, path, Path::new(path), opts);
                return;
            }
            eprintln!("ls: cannot access '{path}': {e}");
            return;
        }
    };

    let mut names: Vec<(String, std::path::PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden(&name, opts.all) {
            continue;
        }
        names.push((name, entry.path()));
    }
    names.sort_by_key(|a| sort_key(&a.0));

    if opts.long {
        for (name, path) in &names {
            show_entry_long(out, name, path, opts);
        }
    } else if opts.one_per_line {
        for (name, _) in &names {
            let _ = writeln!(out, "{name}");
        }
    } else {
        let just_names: Vec<String> = names.iter().map(|(n, _)| n.clone()).collect();
        let _ = write!(out, "{}", join_simple_row(&just_names));
    }
}

fn show_entry(out: &mut impl Write, name: &str, path: &Path, opts: &Options) {
    if opts.long {
        show_entry_long(out, name, path, opts);
    } else {
        let _ = writeln!(out, "{name}");
    }
}

fn show_entry_long(out: &mut impl Write, name: &str, path: &Path, opts: &Options) {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            let _ = writeln!(out, "?????????? ? ? {name}");
            return;
        }
    };

    let file_type = if meta.is_dir() {
        "d"
    } else if meta.is_symlink() {
        "l"
    } else {
        "-"
    };
    let size = meta.len();
    let size_str = if opts.human {
        human_size(size)
    } else {
        format!("{size:>8}")
    };

    let _ = writeln!(out, "{file_type}rw-r--r--  {size_str} {name}");
}

fn human_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;

    if bytes >= GIB {
        format!("{:>5.1}G", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:>5.1}M", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:>5.1}K", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes:>6}")
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
    fn parse_empty() {
        let a = parse_args(&s(&[]));
        assert_eq!(a.opts, Options::default());
        assert!(a.paths.is_empty());
        assert!(a.unknown.is_empty());
    }

    #[test]
    fn parse_one_path() {
        let a = parse_args(&s(&["/etc"]));
        assert_eq!(a.paths, vec!["/etc"]);
    }

    #[test]
    fn parse_short_flags() {
        let a = parse_args(&s(&["-l"]));
        assert!(a.opts.long);
        assert!(!a.opts.all);
    }

    #[test]
    fn parse_clustered() {
        let a = parse_args(&s(&["-lah1"]));
        assert!(a.opts.long);
        assert!(a.opts.all);
        assert!(a.opts.human);
        assert!(a.opts.one_per_line);
    }

    #[test]
    fn parse_unknown_flag_recorded() {
        let a = parse_args(&s(&["-Z"]));
        assert_eq!(a.unknown, vec!['Z']);
    }

    #[test]
    fn parse_double_dash_treated_as_path() {
        // Double-dash args are passed through unchanged (no long-opt
        // support).
        let a = parse_args(&s(&["--color", "/tmp"]));
        assert_eq!(a.paths, vec!["--color", "/tmp"]);
    }

    #[test]
    fn parse_flags_and_paths_mixed() {
        let a = parse_args(&s(&["-l", "a", "-a", "b"]));
        assert!(a.opts.long);
        assert!(a.opts.all);
        assert_eq!(a.paths, vec!["a", "b"]);
    }

    // ---------------- is_hidden ----------------

    #[test]
    fn hidden_dotfile_without_all() {
        assert!(is_hidden(".bashrc", false));
        assert!(is_hidden(".", false));
        assert!(is_hidden("..", false));
    }

    #[test]
    fn hidden_dotfile_with_all_visible() {
        assert!(!is_hidden(".bashrc", true));
    }

    #[test]
    fn hidden_normal_file_always_visible() {
        assert!(!is_hidden("README.md", false));
        assert!(!is_hidden("README.md", true));
    }

    // ---------------- sort_key ----------------

    #[test]
    fn sort_key_is_lowercase() {
        assert_eq!(sort_key("Hello"), "hello");
        assert_eq!(sort_key("WORLD"), "world");
        assert_eq!(sort_key("Mixed-Case"), "mixed-case");
    }

    #[test]
    fn sort_key_preserves_ordering() {
        let mut names = vec!["banana".to_string(), "Apple".to_string(), "cherry".to_string()];
        names.sort_by_key(|n| sort_key(n));
        assert_eq!(names, vec!["Apple", "banana", "cherry"]);
    }

    // ---------------- join_simple_row ----------------

    #[test]
    fn join_empty() {
        assert_eq!(join_simple_row(&[]), "");
    }

    #[test]
    fn join_single() {
        assert_eq!(join_simple_row(&["only".to_string()]), "only\n");
    }

    #[test]
    fn join_multiple() {
        let names: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        assert_eq!(join_simple_row(&names), "a  b  c\n");
    }

    // ---------------- human_size ----------------

    #[test]
    fn human_under_kib() {
        assert_eq!(human_size(0), "     0");
        assert_eq!(human_size(1), "     1");
        assert_eq!(human_size(1023), "  1023");
    }

    #[test]
    fn human_kib() {
        assert_eq!(human_size(1024), "  1.0K");
        assert_eq!(human_size(1536), "  1.5K");
        assert_eq!(human_size(10 * 1024), " 10.0K");
    }

    #[test]
    fn human_mib() {
        assert_eq!(human_size(1024 * 1024), "  1.0M");
        assert_eq!(human_size(5 * 1024 * 1024 + 512 * 1024), "  5.5M");
    }

    #[test]
    fn human_gib() {
        assert_eq!(human_size(1024 * 1024 * 1024), "  1.0G");
        assert_eq!(human_size(2_500_000_000), "  2.3G");
    }

    #[test]
    fn human_boundary_just_under_kib() {
        assert_eq!(human_size(1023), "  1023");
    }

    #[test]
    fn human_boundary_just_under_mib() {
        // 1024 * 1024 - 1 → still K range.
        let s = human_size(1024 * 1024 - 1);
        assert!(s.ends_with('K'));
    }
}
