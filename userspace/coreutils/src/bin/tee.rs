//! tee — read from stdin, write to stdout and files.
//!
//! Usage: tee [-a] [FILE...]
//!   -a  append to files instead of overwriting

use std::env;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (append, paths) = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("tee: {e}");
            process::exit(1);
        }
    };

    let mut files: Vec<File> = Vec::new();
    for path in &paths {
        let file = if append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match file {
            Ok(f) => files.push(f),
            Err(e) => {
                eprintln!("tee: {path}: {e}");
            }
        }
    }

    let mut buf = [0u8; 8192];
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    loop {
        match stdin.lock().read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                let _ = out.write_all(&buf[..n]);
                for f in &mut files {
                    let _ = f.write_all(&buf[..n]);
                }
            }
            Err(e) => {
                eprintln!("tee: read error: {e}");
                process::exit(1);
            }
        }
    }
}

/// Parse tee's argv into `(append, paths)`. Returns an error on unknown
/// flags. `-` alone (and any non-flag arg) is treated as a file path.
fn parse_args(args: &[String]) -> Result<(bool, Vec<String>), String> {
    let mut append = false;
    let mut paths: Vec<String> = Vec::new();
    for arg in args {
        if arg == "-a" {
            append = true;
        } else if arg.len() > 1 && arg != "-" && arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            paths.push(arg.clone());
        }
    }
    Ok((append, paths))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn no_args_defaults() {
        let (a, p) = parse_args(&s(&[])).unwrap();
        assert!(!a);
        assert!(p.is_empty());
    }

    #[test]
    fn dash_a_sets_append() {
        let (a, p) = parse_args(&s(&["-a", "out.txt"])).unwrap();
        assert!(a);
        assert_eq!(p, vec!["out.txt"]);
    }

    #[test]
    fn no_append_default() {
        let (a, p) = parse_args(&s(&["out.txt"])).unwrap();
        assert!(!a);
        assert_eq!(p, vec!["out.txt"]);
    }

    #[test]
    fn multiple_files() {
        let (a, p) = parse_args(&s(&["one.txt", "two.txt", "three.txt"])).unwrap();
        assert!(!a);
        assert_eq!(p, vec!["one.txt", "two.txt", "three.txt"]);
    }

    #[test]
    fn unknown_flag_returns_error() {
        let err = parse_args(&s(&["-z"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn dash_alone_treated_as_path() {
        let (a, p) = parse_args(&s(&["-"])).unwrap();
        assert!(!a);
        assert_eq!(p, vec!["-"]);
    }

    #[test]
    fn dash_a_at_end() {
        let (a, p) = parse_args(&s(&["out.txt", "-a"])).unwrap();
        assert!(a);
        assert_eq!(p, vec!["out.txt"]);
    }

    #[test]
    fn duplicate_dash_a_idempotent() {
        let (a, p) = parse_args(&s(&["-a", "-a"])).unwrap();
        assert!(a);
        assert!(p.is_empty());
    }
}
