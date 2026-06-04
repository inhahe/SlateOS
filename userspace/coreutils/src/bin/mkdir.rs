//! mkdir — make directories.
//!
//! Usage: mkdir [-p] DIRECTORY...
//!   -p  make parent directories as needed, no error if existing

use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (parents, dirs) = match parse_args(&args) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("mkdir: {e}");
            process::exit(1);
        }
    };

    if dirs.is_empty() {
        eprintln!("mkdir: missing operand");
        process::exit(1);
    }

    let mut failed = false;
    for dir in &dirs {
        let result = if parents {
            fs::create_dir_all(dir)
        } else {
            fs::create_dir(dir)
        };
        if let Err(e) = result {
            eprintln!("mkdir: cannot create directory '{dir}': {e}");
            failed = true;
        }
    }

    if failed {
        process::exit(1);
    }
}

/// Parse mkdir's argv into `(parents, dirs)`. Returns an error string for
/// unknown options; the caller is expected to print and exit.
fn parse_args(args: &[String]) -> Result<(bool, Vec<String>), String> {
    let mut parents = false;
    let mut dirs: Vec<String> = Vec::new();
    for arg in args {
        if arg == "-p" {
            parents = true;
        } else if arg.starts_with('-') && arg != "-" {
            return Err(format!("unknown option: {arg}"));
        } else {
            dirs.push(arg.clone());
        }
    }
    Ok((parents, dirs))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn no_args_returns_no_parents_no_dirs() {
        let (p, d) = parse_args(&s(&[])).unwrap();
        assert!(!p);
        assert!(d.is_empty());
    }

    #[test]
    fn dirs_only() {
        let (p, d) = parse_args(&s(&["foo", "bar"])).unwrap();
        assert!(!p);
        assert_eq!(d, vec!["foo", "bar"]);
    }

    #[test]
    fn dash_p_sets_parents() {
        let (p, d) = parse_args(&s(&["-p", "foo/bar"])).unwrap();
        assert!(p);
        assert_eq!(d, vec!["foo/bar"]);
    }

    #[test]
    fn dash_p_at_end_too() {
        let (p, d) = parse_args(&s(&["foo", "-p"])).unwrap();
        assert!(p);
        assert_eq!(d, vec!["foo"]);
    }

    #[test]
    fn duplicate_dash_p_idempotent() {
        let (p, d) = parse_args(&s(&["-p", "-p", "foo"])).unwrap();
        assert!(p);
        assert_eq!(d, vec!["foo"]);
    }

    #[test]
    fn unknown_option_returns_error() {
        let err = parse_args(&s(&["-q"])).unwrap_err();
        assert!(err.contains("unknown option"));
        assert!(err.contains("-q"));
    }

    #[test]
    fn dash_alone_treated_as_dir_name() {
        // POSIX-style: bare "-" is a literal path (though weird).
        let (p, d) = parse_args(&s(&["-"])).unwrap();
        assert!(!p);
        assert_eq!(d, vec!["-"]);
    }
}
