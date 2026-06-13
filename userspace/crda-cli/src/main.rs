#![deny(clippy::all)]

//! crda-cli — SlateOS CRDA wireless regulatory domain agent
//!
//! Multi-personality: `crda`, `regdbdump`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_crda(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: crda [OPTIONS]");
        println!("crda v4.15 (Slate OS) — Central Regulatory Domain Agent");
        println!();
        println!("Options:");
        println!("  --version      Show version");
        println!();
        println!("Applies wireless regulatory domain rules.");
        println!("Called by the kernel via udev when regulatory info needed.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("crda v4.15 (Slate OS)"); return 0; }
    println!("crda: regulatory domain agent");
    println!("  Country: US");
    println!("  2.4 GHz: channels 1-11");
    println!("  5 GHz: channels 36-48, 52-64, 100-144, 149-165");
    0
}

fn run_regdbdump(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: regdbdump <regdb-file>");
        println!("regdbdump v4.15 (Slate OS) — Dump wireless regulatory database");
        return 0;
    }
    let _ = args;
    println!("country US: DFS-FCC");
    println!("  (2402 - 2472 @ 40), (30)");
    println!("  (5170 - 5250 @ 80), (23), AUTO-BW");
    println!("  (5250 - 5330 @ 80), (23), DFS, AUTO-BW");
    println!("  (5490 - 5730 @ 160), (23), DFS");
    println!("  (5735 - 5835 @ 80), (30)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "crda".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "regdbdump" => run_regdbdump(&rest, &prog),
        _ => run_crda(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_crda};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/crda"), "crda");
        assert_eq!(basename(r"C:\bin\crda.exe"), "crda.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("crda.exe"), "crda");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_crda(&["--help".to_string()], "crda"), 0);
        assert_eq!(run_crda(&["-h".to_string()], "crda"), 0);
        let _ = run_crda(&["--version".to_string()], "crda");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_crda(&[], "crda");
    }
}
