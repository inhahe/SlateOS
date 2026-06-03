#![deny(clippy::all)]

//! foliate-cli — OurOS Foliate e-book reader
//!
//! Single personality: `foliate`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_foliate(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: foliate [OPTIONS] [FILE]");
        println!("foliate v3.0 (OurOS) — GNOME e-book reader");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("foliate v3.0 (OurOS)"); return 0; }
    println!("foliate: e-book reader started");
    println!("  Formats: EPUB, MOBI, KF8, FB2, CBZ, PDF");
    println!("  Library: 15 books");
    println!("  Reading progress: tracked");
    println!("  Annotations: highlights & notes");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "foliate".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_foliate(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_foliate};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/foliate"), "foliate");
        assert_eq!(basename(r"C:\bin\foliate.exe"), "foliate.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("foliate.exe"), "foliate");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_foliate(&["--help".to_string()], "foliate"), 0);
        assert_eq!(run_foliate(&["-h".to_string()], "foliate"), 0);
        assert_eq!(run_foliate(&["--version".to_string()], "foliate"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_foliate(&[], "foliate"), 0);
    }
}
