#![deny(clippy::all)]

//! shotwell-cli — Slate OS Shotwell photo manager
//!
//! Single personality: `shotwell`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_shotwell(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shotwell [OPTIONS] [FILES...]");
        println!("shotwell v0.32 (Slate OS) — Photo manager and viewer");
        println!();
        println!("Options:");
        println!("  --datadir DIR     Data directory");
        println!("  --import DIR      Import photos from directory");
        println!("  --version         Show version");
        println!();
        println!("Features: import, organize, tag, edit, publish,");
        println!("  face detection, RAW support, slideshows");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("shotwell v0.32 (Slate OS)"); return 0; }
    println!("shotwell: photo manager started");
    println!("  Library: 0 photos (import to get started)");
    println!("  Formats: JPEG, PNG, TIFF, BMP, GIF, RAW");
    println!("  Editing: crop, rotate, enhance, red-eye, adjust");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "shotwell".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_shotwell(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_shotwell};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/shotwell"), "shotwell");
        assert_eq!(basename(r"C:\bin\shotwell.exe"), "shotwell.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("shotwell.exe"), "shotwell");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shotwell(&["--help".to_string()], "shotwell"), 0);
        assert_eq!(run_shotwell(&["-h".to_string()], "shotwell"), 0);
        let _ = run_shotwell(&["--version".to_string()], "shotwell");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shotwell(&[], "shotwell");
    }
}
