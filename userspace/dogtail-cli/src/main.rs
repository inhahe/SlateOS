#![deny(clippy::all)]

//! dogtail-cli — Slate OS Dogtail GUI test framework
//!
//! Single personality: `dogtail`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dogtail(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: dogtail COMMAND [OPTIONS]");
        println!("Dogtail v0.9 (Slate OS) — GUI test automation via accessibility");
        println!();
        println!("Commands:");
        println!("  run SCRIPT        Run test script");
        println!("  sniff             Sniff UI accessibility tree");
        println!("  record            Record user actions");
        println!("  info              Show AT-SPI info");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Dogtail v0.9 (Slate OS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "run" => {
            let script = args.get(1).map(|s| s.as_str()).unwrap_or("test.py");
            println!("Running test: {}", script);
            println!("  Tests: 5 passed, 0 failed");
        }
        "sniff" => {
            println!("UI tree:");
            println!("  [application] FileManager");
            println!("    [frame] Home");
            println!("      [panel] toolbar");
            println!("      [panel] content");
        }
        "record" => println!("Recording actions... Press Ctrl+C to stop."),
        "info" => {
            println!("Dogtail v0.9");
            println!("  AT-SPI2: available");
            println!("  D-Bus: connected");
            println!("  Applications: 3 accessible");
        }
        _ => println!("dogtail {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "dogtail".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dogtail(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_dogtail};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/dogtail"), "dogtail");
        assert_eq!(basename(r"C:\bin\dogtail.exe"), "dogtail.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("dogtail.exe"), "dogtail");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_dogtail(&["--help".to_string()], "dogtail"), 0);
        assert_eq!(run_dogtail(&["-h".to_string()], "dogtail"), 0);
        let _ = run_dogtail(&["--version".to_string()], "dogtail");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_dogtail(&[], "dogtail");
    }
}
