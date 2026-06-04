//! du — estimate file space usage.
//!
//! Usage: du [-h] [-s] [-a] [FILE...]
//!   -h  human-readable sizes
//!   -s  show only total for each argument
//!   -a  show sizes for all files, not just directories
//!   Default: show each directory's total recursively.
//!
//! Built only on unix-family targets (our x86_64-ouros presents as
//! linux-musl, so `cfg(unix)` matches).  On non-unix hosts (e.g.
//! Windows when running `cargo test --workspace`), a stub `main` keeps
//! the workspace compile-clean.  The pure parsing/formatting helpers
//! (`parse_args`, `human_size`, `format_line`) live outside the cfg gate
//! so they remain unit-testable on the developer host.

#![cfg_attr(not(unix), allow(dead_code))]

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct DuFlags {
    human: bool,
    summary: bool,
    show_all: bool,
}

/// Parse du's argv. Supports combined short options (e.g. `-hs`).
fn parse_args(args: &[String]) -> Result<(DuFlags, Vec<String>), String> {
    let mut flags = DuFlags::default();
    let mut paths: Vec<String> = Vec::new();

    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            for c in arg.chars().skip(1) {
                match c {
                    'h' => flags.human = true,
                    's' => flags.summary = true,
                    'a' => flags.show_all = true,
                    _ => return Err(format!("unknown option: -{c}")),
                }
            }
        } else {
            paths.push(arg.clone());
        }
    }

    Ok((flags, paths))
}

/// Format an IEC byte count like `1.5K`, `2.0M`, `3.0G`, or `512B`.
fn human_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}G", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}M", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}K", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}

/// Format a single output line: size, tab, path. In non-human mode, sizes
/// are reported as 1 KiB blocks (matching POSIX du).
fn format_line(bytes: u64, path: &str, human: bool) -> String {
    if human {
        format!("{}\t{path}", human_size(bytes))
    } else {
        format!("{}\t{path}", bytes / 1024)
    }
}

#[cfg(not(unix))]
fn main() {
    eprintln!("du: unix-only utility; not supported on this platform");
    std::process::exit(1);
}

#[cfg(unix)]
use std::env;
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process;

#[cfg(unix)]
fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let (flags, mut paths) = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("du: {e}");
            process::exit(1);
        }
    };

    if paths.is_empty() {
        paths.push(".".to_string());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut exit_code = 0;

    for path in &paths {
        let p = Path::new(path);
        match compute_du(p, &flags, &mut out) {
            Ok(total) => {
                let _ = writeln!(out, "{}", format_line(total, path, flags.human));
            }
            Err(e) => {
                eprintln!("du: {path}: {e}");
                exit_code = 1;
            }
        }
    }

    process::exit(exit_code);
}

#[cfg(unix)]
fn compute_du(path: &Path, flags: &DuFlags, out: &mut impl Write) -> Result<u64, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{e}"))?;

    if meta.is_file() {
        return Ok(meta.blocks().saturating_mul(512));
    }

    if !meta.is_dir() {
        return Ok(0);
    }

    let mut total: u64 = meta.blocks().saturating_mul(512);

    let entries = fs::read_dir(path).map_err(|e| format!("{e}"))?;
    for entry_result in entries {
        let entry = entry_result.map_err(|e| format!("{e}"))?;
        let entry_path = entry.path();
        let entry_meta = match fs::symlink_metadata(&entry_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("du: {}: {e}", entry_path.display());
                continue;
            }
        };

        if entry_meta.is_dir() {
            match compute_du(&entry_path, flags, out) {
                Ok(sub_total) => {
                    if !flags.summary {
                        let _ = writeln!(
                            out,
                            "{}",
                            format_line(sub_total, &entry_path.display().to_string(), flags.human)
                        );
                    }
                    total = total.saturating_add(sub_total);
                }
                Err(e) => {
                    eprintln!("du: {}: {e}", entry_path.display());
                }
            }
        } else {
            let size = entry_meta.blocks().saturating_mul(512);
            total = total.saturating_add(size);
            if flags.show_all && !flags.summary {
                let _ = writeln!(
                    out,
                    "{}",
                    format_line(size, &entry_path.display().to_string(), flags.human)
                );
            }
        }
    }

    Ok(total)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_no_args() {
        let (f, p) = parse_args(&s(&[])).unwrap();
        assert!(!f.human && !f.summary && !f.show_all);
        assert!(p.is_empty());
    }

    #[test]
    fn parse_paths_only() {
        let (f, p) = parse_args(&s(&["/etc", "/tmp"])).unwrap();
        assert!(!f.human && !f.summary && !f.show_all);
        assert_eq!(p, vec!["/etc", "/tmp"]);
    }

    #[test]
    fn parse_dash_h() {
        let (f, _) = parse_args(&s(&["-h"])).unwrap();
        assert!(f.human);
    }

    #[test]
    fn parse_dash_s() {
        let (f, _) = parse_args(&s(&["-s"])).unwrap();
        assert!(f.summary);
    }

    #[test]
    fn parse_dash_a() {
        let (f, _) = parse_args(&s(&["-a"])).unwrap();
        assert!(f.show_all);
    }

    #[test]
    fn parse_combined_hsa() {
        let (f, _) = parse_args(&s(&["-hsa"])).unwrap();
        assert!(f.human && f.summary && f.show_all);
    }

    #[test]
    fn parse_separate_flags() {
        let (f, p) = parse_args(&s(&["-h", "-s", "/etc"])).unwrap();
        assert!(f.human && f.summary);
        assert_eq!(p, vec!["/etc"]);
    }

    #[test]
    fn parse_unknown_short() {
        let err = parse_args(&s(&["-z"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_combined_with_unknown() {
        let err = parse_args(&s(&["-hsx"])).unwrap_err();
        assert!(err.contains("unknown option"));
    }

    #[test]
    fn parse_double_dash_treated_as_path() {
        // The implementation skips items starting with "--" through the
        // initial guard so they fall through to the path bucket.
        let (_, p) = parse_args(&s(&["--unknown", "/etc"])).unwrap();
        assert_eq!(p, vec!["--unknown", "/etc"]);
    }

    #[test]
    fn parse_dash_alone_as_path() {
        let (_, p) = parse_args(&s(&["-"])).unwrap();
        assert_eq!(p, vec!["-"]);
    }

    #[test]
    fn human_size_bytes() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(1023), "1023B");
    }

    #[test]
    fn human_size_kib() {
        assert_eq!(human_size(1024), "1.0K");
        assert_eq!(human_size(1536), "1.5K");
    }

    #[test]
    fn human_size_mib() {
        assert_eq!(human_size(1024 * 1024), "1.0M");
    }

    #[test]
    fn human_size_gib() {
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0G");
    }

    #[test]
    fn format_line_blocks() {
        assert_eq!(format_line(4096, "/etc", false), "4\t/etc");
    }

    #[test]
    fn format_line_human() {
        assert_eq!(format_line(4096, "/etc", true), "4.0K\t/etc");
    }

    #[test]
    fn format_line_under_kib_blocks() {
        // bytes / 1024 = 0 below 1 KiB.
        assert_eq!(format_line(512, "x", false), "0\tx");
    }

    #[test]
    fn format_line_under_kib_human() {
        assert_eq!(format_line(512, "x", true), "512B\tx");
    }
}
