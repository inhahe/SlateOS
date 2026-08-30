#![deny(clippy::all)]

//! font-manager-cli — Slate OS Font Manager
//!
//! Single personality: `font-manager`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_font_manager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: font-manager [OPTIONS]");
        println!("font-manager v0.9 (Slate OS) — Font management application");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Font preview, comparison, installation,");
        println!("  family browsing, character map, Google Fonts");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("font-manager v0.9 (Slate OS)"); return 0; }
    println!("font-manager: font management started");
    println!("  Installed: 142 font families");
    println!("  System: 98 families");
    println!("  User: 44 families");
    println!("  Categories: serif, sans-serif, monospace, display, handwriting");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "font-manager".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_font_manager(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_font_manager};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/font-manager"), "font-manager");
        assert_eq!(basename(r"C:\bin\font-manager.exe"), "font-manager.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("font-manager.exe"), "font-manager");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_font_manager(&["--help".to_string()], "font-manager"), 0);
        assert_eq!(run_font_manager(&["-h".to_string()], "font-manager"), 0);
        let _ = run_font_manager(&["--version".to_string()], "font-manager");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_font_manager(&[], "font-manager");
    }
}
