//! readlink — print resolved symbolic links or canonical file names.
//!
//! Usage: readlink [-f] FILE...
//!   Without -f: print the target of a symbolic link.
//!   With -f: canonicalize the entire path (resolve all symlinks,
//!            make absolute). Like `realpath`.

use std::env;
use std::fs;
use std::process;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ReadlinkArgs {
    canonicalize: bool,
    files: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = parse_args(&args);

    if parsed.files.is_empty() {
        eprintln!("readlink: missing operand");
        process::exit(1);
    }

    let mut exit_code = 0;
    for path_str in &parsed.files {
        if parsed.canonicalize {
            match fs::canonicalize(path_str) {
                Ok(p) => println!("{}", p.display()),
                Err(e) => {
                    eprintln!("readlink: {path_str}: {e}");
                    exit_code = 1;
                }
            }
        } else {
            match fs::read_link(path_str) {
                Ok(target) => println!("{}", target.display()),
                Err(e) => {
                    eprintln!("readlink: {path_str}: {e}");
                    exit_code = 1;
                }
            }
        }
    }

    process::exit(exit_code);
}

/// Parse readlink's argv. Treats -f / -e / -m as the canonicalize flag and
/// anything else as a file path.
fn parse_args(args: &[String]) -> ReadlinkArgs {
    let mut canonicalize = false;
    let mut files: Vec<String> = Vec::new();
    for arg in args {
        if arg == "-f" || arg == "-e" || arg == "-m" {
            canonicalize = true;
        } else {
            files.push(arg.clone());
        }
    }
    ReadlinkArgs { canonicalize, files }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn no_args_returns_no_files_no_canon() {
        let p = parse_args(&s(&[]));
        assert!(!p.canonicalize);
        assert!(p.files.is_empty());
    }

    #[test]
    fn single_file() {
        let p = parse_args(&s(&["foo"]));
        assert!(!p.canonicalize);
        assert_eq!(p.files, vec!["foo"]);
    }

    #[test]
    fn dash_f_sets_canon() {
        let p = parse_args(&s(&["-f", "foo"]));
        assert!(p.canonicalize);
        assert_eq!(p.files, vec!["foo"]);
    }

    #[test]
    fn dash_e_sets_canon() {
        let p = parse_args(&s(&["-e", "foo"]));
        assert!(p.canonicalize);
        assert_eq!(p.files, vec!["foo"]);
    }

    #[test]
    fn dash_m_sets_canon() {
        let p = parse_args(&s(&["-m", "foo"]));
        assert!(p.canonicalize);
        assert_eq!(p.files, vec!["foo"]);
    }

    #[test]
    fn multiple_files() {
        let p = parse_args(&s(&["a", "b", "c"]));
        assert!(!p.canonicalize);
        assert_eq!(p.files, vec!["a", "b", "c"]);
    }

    #[test]
    fn flag_at_end() {
        let p = parse_args(&s(&["foo", "-f"]));
        assert!(p.canonicalize);
        assert_eq!(p.files, vec!["foo"]);
    }

    #[test]
    fn unknown_flag_treated_as_path() {
        // Implementation doesn't validate non-supported flags.
        let p = parse_args(&s(&["-x", "foo"]));
        assert!(!p.canonicalize);
        assert_eq!(p.files, vec!["-x", "foo"]);
    }
}
