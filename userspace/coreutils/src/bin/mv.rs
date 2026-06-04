//! mv -- move or rename files and directories.
//!
//! Usage: mv [-f] SOURCE... DEST
//!   -f  force: do not report errors when overwriting existing files

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MvArgs {
    force: bool,
    /// All positional arguments; last is dest, rest are sources.
    paths: Vec<String>,
}

/// Parse mv's argv.  Returns an error if an unknown short flag is seen.
fn parse_args(args: &[String]) -> Result<MvArgs, String> {
    let mut out = MvArgs::default();
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            let rest = arg.get(1..).unwrap_or("");
            for c in rest.chars() {
                match c {
                    'f' => out.force = true,
                    other => return Err(format!("unknown option: -{other}")),
                }
            }
        } else {
            out.paths.push(arg.clone());
        }
    }
    Ok(out)
}

/// Resolve the per-source target path: when `dest_is_dir`, append the
/// source's file-name component to `dest`; otherwise use `dest` itself.
fn compute_target(src: &Path, dest: &Path, dest_is_dir: bool) -> PathBuf {
    if dest_is_dir {
        let name = src.file_name().unwrap_or_default();
        dest.join(name)
    } else {
        dest.to_path_buf()
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mv: {e}");
            process::exit(1);
        }
    };

    if parsed.paths.len() < 2 {
        eprintln!("mv: missing operand");
        process::exit(1);
    }

    let dest = parsed.paths.last().cloned().unwrap_or_default();
    let sources = parsed.paths.get(..parsed.paths.len().saturating_sub(1)).unwrap_or(&[]);
    let dest_path = PathBuf::from(&dest);
    let dest_is_dir = dest_path.is_dir();

    if sources.len() > 1 && !dest_is_dir {
        eprintln!("mv: target '{dest}' is not a directory");
        process::exit(1);
    }

    let mut failed = false;
    for src_str in sources {
        let src = Path::new(src_str);
        let target = compute_target(src, &dest_path, dest_is_dir);

        if let Err(e) = fs::rename(src, &target) {
            // rename() can fail across filesystems; fall back to copy + remove.
            if src.is_dir() {
                eprintln!(
                    "mv: cannot move '{src_str}' to '{}': {e}",
                    target.display()
                );
                failed = true;
            } else {
                match fs::copy(src, &target).and_then(|_| fs::remove_file(src)) {
                    Ok(_) => {}
                    Err(e2) => {
                        if !parsed.force {
                            eprintln!(
                                "mv: cannot move '{src_str}' to '{}': {e2}",
                                target.display()
                            );
                        }
                        failed = true;
                    }
                }
            }
        }
    }

    if failed {
        process::exit(1);
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
        let a = parse_args(&s(&[])).unwrap();
        assert!(!a.force);
        assert!(a.paths.is_empty());
    }

    #[test]
    fn parse_simple_rename() {
        let a = parse_args(&s(&["a", "b"])).unwrap();
        assert!(!a.force);
        assert_eq!(a.paths, vec!["a", "b"]);
    }

    #[test]
    fn parse_dash_f_force() {
        let a = parse_args(&s(&["-f", "a", "b"])).unwrap();
        assert!(a.force);
        assert_eq!(a.paths, vec!["a", "b"]);
    }

    #[test]
    fn parse_force_clustered() {
        let a = parse_args(&s(&["-ff", "a", "b"])).unwrap();
        assert!(a.force);
    }

    #[test]
    fn parse_unknown_flag_errors() {
        let err = parse_args(&s(&["-z", "a", "b"])).unwrap_err();
        assert!(err.contains("unknown option"));
        assert!(err.contains('z'));
    }

    #[test]
    fn parse_multiple_sources() {
        let a = parse_args(&s(&["a", "b", "c", "dest_dir"])).unwrap();
        assert_eq!(a.paths, vec!["a", "b", "c", "dest_dir"]);
    }

    #[test]
    fn parse_force_at_end() {
        let a = parse_args(&s(&["a", "b", "-f"])).unwrap();
        assert!(a.force);
        assert_eq!(a.paths, vec!["a", "b"]);
    }

    #[test]
    fn parse_bare_dash_is_path() {
        let a = parse_args(&s(&["-", "dest"])).unwrap();
        assert_eq!(a.paths, vec!["-", "dest"]);
    }

    // ---------------- compute_target ----------------

    #[test]
    fn target_file_to_file() {
        let t = compute_target(Path::new("a.txt"), Path::new("b.txt"), false);
        assert_eq!(t, PathBuf::from("b.txt"));
    }

    #[test]
    fn target_file_into_dir() {
        let t = compute_target(Path::new("src/a.txt"), Path::new("dst"), true);
        assert_eq!(t, PathBuf::from("dst").join("a.txt"));
    }

    #[test]
    fn target_rename_within_dir() {
        let t = compute_target(Path::new("./old"), Path::new("new"), false);
        assert_eq!(t, PathBuf::from("new"));
    }

    #[test]
    fn target_nested_source_into_dir() {
        let t = compute_target(Path::new("a/b/c.txt"), Path::new("/tmp"), true);
        assert_eq!(t, PathBuf::from("/tmp").join("c.txt"));
    }
}
