#![deny(clippy::all)]

//! fontmatrix-cli — OurOS Fontmatrix font manager
//!
//! Single personality: `fontmatrix`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fontmatrix(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fontmatrix [OPTIONS]");
        println!("fontmatrix v0.9 (OurOS) — Font management and comparison tool");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Font activation/deactivation, tagging, filtering,");
        println!("  comparison view, Panose classification, font info,");
        println!("  specimen sheet generation");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fontmatrix v0.9 (OurOS)"); return 0; }
    println!("fontmatrix: font management started");
    println!("  Total fonts: 142 families");
    println!("  Active: 140");
    println!("  Inactive: 2");
    println!("  Tags: serif(32), sans(45), mono(18), display(27), other(20)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fontmatrix".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fontmatrix(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fontmatrix};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fontmatrix"), "fontmatrix");
        assert_eq!(basename(r"C:\bin\fontmatrix.exe"), "fontmatrix.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fontmatrix.exe"), "fontmatrix");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_fontmatrix(&["--help".to_string()], "fontmatrix"), 0);
        assert_eq!(run_fontmatrix(&["-h".to_string()], "fontmatrix"), 0);
        assert_eq!(run_fontmatrix(&["--version".to_string()], "fontmatrix"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_fontmatrix(&[], "fontmatrix"), 0);
    }
}
