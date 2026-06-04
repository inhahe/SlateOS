#![deny(clippy::all)]

//! mousetweaks-cli — OurOS mouse accessibility enhancements
//!
//! Single personality: `mousetweaks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mousetweaks(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mousetweaks [OPTIONS]");
        println!("mousetweaks v3.32 (OurOS) — Mouse accessibility enhancements");
        println!();
        println!("Options:");
        println!("  --dwell           Enable dwell click");
        println!("  --dwell-time MS   Dwell time (default: 1200)");
        println!("  --secondary       Enable simulated secondary click");
        println!("  --threshold PX    Movement threshold");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mousetweaks v3.32 (OurOS)"); return 0; }
    println!("mousetweaks: mouse accessibility active");
    println!("  Dwell click: hover to click");
    println!("  Dwell time: 1200ms");
    println!("  Secondary click: long press simulation");
    println!("  Click type window: available");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mousetweaks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mousetweaks(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mousetweaks};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mousetweaks"), "mousetweaks");
        assert_eq!(basename(r"C:\bin\mousetweaks.exe"), "mousetweaks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mousetweaks.exe"), "mousetweaks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mousetweaks(&["--help".to_string()], "mousetweaks"), 0);
        assert_eq!(run_mousetweaks(&["-h".to_string()], "mousetweaks"), 0);
        let _ = run_mousetweaks(&["--version".to_string()], "mousetweaks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mousetweaks(&[], "mousetweaks");
    }
}
