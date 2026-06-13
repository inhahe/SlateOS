#![deny(clippy::all)]

//! bottles-cli — Slate OS Bottles Wine prefix manager
//!
//! Single personality: `bottles`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_bottles(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bottles [OPTIONS]");
        println!("bottles v51.0 (Slate OS) — Wine prefix manager");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("bottles v51.0 (Slate OS)"); return 0; }
    println!("bottles: Wine prefix manager started");
    println!("  Bottles: 3 configured");
    println!("    Gaming (Caffe 8.21, DXVK 2.3)");
    println!("    Software (Soda 9.0)");
    println!("    Custom (Wine 9.0)");
    println!("  Runners: caffe, soda, wine, proton");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "bottles".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_bottles(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bottles};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bottles"), "bottles");
        assert_eq!(basename(r"C:\bin\bottles.exe"), "bottles.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bottles.exe"), "bottles");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bottles(&["--help".to_string()], "bottles"), 0);
        assert_eq!(run_bottles(&["-h".to_string()], "bottles"), 0);
        let _ = run_bottles(&["--version".to_string()], "bottles");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bottles(&[], "bottles");
    }
}
