//! pwd -- print the current working directory.
//!
//! Usage: pwd
//!
//! There are essentially two behaviours: print the cwd to stdout, or
//! print an error to stderr and exit 1.  Both are wrapped in
//! `format_cwd_result` so unit tests can exercise them without touching
//! the actual cwd.

use std::env;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process;

fn main() {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let code = print_cwd(env::current_dir(), &mut out, &mut err);
    process::exit(code);
}

/// Render the result of `env::current_dir()` to the given writers.
/// Returns the exit code (0 success, 1 failure).
fn print_cwd(result: io::Result<PathBuf>, out: &mut impl Write, err: &mut impl Write) -> i32 {
    match result {
        Ok(path) => {
            let _ = writeln!(out, "{}", path.display());
            0
        }
        Err(e) => {
            let _ = writeln!(err, "pwd: {e}");
            1
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn print_cwd_success_writes_path_to_stdout() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = print_cwd(Ok(PathBuf::from("/some/cwd")), &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("/some/cwd"), "got {s:?}");
        assert!(s.ends_with('\n'));
        assert!(err.is_empty(), "no error expected: {err:?}");
    }

    #[test]
    fn print_cwd_failure_writes_to_stderr_and_returns_1() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = print_cwd(
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope")),
            &mut out,
            &mut err,
        );
        assert_eq!(code, 1);
        assert!(out.is_empty(), "stdout should be empty on error");
        let s = String::from_utf8(err).unwrap();
        assert!(s.starts_with("pwd: "), "got {s:?}");
        assert!(s.contains("nope"));
    }

    #[test]
    fn print_cwd_handles_path_with_spaces() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = print_cwd(Ok(PathBuf::from("/home/with space")), &mut out, &mut err);
        assert_eq!(code, 0);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("/home/with space"));
    }

    #[test]
    fn print_cwd_handles_relative_path() {
        // Whatever current_dir() actually returns is preserved verbatim.
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = print_cwd(Ok(PathBuf::from(".")), &mut out, &mut err);
        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(out).unwrap(), ".\n");
    }

    #[test]
    fn print_cwd_empty_path_still_prints_newline() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = print_cwd(Ok(PathBuf::new()), &mut out, &mut err);
        assert_eq!(code, 0);
        assert_eq!(String::from_utf8(out).unwrap(), "\n");
    }
}
