#![deny(clippy::all)]

//! blueberry-cli — Slate OS Blueberry Bluetooth config tool (Cinnamon)
//!
//! Single personality: `blueberry`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_blueberry(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blueberry");
        println!("blueberry v1.4 (Slate OS) — Bluetooth configuration (Cinnamon)");
        println!();
        println!("Bluetooth device manager from Linux Mint / Cinnamon.");
        return 0;
    }
    let _ = args;
    println!("blueberry: Bluetooth settings");
    println!("  Bluetooth: ON");
    println!("  Visibility: ON (2 minutes)");
    println!("  Paired devices: 2");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "blueberry".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_blueberry(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_blueberry};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/blueberry"), "blueberry");
        assert_eq!(basename(r"C:\bin\blueberry.exe"), "blueberry.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("blueberry.exe"), "blueberry");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_blueberry(&["--help".to_string()], "blueberry"), 0);
        assert_eq!(run_blueberry(&["-h".to_string()], "blueberry"), 0);
        let _ = run_blueberry(&["--version".to_string()], "blueberry");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_blueberry(&[], "blueberry");
    }
}
