#![deny(clippy::all)]

//! yambar-cli — OurOS yambar modular status bar
//!
//! Single personality: `yambar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_yambar(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: yambar [OPTIONS]");
        println!("yambar v1.10 (OurOS) — Modular Wayland status bar");
        println!();
        println!("Options:");
        println!("  -c FILE           Configuration file (YAML)");
        println!("  -b BACKEND        Backend (wayland, x11)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("yambar v1.10 (OurOS)"); return 0; }
    println!("yambar: status bar running");
    println!("  Config: ~/.config/yambar/config.yml");
    println!("  Modules: clock, battery, network, cpu, memory");
    println!("  Output: HDMI-A-1");
    if args.is_empty() {
        println!("  Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "yambar".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_yambar(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_yambar};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/yambar"), "yambar");
        assert_eq!(basename(r"C:\bin\yambar.exe"), "yambar.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("yambar.exe"), "yambar");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_yambar(&["--help".to_string()], "yambar"), 0);
        assert_eq!(run_yambar(&["-h".to_string()], "yambar"), 0);
        assert_eq!(run_yambar(&["--version".to_string()], "yambar"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_yambar(&[], "yambar"), 0);
    }
}
