//! find — search for files in a directory hierarchy.
//!
//! Usage: find [PATH...] [EXPRESSION]
//!   -name PATTERN    match filename (shell glob: *, ?, [])
//!   -type TYPE       match type: f (file), d (dir), l (symlink)
//!   -maxdepth N      descend at most N levels
//!   -print           print matching paths (default action)

use std::env;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let mut paths: Vec<String> = Vec::new();
    let mut name_pattern: Option<String> = None;
    let mut type_filter: Option<char> = None;
    let mut max_depth: Option<usize> = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-name" => {
                i += 1;
                if i < args.len() {
                    name_pattern = Some(args[i].clone());
                }
            }
            "-type" => {
                i += 1;
                if i < args.len() {
                    type_filter = args[i].chars().next();
                }
            }
            "-maxdepth" => {
                i += 1;
                if i < args.len() {
                    max_depth = args[i].parse().ok();
                }
            }
            "-print" => {} // default action, ignore
            arg if !arg.starts_with('-') => {
                paths.push(arg.to_string());
            }
            other => {
                eprintln!("find: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    if paths.is_empty() {
        paths.push(".".to_string());
    }

    for path in &paths {
        let p = Path::new(path);
        find_recursive(p, 0, max_depth, name_pattern.as_deref(), type_filter);
    }
}

fn find_recursive(
    dir: &Path,
    depth: usize,
    max_depth: Option<usize>,
    name_pattern: Option<&str>,
    type_filter: Option<char>,
) {
    // Check the directory itself at depth 0
    if depth == 0
        && matches_filters(dir, name_pattern, type_filter) {
            println!("{}", dir.display());
        }

    if let Some(max) = max_depth
        && depth >= max {
            return;
        }

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("find: '{}': {e}", dir.display());
            return;
        }
    };

    for entry_result in entries {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                eprintln!("find: {e}");
                continue;
            }
        };

        let path = entry.path();

        if matches_filters(&path, name_pattern, type_filter) {
            println!("{}", path.display());
        }

        // Recurse into directories
        if path.is_dir() {
            find_recursive(&path, depth + 1, max_depth, name_pattern, type_filter);
        }
    }
}

fn matches_filters(path: &Path, name_pattern: Option<&str>, type_filter: Option<char>) -> bool {
    // Type filter
    if let Some(t) = type_filter {
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(_) => return false,
        };
        let matches_type = match t {
            'f' => meta.is_file(),
            'd' => meta.is_dir(),
            'l' => meta.file_type().is_symlink(),
            _ => true,
        };
        if !matches_type {
            return false;
        }
    }

    // Name filter (glob-like)
    if let Some(pattern) = name_pattern {
        let filename = match path.file_name() {
            Some(n) => n.to_string_lossy(),
            None => return false,
        };
        if !glob_match(pattern, &filename) {
            return false;
        }
    }

    true
}

/// Simple glob matching: * matches any sequence, ? matches one char,
/// [abc] matches one of the listed chars.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_inner(&pat, &txt, 0, 0)
}

fn glob_match_inner(pat: &[char], txt: &[char], pi: usize, ti: usize) -> bool {
    if pi == pat.len() {
        return ti == txt.len();
    }

    match pat[pi] {
        '*' => {
            // Try matching zero or more characters
            for skip in 0..=(txt.len() - ti) {
                if glob_match_inner(pat, txt, pi + 1, ti + skip) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < txt.len() {
                glob_match_inner(pat, txt, pi + 1, ti + 1)
            } else {
                false
            }
        }
        '[' => {
            if ti >= txt.len() {
                return false;
            }
            // Find closing ]
            let mut end = pi + 1;
            while end < pat.len() && pat[end] != ']' {
                end += 1;
            }
            if end >= pat.len() {
                return false; // malformed
            }
            let chars = &pat[pi + 1..end];
            let matches = chars.contains(&txt[ti]);
            if matches {
                glob_match_inner(pat, txt, end + 1, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < txt.len() && txt[ti] == c {
                glob_match_inner(pat, txt, pi + 1, ti + 1)
            } else {
                false
            }
        }
    }
}
