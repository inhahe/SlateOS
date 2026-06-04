//! touch -- create files or update their timestamps.
//!
//! Usage: touch FILE...
//!   Creates each FILE if it does not exist.
//!   Updates the modification timestamp if it does exist.

use std::env;
use std::fs::OpenOptions;
use std::io;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    let paths = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("touch: {e}");
            process::exit(1);
        }
    };

    let mut failed = false;
    for path in &paths {
        if let Err(e) = touch_one(path) {
            eprintln!("touch: cannot touch '{path}': {e}");
            failed = true;
        }
    }

    if failed {
        process::exit(1);
    }
}

/// Validate touch's argv.  Currently touch takes no flags; the only
/// failure mode is "no operands".  Returns the list of paths on success.
fn parse_args(args: &[String]) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err("missing operand".to_string());
    }
    Ok(args.to_vec())
}

/// Create `path` if it doesn't exist, otherwise bump its modification
/// time by setting its length to its current length (a portable trick
/// to update mtime without pulling in extra crates).
fn touch_one(path: &str) -> io::Result<()> {
    // write(true) (without truncate) lets us call `set_len` on Windows;
    // append-only handles can't be resized.  Without truncate(true), the
    // file's existing contents are preserved.
    let file = OpenOptions::new().create(true).write(true).truncate(false).open(path)?;
    let meta = file.metadata()?;
    let len = meta.len();
    file.set_len(len)?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    /// Unique temp path so tests can run in parallel without colliding.
    fn unique_temp_path(stem: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        env::temp_dir().join(format!("touch_test_{stem}_{pid}_{ts}_{n}"))
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
    fn parse_preserves_dash_args() {
        // touch doesn't grok flags yet — pass everything through as paths.
        let p = parse_args(&s(&["-a", "file"])).unwrap();
        assert_eq!(p, vec!["-a", "file"]);
    }

    // ---------------- touch_one ----------------

    #[test]
    fn touch_creates_missing_file() {
        let p = unique_temp_path("create");
        // Sanity: it shouldn't exist.
        let _ = fs::remove_file(&p);
        let path_str = p.to_string_lossy().into_owned();

        touch_one(&path_str).unwrap();
        assert!(p.exists(), "touch_one should have created {p:?}");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn touch_does_not_truncate_existing() {
        let p = unique_temp_path("preserve");
        let path_str = p.to_string_lossy().into_owned();
        fs::write(&p, b"hello").unwrap();

        touch_one(&path_str).unwrap();
        let contents = fs::read(&p).unwrap();
        assert_eq!(contents, b"hello", "touch must not modify existing data");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn touch_on_unwritable_directory_errors() {
        // Path inside a directory that definitely doesn't exist → error.
        let bad = "/this/does/not/exist/__touch_bad__";
        let err = touch_one(bad).unwrap_err();
        // Just check it's an IO error of some flavour; specific kind varies
        // by platform (NotFound on linux, other on Windows-style mounts).
        let _ = err.kind();
    }
}
