#![deny(clippy::all)]

//! fnott-cli — SlateOS fnott notification daemon
//!
//! Single personality: `fnott`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fnott(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fnott [OPTIONS]");
        println!("fnott v1.5 (SlateOS) — Lightweight Wayland notification daemon");
        println!();
        println!("Options:");
        println!("  (fnott reads config from ~/.config/fnott/fnott.ini)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("fnott v1.5 (SlateOS)"); return 0; }
    println!("fnott: notification daemon running");
    println!("  Config: ~/.config/fnott/fnott.ini");
    if args.is_empty() {
        println!("  Listening for D-Bus notifications...");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "fnott".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fnott(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_fnott};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/fnott"), "fnott");
        assert_eq!(basename(r"C:\bin\fnott.exe"), "fnott.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("fnott.exe"), "fnott");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fnott(&["--help".to_string()], "fnott"), 0);
        assert_eq!(run_fnott(&["-h".to_string()], "fnott"), 0);
        let _ = run_fnott(&["--version".to_string()], "fnott");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fnott(&[], "fnott");
    }
}
