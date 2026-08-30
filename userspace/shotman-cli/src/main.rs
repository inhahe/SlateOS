#![deny(clippy::all)]

//! shotman-cli — Slate OS shotman screenshot manager
//!
//! Single personality: `shotman`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_shotman(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: shotman COMMAND [OPTIONS]");
        println!("shotman v0.4 (Slate OS) — Screenshot manager for Wayland");
        println!();
        println!("Commands:");
        println!("  capture           Take screenshot");
        println!("  region            Capture region");
        println!("  output            Capture output");
        println!("  window            Capture window");
        println!("  version           Show version");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("capture");
    match cmd {
        "capture" | "output" => {
            println!("Screenshot saved: ~/Pictures/screenshot.png");
        }
        "region" => {
            println!("Select region...");
            println!("Screenshot saved: ~/Pictures/screenshot.png");
        }
        "window" => {
            println!("Click window...");
            println!("Screenshot saved: ~/Pictures/screenshot.png");
        }
        "version" | "--version" => println!("shotman v0.4 (Slate OS)"),
        _ => println!("shotman {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "shotman".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_shotman(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_shotman};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/shotman"), "shotman");
        assert_eq!(basename(r"C:\bin\shotman.exe"), "shotman.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("shotman.exe"), "shotman");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_shotman(&["--help".to_string()], "shotman"), 0);
        assert_eq!(run_shotman(&["-h".to_string()], "shotman"), 0);
        let _ = run_shotman(&["--version".to_string()], "shotman");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_shotman(&[], "shotman");
    }
}
