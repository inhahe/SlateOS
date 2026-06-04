//! rmdir — remove empty directories.
//!
//! Usage: rmdir [-p] DIRECTORY...
//!   -p  remove parent directories as well if they become empty

use std::env;
use std::fs;
use std::path::Path;
use std::process;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct RmdirArgs {
    parents: bool,
    dirs: Vec<String>,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rmdir: {e}");
            process::exit(1);
        }
    };

    if parsed.dirs.is_empty() {
        eprintln!("rmdir: missing operand");
        process::exit(1);
    }

    let mut failed = false;
    for dir in &parsed.dirs {
        if let Err(e) = fs::remove_dir(dir) {
            eprintln!("rmdir: failed to remove '{dir}': {e}");
            failed = true;
            continue;
        }
        if parsed.parents {
            for parent in parent_chain(Path::new(dir)) {
                if fs::remove_dir(&parent).is_err() {
                    break;
                }
            }
        }
    }

    if failed {
        process::exit(1);
    }
}

/// Parse rmdir's argv into `(parents, dirs)`.
fn parse_args(args: &[String]) -> Result<RmdirArgs, String> {
    let mut parents = false;
    let mut dirs: Vec<String> = Vec::new();
    for arg in args {
        if arg == "-p" {
            parents = true;
        } else if arg.starts_with('-') && arg.len() > 1 {
            return Err(format!("unknown option: {arg}"));
        } else {
            dirs.push(arg.clone());
        }
    }
    Ok(RmdirArgs { parents, dirs })
}

/// Walk parents of `dir` upward until an empty/root ancestor is reached.
/// Returns the candidate parents in the order rmdir would try to remove them.
fn parent_chain(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out: Vec<std::path::PathBuf> = Vec::new();
    let mut p = dir.parent();
    while let Some(parent) = p {
        if parent.as_os_str().is_empty() {
            break;
        }
        out.push(parent.to_path_buf());
        p = parent.parent();
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_no_args() {
        let a = parse_args(&s(&[])).unwrap();
        assert!(!a.parents);
        assert!(a.dirs.is_empty());
    }

    #[test]
    fn parse_dirs_only() {
        let a = parse_args(&s(&["foo", "bar"])).unwrap();
        assert!(!a.parents);
        assert_eq!(a.dirs, vec!["foo", "bar"]);
    }

    #[test]
    fn parse_dash_p() {
        let a = parse_args(&s(&["-p", "foo"])).unwrap();
        assert!(a.parents);
        assert_eq!(a.dirs, vec!["foo"]);
    }

    #[test]
    fn parse_dash_p_at_end() {
        let a = parse_args(&s(&["foo", "-p"])).unwrap();
        assert!(a.parents);
        assert_eq!(a.dirs, vec!["foo"]);
    }

    #[test]
    fn parse_unknown_flag() {
        let err = parse_args(&s(&["-z"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_dash_alone_as_dir() {
        let a = parse_args(&s(&["-"])).unwrap();
        assert!(!a.parents);
        assert_eq!(a.dirs, vec!["-"]);
    }

    #[test]
    fn parent_chain_simple() {
        let chain = parent_chain(Path::new("a/b/c"));
        assert_eq!(chain, vec![PathBuf::from("a/b"), PathBuf::from("a")]);
    }

    #[test]
    fn parent_chain_single_component() {
        // "foo" has no useful parent.
        let chain = parent_chain(Path::new("foo"));
        assert!(chain.is_empty());
    }

    #[test]
    fn parent_chain_deep() {
        let chain = parent_chain(Path::new("a/b/c/d"));
        assert_eq!(
            chain,
            vec![
                PathBuf::from("a/b/c"),
                PathBuf::from("a/b"),
                PathBuf::from("a"),
            ]
        );
    }

    #[test]
    fn parent_chain_empty_path() {
        let chain = parent_chain(Path::new(""));
        assert!(chain.is_empty());
    }
}
