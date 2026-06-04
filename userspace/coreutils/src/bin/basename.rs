//! basename -- strip directory and optional suffix from a pathname.
//!
//! Usage: basename PATH [SUFFIX]
//!   Print the final component of PATH, removing a trailing SUFFIX if given.

use std::env;
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("basename: missing operand");
        process::exit(1);
    }

    let path = &args[0];
    let suffix = args.get(1).map(|s| s.as_str());

    println!("{}", basename(path, suffix));
}

/// Compute the final path component of `path`, optionally stripping a
/// trailing `suffix`. Pure helper — unit-testable without I/O.
///
/// POSIX semantics:
/// - The suffix is removed only if the base name is strictly longer than
///   the suffix (so `basename foo .foo` -> `foo`, not the empty string).
/// - The path `/` returns `/`.
/// - Empty path returns empty string.
fn basename(path: &str, suffix: Option<&str>) -> String {
    let base = Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            // Path::file_name returns None for paths like "/", "..", and
            // ".", and for empty paths. Preserve "/" verbatim; otherwise
            // return the path unchanged.
            if path == "/" {
                "/".to_string()
            } else {
                path.to_string()
            }
        });

    match suffix {
        Some(sfx) if !sfx.is_empty() && base.len() > sfx.len() && base.ends_with(sfx) => {
            base[..base.len() - sfx.len()].to_string()
        }
        _ => base,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn plain_filename() {
        assert_eq!(basename("foo.txt", None), "foo.txt");
    }

    #[test]
    fn absolute_path() {
        assert_eq!(basename("/usr/bin/cat", None), "cat");
    }

    #[test]
    fn relative_path() {
        assert_eq!(basename("dir/file", None), "file");
    }

    #[test]
    fn root_path() {
        assert_eq!(basename("/", None), "/");
    }

    #[test]
    fn empty_path() {
        assert_eq!(basename("", None), "");
    }

    #[test]
    fn trailing_slash_keeps_last_component() {
        // Path::file_name strips a trailing slash.
        assert_eq!(basename("/usr/bin/", None), "bin");
    }

    #[test]
    fn suffix_stripped_when_matching() {
        assert_eq!(basename("foo.txt", Some(".txt")), "foo");
        assert_eq!(basename("/path/to/script.sh", Some(".sh")), "script");
    }

    #[test]
    fn suffix_not_stripped_when_equal_length() {
        // POSIX: base.len() > sfx.len() — so identical names are not stripped
        // (otherwise basename returns empty string).
        assert_eq!(basename("foo", Some("foo")), "foo");
        assert_eq!(basename(".txt", Some(".txt")), ".txt");
    }

    #[test]
    fn suffix_not_stripped_when_not_suffix() {
        assert_eq!(basename("foo.txt", Some(".sh")), "foo.txt");
        assert_eq!(basename("readme", Some(".md")), "readme");
    }

    #[test]
    fn empty_suffix_no_op() {
        assert_eq!(basename("foo.txt", Some("")), "foo.txt");
    }

    #[test]
    fn suffix_longer_than_base_no_op() {
        assert_eq!(basename("ab", Some("xyz")), "ab");
    }

    #[test]
    fn nested_path_strips_to_final_component() {
        assert_eq!(basename("/a/b/c/d/e/f.tar.gz", None), "f.tar.gz");
        assert_eq!(basename("/a/b/c/d/e/f.tar.gz", Some(".gz")), "f.tar");
    }

    #[test]
    fn dot_and_dotdot() {
        // Path::file_name returns None for "." and ".." — fall through to
        // the path-verbatim branch.
        assert_eq!(basename(".", None), ".");
        assert_eq!(basename("..", None), "..");
    }

    #[test]
    fn hidden_file() {
        assert_eq!(basename("/home/user/.bashrc", None), ".bashrc");
    }
}
