//! rm — remove files or directories.
//!
//! Usage: rm [-r] [-f] FILE...
//!   -r  remove directories and their contents recursively
//!   -f  ignore nonexistent files, never prompt

use std::env;
use std::fs;
use std::path::Path;
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct RmFlags {
    recursive: bool,
    force: bool,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (flags, paths) = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rm: {e}");
            process::exit(1);
        }
    };

    if paths.is_empty() {
        if !flags.force {
            eprintln!("rm: missing operand");
            process::exit(1);
        }
        return;
    }

    let mut failed = false;
    for path_str in &paths {
        let path = Path::new(path_str);
        if !path.exists() {
            if !flags.force {
                eprintln!("rm: cannot remove '{path_str}': No such file or directory");
                failed = true;
            }
            continue;
        }

        let result = if path.is_dir() {
            if flags.recursive {
                fs::remove_dir_all(path)
            } else {
                eprintln!("rm: cannot remove '{path_str}': Is a directory");
                failed = true;
                continue;
            }
        } else {
            fs::remove_file(path)
        };

        if let Err(e) = result
            && !flags.force
        {
            eprintln!("rm: cannot remove '{path_str}': {e}");
            failed = true;
        }
    }

    if failed {
        process::exit(1);
    }
}

/// Parse rm's argv into `(flags, paths)`. Combined short options like
/// `-rf` are split per char.
fn parse_args(args: &[String]) -> Result<(RmFlags, Vec<String>), String> {
    let mut flags = RmFlags::default();
    let mut paths: Vec<String> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    'r' | 'R' => flags.recursive = true,
                    'f' => flags.force = true,
                    _ => return Err(format!("unknown option: -{c}")),
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    Ok((flags, paths))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn no_args() {
        let (f, p) = parse_args(&s(&[])).unwrap();
        assert!(!f.recursive && !f.force);
        assert!(p.is_empty());
    }

    #[test]
    fn just_paths() {
        let (f, p) = parse_args(&s(&["a", "b"])).unwrap();
        assert!(!f.recursive && !f.force);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn dash_r_sets_recursive() {
        let (f, _) = parse_args(&s(&["-r", "a"])).unwrap();
        assert!(f.recursive);
        assert!(!f.force);
    }

    #[test]
    fn capital_r_also_recursive() {
        let (f, _) = parse_args(&s(&["-R", "a"])).unwrap();
        assert!(f.recursive);
    }

    #[test]
    fn dash_f_sets_force() {
        let (f, _) = parse_args(&s(&["-f", "a"])).unwrap();
        assert!(!f.recursive);
        assert!(f.force);
    }

    #[test]
    fn combined_rf() {
        let (f, p) = parse_args(&s(&["-rf", "a"])).unwrap();
        assert!(f.recursive && f.force);
        assert_eq!(p, vec!["a"]);
    }

    #[test]
    fn combined_fr_order_irrelevant() {
        let (f, _) = parse_args(&s(&["-fr", "a"])).unwrap();
        assert!(f.recursive && f.force);
    }

    #[test]
    fn split_flags() {
        let (f, _) = parse_args(&s(&["-r", "-f", "a"])).unwrap();
        assert!(f.recursive && f.force);
    }

    #[test]
    fn unknown_flag() {
        let err = parse_args(&s(&["-x", "a"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn combined_with_unknown_errors() {
        let err = parse_args(&s(&["-rx", "a"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn dash_alone_is_path() {
        let (_, p) = parse_args(&s(&["-"])).unwrap();
        assert_eq!(p, vec!["-"]);
    }

    #[test]
    fn flag_at_end() {
        let (f, p) = parse_args(&s(&["a", "-r"])).unwrap();
        assert!(f.recursive);
        assert_eq!(p, vec!["a"]);
    }
}
