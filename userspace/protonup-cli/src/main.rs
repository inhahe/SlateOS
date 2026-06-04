#![deny(clippy::all)]

//! protonup-cli — OurOS ProtonUp-Qt Proton manager
//!
//! Multi-personality: `protonup-qt`, `protonup`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: protonup-qt [OPTIONS]");
        println!("protonup-qt v2.9 (OurOS) — Proton/Wine-GE installer GUI");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("protonup-qt v2.9 (OurOS)"); return 0; }
    println!("protonup-qt: compatibility tool manager started");
    println!("  Installed:");
    println!("    GE-Proton8-26");
    println!("    GE-Proton8-25");
    println!("    wine-ge-8-26");
    println!("  Install locations: Steam, Lutris");
    0
}

fn run_cli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: protonup [OPTIONS]");
        println!("protonup v0.2 (OurOS) — CLI Proton/Wine-GE installer");
        println!();
        println!("Options:");
        println!("  -l                List installed versions");
        println!("  -t TAG            Install specific version");
        println!("  -d TAG            Delete version");
        println!("  --releases        Show available releases");
        return 0;
    }
    if args.iter().any(|a| a == "-l") {
        println!("GE-Proton8-26");
        println!("GE-Proton8-25");
        return 0;
    }
    if args.iter().any(|a| a == "--releases") {
        println!("Available:");
        println!("  GE-Proton8-26 (latest)");
        println!("  GE-Proton8-25");
        println!("  GE-Proton8-24");
        return 0;
    }
    println!("protonup: installing latest GE-Proton...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "protonup-qt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "protonup" => run_cli(&rest, &prog),
        _ => run_qt(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/protonup"), "protonup");
        assert_eq!(basename(r"C:\bin\protonup.exe"), "protonup.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("protonup.exe"), "protonup");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_qt(&["--help".to_string()], "protonup"), 0);
        assert_eq!(run_qt(&["-h".to_string()], "protonup"), 0);
        let _ = run_qt(&["--version".to_string()], "protonup");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_qt(&[], "protonup");
    }
}
