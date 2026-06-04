//! which — locate a command.
//!
//! Usage: which COMMAND...
//!   Searches PATH for each COMMAND and prints the first match.

use std::env;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        process::exit(1);
    }

    let path_var = env::var("PATH").unwrap_or_default();
    let dirs: Vec<&str> = split_path(&path_var);

    let mut failed = false;
    for cmd in &args {
        match find_command(cmd, &dirs, |p| p.exists()) {
            Some(found) => println!("{}", found.display()),
            None => {
                eprintln!("which: no {cmd} in ({path_var})");
                failed = true;
            }
        }
    }

    if failed {
        process::exit(1);
    }
}

/// Split a PATH-style colon-separated variable into directory entries.
fn split_path(path_var: &str) -> Vec<&str> {
    path_var.split(':').collect()
}

/// Find the first match for `cmd` in `dirs`, using `exists` to probe the
/// filesystem. If `cmd` contains a slash it is treated as a literal path.
fn find_command<F>(cmd: &str, dirs: &[&str], mut exists: F) -> Option<PathBuf>
where
    F: FnMut(&Path) -> bool,
{
    if cmd.contains('/') {
        let p = PathBuf::from(cmd);
        if exists(&p) {
            return Some(p);
        }
        return None;
    }

    for dir in dirs {
        let candidate = PathBuf::from(dir).join(cmd);
        if exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn split_path_basic() {
        let dirs = split_path("/usr/bin:/usr/local/bin:/bin");
        assert_eq!(dirs, vec!["/usr/bin", "/usr/local/bin", "/bin"]);
    }

    #[test]
    fn split_path_empty_returns_one_empty() {
        // split on ':' of "" yields one empty element.
        let dirs = split_path("");
        assert_eq!(dirs, vec![""]);
    }

    #[test]
    fn split_path_with_empty_segments() {
        let dirs = split_path("/a::/b:");
        assert_eq!(dirs, vec!["/a", "", "/b", ""]);
    }

    #[test]
    fn find_command_returns_first_hit() {
        let dirs = vec!["/usr/bin", "/bin"];
        // Path comparison treats `/` and `\` as equivalent separators on Windows.
        let result = find_command("ls", &dirs, |p| {
            p == Path::new("/bin/ls") || p == Path::new("/usr/bin/ls")
        });
        // /usr/bin comes first in dirs.
        assert_eq!(result, Some(PathBuf::from("/usr/bin").join("ls")));
    }

    #[test]
    fn find_command_misses_when_absent() {
        let dirs = vec!["/usr/bin", "/bin"];
        let result = find_command("nonesuch", &dirs, |_| false);
        assert!(result.is_none());
    }

    #[test]
    fn find_command_with_slash_treated_literally_found() {
        let dirs = vec!["/usr/bin"];
        let result = find_command("./foo", &dirs, |p| p == Path::new("./foo"));
        assert_eq!(result, Some(PathBuf::from("./foo")));
    }

    #[test]
    fn find_command_with_slash_not_found() {
        let dirs = vec!["/usr/bin"];
        let result = find_command("./missing", &dirs, |_| false);
        assert!(result.is_none());
    }

    #[test]
    fn find_command_with_absolute_path() {
        let result = find_command("/sbin/init", &[], |p| p == Path::new("/sbin/init"));
        assert_eq!(result, Some(PathBuf::from("/sbin/init")));
    }

    #[test]
    fn find_command_skips_missing_dirs() {
        let dirs = vec!["/missing", "/also-missing", "/bin"];
        let result = find_command("sh", &dirs, |p| p == Path::new("/bin/sh"));
        assert_eq!(result, Some(PathBuf::from("/bin/sh")));
    }

    #[test]
    fn find_command_no_dirs_no_match() {
        let result = find_command("ls", &[], |_| true);
        assert!(result.is_none());
    }

    #[test]
    fn find_command_empty_dir_joins_to_name() {
        // Dir "" + "ls" -> "ls" relative path.
        let dirs = vec![""];
        let result = find_command("ls", &dirs, |p| p == Path::new("ls"));
        assert_eq!(result, Some(PathBuf::from("ls")));
    }
}
