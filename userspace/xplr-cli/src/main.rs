#![deny(clippy::all)]

//! xplr-cli — Slate OS xplr file explorer
//!
//! Single personality: `xplr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_xplr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: xplr [OPTIONS] [PATH]...");
        println!("xplr 0.21.9 (Slate OS) — Hackable, minimal, fast TUI file explorer");
        println!();
        println!("Options:");
        println!("  --config FILE        Config file");
        println!("  --extra-config FILE  Extra config file");
        println!("  --on-load CMD        Command on load");
        println!("  --pipe-msg-in        Read messages from stdin");
        println!("  --print-pwd-as-result Print pwd as result");
        println!("  --read-only          Read-only mode");
        println!("  --vroot PATH         Virtual root");
        println!("  -V, --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-V" || a == "--version") {
        println!("xplr 0.21.9 (Slate OS)");
        return 0;
    }
    if args.iter().any(|a| a == "--read-only") {
        println!("xplr: Read-only mode enabled");
    }
    let path = args.iter().rfind(|a| !a.starts_with('-'))
        .map(|s| s.as_str()).unwrap_or(".");
    println!("xplr: Exploring '{}'", path);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "xplr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_xplr(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_xplr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/xplr"), "xplr");
        assert_eq!(basename(r"C:\bin\xplr.exe"), "xplr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("xplr.exe"), "xplr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_xplr(&["--help".to_string()], "xplr"), 0);
        assert_eq!(run_xplr(&["-h".to_string()], "xplr"), 0);
        let _ = run_xplr(&["--version".to_string()], "xplr");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_xplr(&[], "xplr");
    }
}
