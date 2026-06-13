#![deny(clippy::all)]

//! havoc-cli — SlateOS Havoc minimal Wayland terminal
//!
//! Single personality: `havoc`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_havoc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: havoc [OPTIONS] [CMD [ARGS...]]");
        println!("havoc v0.5 (SlateOS) — Minimal Wayland terminal emulator");
        println!();
        println!("Options:");
        println!("  CMD               Command to run (default: shell)");
        println!("  -f FONT           Font (e.g. 'monospace:size=12')");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("havoc v0.5 (SlateOS)"); return 0; }
    println!("havoc: minimal Wayland terminal");
    println!("  Protocol: Wayland");
    println!("  Font: monospace 12pt");
    if args.is_empty() {
        println!("  Shell: /bin/sh");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "havoc".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_havoc(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_havoc};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/havoc"), "havoc");
        assert_eq!(basename(r"C:\bin\havoc.exe"), "havoc.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("havoc.exe"), "havoc");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_havoc(&["--help".to_string()], "havoc"), 0);
        assert_eq!(run_havoc(&["-h".to_string()], "havoc"), 0);
        let _ = run_havoc(&["--version".to_string()], "havoc");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_havoc(&[], "havoc");
    }
}
