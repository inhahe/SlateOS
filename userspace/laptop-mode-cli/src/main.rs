#![deny(clippy::all)]

//! laptop-mode-cli — SlateOS Laptop Mode Tools power saving
//!
//! Single personality: `laptop-mode`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_laptop_mode(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: laptop-mode <command> [OPTIONS]");
        println!("laptop-mode v1.74 (SlateOS) — Laptop power saving");
        println!();
        println!("Commands:");
        println!("  status         Show current mode and module status");
        println!("  start          Start laptop mode");
        println!("  stop           Stop laptop mode");
        println!("  force          Force battery mode");
        println!("  auto           Auto-detect power source");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("laptop-mode v1.74 (SlateOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("status") => {
            println!("Laptop Mode Tools status:");
            println!("  Power source: AC");
            println!("  Laptop mode: disabled (on AC)");
            println!("  Modules enabled: intel-sata-powermgmt, cpufreq, wireless");
            println!("  Modules disabled: bluetooth (manually), nmi-watchdog");
        }
        _ => {
            println!("laptop-mode: power management tool");
            println!("  Use 'laptop-mode status' for current state");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "laptop-mode".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_laptop_mode(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_laptop_mode};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/laptop-mode"), "laptop-mode");
        assert_eq!(basename(r"C:\bin\laptop-mode.exe"), "laptop-mode.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("laptop-mode.exe"), "laptop-mode");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_laptop_mode(&["--help".to_string()], "laptop-mode"), 0);
        assert_eq!(run_laptop_mode(&["-h".to_string()], "laptop-mode"), 0);
        let _ = run_laptop_mode(&["--version".to_string()], "laptop-mode");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_laptop_mode(&[], "laptop-mode");
    }
}
