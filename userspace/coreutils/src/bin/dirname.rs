//! dirname — strip last component from file name.
//!
//! Usage: dirname PATH

use std::env;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("dirname: missing operand");
        process::exit(1);
    }
    for arg in &args {
        println!("{}", dirname_of(arg));
    }
}

/// Return the directory portion of `path`, or `"."` if the path has no
/// directory component. Pure helper — unit-testable without I/O.
fn dirname_of(path: &str) -> String {
    let p = Path::new(path);
    match p.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.display().to_string(),
        _ => ".".to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn simple_file_returns_dot() {
        assert_eq!(dirname_of("foo.txt"), ".");
    }

    #[test]
    fn absolute_path() {
        assert_eq!(dirname_of("/usr/bin/cat"), "/usr/bin");
    }

    #[test]
    fn relative_nested_path() {
        assert_eq!(dirname_of("dir/sub/file"), "dir/sub");
    }

    #[test]
    fn root_path_returns_dot() {
        // Path::new("/").parent() is None on Unix and on Windows.
        assert_eq!(dirname_of("/"), ".");
    }

    #[test]
    fn empty_path_returns_dot() {
        assert_eq!(dirname_of(""), ".");
    }

    #[test]
    fn trailing_slash() {
        // Path::new("/usr/bin/").parent() == Path::new("/usr") — note that
        // a trailing slash effectively strips the empty component.
        assert_eq!(dirname_of("/usr/bin/"), "/usr");
    }

    #[test]
    fn top_level_absolute() {
        assert_eq!(dirname_of("/etc"), "/");
    }

    #[test]
    fn top_level_relative() {
        assert_eq!(dirname_of("foo/bar"), "foo");
    }

    #[test]
    fn dot_returns_dot() {
        // Path::new(".").parent() is Some("") -> fall through to ".".
        assert_eq!(dirname_of("."), ".");
    }

    #[test]
    fn dotdot_returns_dot() {
        assert_eq!(dirname_of(".."), ".");
    }

    #[test]
    fn deeply_nested() {
        assert_eq!(dirname_of("/a/b/c/d/e/f"), "/a/b/c/d/e");
    }
}
