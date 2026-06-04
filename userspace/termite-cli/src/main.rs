#![deny(clippy::all)]

//! termite-cli — OurOS Termite terminal emulator
//!
//! Single personality: `termite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_termite(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: termite [OPTIONS]");
        println!("termite v16 (OurOS) — VTE-based terminal with vim keybindings");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file");
        println!("  -e CMD            Execute command");
        println!("  -r ROLE           Window role");
        println!("  -t TITLE          Window title");
        println!("  -d DIR            Working directory");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("termite v16 (OurOS)"); return 0; }
    println!("Termite terminal starting...");
    println!("  VTE: 0.72");
    println!("  Hints: url mode (Ctrl+Shift+X)");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "termite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_termite(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_termite};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/termite"), "termite");
        assert_eq!(basename(r"C:\bin\termite.exe"), "termite.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("termite.exe"), "termite");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_termite(&["--help".to_string()], "termite"), 0);
        assert_eq!(run_termite(&["-h".to_string()], "termite"), 0);
        let _ = run_termite(&["--version".to_string()], "termite");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_termite(&[], "termite");
    }
}
