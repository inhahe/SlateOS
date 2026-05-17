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

struct Options {
    ignore_case: bool,
    invert: bool,
    count_only: bool,
    line_numbers: bool,
    recursive: bool,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut opts = Options {
        ignore_case: false,
        invert: false,
        count_only: false,
        line_numbers: false,
        recursive: false,
    };
    let mut positional: Vec<String> = Vec::new();

    for arg in &args {
        if arg.starts_with('-') && arg.len() > 1 && arg != "-" {
            for c in arg[1..].chars() {
                match c {
                    'i' => opts.ignore_case = true,
                    'v' => opts.invert = true,
                    'c' => opts.count_only = true,
                    'n' => opts.line_numbers = true,
                    'r' => opts.recursive = true,
                    _ => {
                        eprintln!("grep: unknown option: -{c}");
                        process::exit(2);
                    }
                }
            }
        } else {
            positional.push(arg.clone());
        }
    }

    if positional.is_empty() {
        eprintln!("grep: missing PATTERN");
        process::exit(2);
    }

    let pattern = positional[0].clone();
    let mut files: Vec<String> = positional[1..].to_vec();

    if files.is_empty() {
        files.push("-".to_string());
    }

    // Expand directories when -r is set.
    if opts.recursive {
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
                if !opts.recursive {
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

        let matched = search_stream(&mut out, reader, &pattern, path, show_filename, &opts);
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

        let hay = if opts.ignore_case {
            line.to_lowercase()
        } else {
            line.clone()
        };

        let matched = hay.contains(&pattern_cmp);
        let selected = if opts.invert { !matched } else { matched };

        if selected {
            match_count += 1;
            if !opts.count_only {
                let prefix = if show_filename {
                    format!("{filename}:")
                } else {
                    String::new()
                };
                if opts.line_numbers {
                    let _ = writeln!(out, "{prefix}{}:{line}", line_idx + 1);
                } else {
                    let _ = writeln!(out, "{prefix}{line}");
                }
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
