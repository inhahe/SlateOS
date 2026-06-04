//! cp — copy files and directories.
//!
//! Usage: cp [-r] SOURCE DEST
//!        cp [-r] SOURCE... DIRECTORY
//!   -r  copy directories recursively

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct CpArgs {
    recursive: bool,
    /// All positional arguments.  The last one is the destination; the
    /// rest are sources.  We keep them in a single vector so the caller
    /// can decide once whether the destination is a directory.
    paths: Vec<String>,
}

/// Parse cp's argv.  Returns an error if an unknown short flag is seen.
fn parse_args(args: &[String]) -> Result<CpArgs, String> {
    let mut out = CpArgs::default();
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            let rest = arg.get(1..).unwrap_or("");
            for c in rest.chars() {
                match c {
                    'r' | 'R' => out.recursive = true,
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
/// Returns `dest` unchanged if the source has no file-name (e.g. "/").
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
            eprintln!("cp: {e}");
            process::exit(1);
        }
    };

    if parsed.paths.len() < 2 {
        eprintln!("cp: missing operand");
        process::exit(1);
    }

    let dest = parsed.paths.last().cloned().unwrap_or_default();
    let sources = parsed.paths.get(..parsed.paths.len().saturating_sub(1)).unwrap_or(&[]);
    let dest_path = PathBuf::from(&dest);
    let dest_is_dir = dest_path.is_dir();

    if sources.len() > 1 && !dest_is_dir {
        eprintln!("cp: target '{dest}' is not a directory");
        process::exit(1);
    }

    let mut failed = false;
    for src_str in sources {
        let src = Path::new(src_str);
        let target = compute_target(src, &dest_path, dest_is_dir);

        if src.is_dir() {
            if !parsed.recursive {
                eprintln!("cp: omitting directory '{src_str}'");
                failed = true;
                continue;
            }
            if let Err(e) = copy_dir_recursive(src, &target) {
                eprintln!("cp: error copying '{src_str}': {e}");
                failed = true;
            }
        } else if let Err(e) = fs::copy(src, &target) {
            eprintln!(
                "cp: error copying '{src_str}' to '{}': {e}",
                target.display()
            );
            failed = true;
        }
    }

    if failed {
        process::exit(1);
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
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
        assert!(!a.recursive);
        assert!(a.paths.is_empty());
    }

    #[test]
    fn parse_simple_copy() {
        let a = parse_args(&s(&["a", "b"])).unwrap();
        assert!(!a.recursive);
        assert_eq!(a.paths, vec!["a", "b"]);
    }

    #[test]
    fn parse_dash_r_sets_recursive() {
        let a = parse_args(&s(&["-r", "src", "dst"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.paths, vec!["src", "dst"]);
    }

    #[test]
    fn parse_dash_uppercase_r_also_recursive() {
        let a = parse_args(&s(&["-R", "src", "dst"])).unwrap();
        assert!(a.recursive);
    }

    #[test]
    fn parse_clustered_r() {
        // Only -r/-R are supported; clustering one with itself is fine.
        let a = parse_args(&s(&["-rR", "a", "b"])).unwrap();
        assert!(a.recursive);
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
    fn parse_flag_at_end_treated_as_flag() {
        let a = parse_args(&s(&["a", "b", "-r"])).unwrap();
        assert!(a.recursive);
        assert_eq!(a.paths, vec!["a", "b"]);
    }

    #[test]
    fn parse_bare_dash_treated_as_path() {
        // arg == "-" is not a flag (no chars after), so it's a path.
        let a = parse_args(&s(&["-", "dest"])).unwrap();
        assert!(!a.recursive);
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
    fn target_dir_into_dir_appends_basename() {
        let t = compute_target(Path::new("src/sub"), Path::new("dst"), true);
        assert_eq!(t, PathBuf::from("dst").join("sub"));
    }

    #[test]
    fn target_source_with_no_filename_into_dir() {
        // Path with no file-name (e.g. "/") joins empty string → "dst".
        let t = compute_target(Path::new("/"), Path::new("dst"), true);
        assert_eq!(t, PathBuf::from("dst").join(""));
    }

    #[test]
    fn target_absolute_dest_not_dir() {
        let t = compute_target(Path::new("a"), Path::new("/etc/passwd"), false);
        assert_eq!(t, PathBuf::from("/etc/passwd"));
    }
}
