//! ln -- create links between files.
//!
//! Usage: ln [-s] TARGET LINK_NAME
//!   -s  create a symbolic link instead of a hard link

use std::env;
use std::process;

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct LnArgs {
    symbolic: bool,
    target: String,
    link_name: String,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ln: {e}");
            process::exit(1);
        }
    };

    let result = if parsed.symbolic {
        symlink(&parsed.target, &parsed.link_name)
    } else {
        std::fs::hard_link(&parsed.target, &parsed.link_name)
    };

    if let Err(e) = result {
        let kind = if parsed.symbolic { "symbolic" } else { "hard" };
        eprintln!(
            "ln: cannot create {kind} link '{}' -> '{}': {e}",
            parsed.link_name, parsed.target
        );
        process::exit(1);
    }
}

/// Parse ln's argv. Combined short options like `-sf` are split per char.
fn parse_args(args: &[String]) -> Result<LnArgs, String> {
    let mut symbolic = false;
    let mut paths: Vec<String> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            for c in arg.chars().skip(1) {
                match c {
                    's' => symbolic = true,
                    _ => return Err(format!("unknown option: -{c}")),
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    if paths.len() != 2 {
        return Err("expected exactly two arguments: TARGET LINK_NAME".to_string());
    }

    let link_name = paths.pop().unwrap_or_default();
    let target = paths.pop().unwrap_or_default();
    Ok(LnArgs { symbolic, target, link_name })
}

/// Create a symbolic link. Delegates to the platform-specific API.
#[cfg(unix)]
fn symlink(target: &str, link_name: &str) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link_name)
}

#[cfg(windows)]
fn symlink(target: &str, link_name: &str) -> std::io::Result<()> {
    let target_path = std::path::Path::new(target);
    if target_path.is_dir() {
        std::os::windows::fs::symlink_dir(target, link_name)
    } else {
        std::os::windows::fs::symlink_file(target, link_name)
    }
}

#[cfg(not(any(unix, windows)))]
fn symlink(_target: &str, _link_name: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symbolic links not supported on this platform",
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn hard_link_two_args() {
        let p = parse_args(&s(&["a.txt", "b.txt"])).unwrap();
        assert!(!p.symbolic);
        assert_eq!(p.target, "a.txt");
        assert_eq!(p.link_name, "b.txt");
    }

    #[test]
    fn symbolic_flag() {
        let p = parse_args(&s(&["-s", "a.txt", "b.txt"])).unwrap();
        assert!(p.symbolic);
        assert_eq!(p.target, "a.txt");
        assert_eq!(p.link_name, "b.txt");
    }

    #[test]
    fn symbolic_flag_at_end() {
        let p = parse_args(&s(&["a.txt", "b.txt", "-s"])).unwrap();
        assert!(p.symbolic);
    }

    #[test]
    fn too_few_args() {
        let err = parse_args(&s(&["a.txt"])).unwrap_err();
        assert!(err.contains("exactly two"));
    }

    #[test]
    fn no_args() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("exactly two"));
    }

    #[test]
    fn too_many_args() {
        let err = parse_args(&s(&["a", "b", "c"])).unwrap_err();
        assert!(err.contains("exactly two"));
    }

    #[test]
    fn unknown_flag() {
        let err = parse_args(&s(&["-x", "a", "b"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn combined_short_options_with_s() {
        // `-ss` is duplicate, idempotent.
        let p = parse_args(&s(&["-ss", "a", "b"])).unwrap();
        assert!(p.symbolic);
    }

    #[test]
    fn combined_short_options_with_unknown() {
        let err = parse_args(&s(&["-sx", "a", "b"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn duplicate_dash_s_idempotent() {
        let p = parse_args(&s(&["-s", "-s", "a", "b"])).unwrap();
        assert!(p.symbolic);
    }
}
