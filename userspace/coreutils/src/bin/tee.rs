//! tee — read from stdin, write to stdout and files.
//!
//! Usage: tee [-a|--append] [--] [FILE...]
//!   -a, --append  append to files instead of overwriting
//!   --            end of options; every later argument is a file name
//!
//! `tee` exists to put a copy of a stream somewhere durable, so the one thing
//! it must never do is report success after failing to write that copy. Three
//! rules follow, and all three were once broken (`known-issues.md` →
//! `B-tee-REPORTS-SUCCESS-AFTER-LOSING-THE-DATA`):
//!
//! 1. A file that could not be opened is an error, not a warning: the exit
//!    status is 1 even though the rest of the copy succeeds.
//! 2. A write that fails is reported and drops that destination, and the exit
//!    status is 1. It is never discarded.
//! 3. The final flush is checked. Buffered output that fails on flush is data
//!    the caller believes was written.
//!
//! The one deliberate exception is a broken pipe on stdout, which is how a
//! pipeline shuts down normally — `cmd | tee log | head -1` — and is not an
//! error. `-i`/`--ignore-interrupts` is absent because this OS does not use
//! Unix signals for process control (`design.txt`); there is no SIGINT to
//! ignore.

use coreutils::quote::quotef_os;
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
            eprintln!("Usage: tee [-a|--append] [--] [FILE...]");
            process::exit(1);
        }
    };

    // Anything that goes wrong after this point still copies as much as it can
    // — that is what tee is for — but it must not exit 0. A script doing
    // `build | tee /var/log/build` needs to learn that the log was not written.
    let mut status = 0;

    // The name is kept alongside the handle so a write failure can say which
    // file it was. `File` alone cannot tell you its own path.
    let mut files: Vec<(String, File)> = Vec::new();
    for path in &paths {
        let opened = if append {
            OpenOptions::new().create(true).append(true).open(path)
        } else {
            File::create(path)
        };
        match opened {
            Ok(f) => files.push((path.clone(), f)),
            Err(e) => {
                eprintln!("tee: {}: {e}", quotef_os(path));
                status = 1;
            }
        }
    }

    let mut buf = [0u8; 8192];
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut stdout_live = true;

    loop {
        let n = match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                eprintln!("tee: read error: {e}");
                process::exit(1);
            }
        };
        let chunk = buf.get(..n).unwrap_or(&[]);

        if stdout_live
            && let Err(e) = out.write_all(chunk)
        {
            // A downstream reader that has gone away is an ordinary end to a
            // pipeline, not a failure of this program.
            if e.kind() == io::ErrorKind::BrokenPipe {
                process::exit(status);
            }
            eprintln!("tee: 'standard output': {e}");
            status = 1;
            stdout_live = false;
        }

        // A failed destination is dropped rather than retried on every block,
        // so one full disk does not produce one message per 8 KiB of input.
        files.retain_mut(|(name, f)| match f.write_all(chunk) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("tee: {}: {e}", quotef_os(name));
                status = 1;
                false
            }
        });
    }

    // Buffered data that fails here is data the caller was told had been
    // written. Dropping the handles would swallow exactly that error.
    if stdout_live
        && let Err(e) = out.flush()
        && e.kind() != io::ErrorKind::BrokenPipe
    {
        eprintln!("tee: 'standard output': {e}");
        status = 1;
    }
    for (name, f) in &mut files {
        if let Err(e) = f.flush() {
            eprintln!("tee: {}: {e}", quotef_os(name));
            status = 1;
        }
    }

    process::exit(status);
}

/// Parse tee's argv into `(append, paths)`. Returns an error on unknown
/// flags. `-` alone (and any non-flag arg) is treated as a file path.
///
/// Options are permuted, as GNU's are, so `tee out.txt -a` appends. `--` stops
/// that, which is the only way to name a file `-a`.
fn parse_args(args: &[String]) -> Result<(bool, Vec<String>), String> {
    let mut append = false;
    let mut paths: Vec<String> = Vec::new();
    let mut end_of_options = false;
    for arg in args {
        if end_of_options {
            paths.push(arg.clone());
        } else if arg == "--" {
            end_of_options = true;
        } else if arg == "-a" || arg == "--append" {
            append = true;
        } else if arg.len() > 1 && arg != "-" && arg.starts_with('-') {
            return Err(format!("unknown option: {arg}"));
        } else {
            // A bare `-` is a file named `-`, not stdout: GNU tee has no
            // special case for it.
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
    fn long_append_sets_append() {
        let (a, p) = parse_args(&s(&["--append", "out.txt"])).unwrap();
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

    // Without `--` there is no way to write to a file called `-a`; it would be
    // read as the append flag and silently produce no file at all.
    #[test]
    fn double_dash_ends_options() {
        let (a, p) = parse_args(&s(&["--", "-a", "-z"])).unwrap();
        assert!(!a);
        assert_eq!(p, vec!["-a", "-z"]);
    }

    #[test]
    fn double_dash_after_a_real_flag() {
        let (a, p) = parse_args(&s(&["-a", "--", "-z"])).unwrap();
        assert!(a);
        assert_eq!(p, vec!["-z"]);
    }
}
