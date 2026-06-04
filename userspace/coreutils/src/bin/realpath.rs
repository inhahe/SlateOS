//! realpath — print the resolved path.
//!
//! Usage: realpath FILE...
//!   Resolves all symlinks and relative components, prints the absolute path.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let paths = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("realpath: {e}");
            process::exit(1);
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let stderr = io::stderr();
    let mut err = stderr.lock();

    let exit_code = resolve_all(&paths, &mut out, &mut err);
    process::exit(exit_code);
}

/// Validate realpath's argv.  Currently realpath takes no flags; the
/// only failure mode is "no operands".
fn parse_args(args: &[String]) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err("missing operand".to_string());
    }
    Ok(args.to_vec())
}

/// Resolve each path with `fs::canonicalize`, writing resolved paths to
/// `out` and errors to `err`.  Returns 0 if all succeeded, 1 if any
/// failed.  Splitting this out lets tests exercise the success/error
/// fan-out without scraping stdio.
fn resolve_all(paths: &[String], out: &mut impl Write, err: &mut impl Write) -> i32 {
    let mut exit_code = 0;
    for path_str in paths {
        match fs::canonicalize(path_str) {
            Ok(p) => {
                let _ = writeln!(out, "{}", p.display());
            }
            Err(e) => {
                let _ = writeln!(err, "realpath: {path_str}: {e}");
                exit_code = 1;
            }
        }
    }
    exit_code
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    fn unique_temp_path(stem: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        env::temp_dir().join(format!("realpath_test_{stem}_{pid}_{ts}_{n}"))
    }

    // ---------------- parse_args ----------------

    #[test]
    fn parse_empty_errors() {
        let err = parse_args(&s(&[])).unwrap_err();
        assert!(err.contains("missing operand"));
    }

    #[test]
    fn parse_single_path() {
        let p = parse_args(&s(&["foo"])).unwrap();
        assert_eq!(p, vec!["foo"]);
    }

    #[test]
    fn parse_multiple_paths() {
        let p = parse_args(&s(&["a", "b", "c"])).unwrap();
        assert_eq!(p, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_dash_args_preserved_as_paths() {
        // No flag support yet — pass through.
        let p = parse_args(&s(&["-q", "foo"])).unwrap();
        assert_eq!(p, vec!["-q", "foo"]);
    }

    // ---------------- resolve_all ----------------

    #[test]
    fn resolve_existing_file_succeeds() {
        let p = unique_temp_path("ok");
        std::fs::write(&p, b"x").unwrap();
        let path_str = p.to_string_lossy().into_owned();

        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = resolve_all(&[path_str], &mut out, &mut err);
        assert_eq!(code, 0);
        assert!(!out.is_empty(), "canonical path should be printed");
        assert!(err.is_empty(), "no error expected: {err:?}");

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn resolve_missing_file_fails() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = resolve_all(
            &s(&["/this/does/not/exist/__nope__"]),
            &mut out,
            &mut err,
        );
        assert_eq!(code, 1);
        assert!(out.is_empty(), "nothing should be written to stdout");
        assert!(!err.is_empty(), "error message expected on stderr");
    }

    #[test]
    fn resolve_mixed_exits_one_but_prints_successes() {
        let p = unique_temp_path("mixed");
        std::fs::write(&p, b"x").unwrap();
        let path_str = p.to_string_lossy().into_owned();

        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = resolve_all(
            &[path_str, "/no/such/path/__missing__".to_string()],
            &mut out,
            &mut err,
        );
        assert_eq!(code, 1);
        assert!(!out.is_empty(), "good path still printed");
        assert!(!err.is_empty(), "bad path reports error");

        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn resolve_empty_input_is_success() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = resolve_all(&[], &mut out, &mut err);
        assert_eq!(code, 0);
        assert!(out.is_empty());
        assert!(err.is_empty());
    }
}
