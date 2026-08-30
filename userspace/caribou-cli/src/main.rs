#![deny(clippy::all)]

//! caribou-cli — Slate OS Caribou on-screen keyboard
//!
//! Multi-personality: `caribou`, `caribou-preferences`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_caribou(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: caribou [OPTIONS]");
        println!("caribou v0.4 (Slate OS) — GNOME on-screen keyboard");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Auto-activates when text input fields receive focus.");
        println!("Integrates with AT-SPI accessibility framework.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("caribou v0.4 (Slate OS)"); return 0; }
    println!("caribou: on-screen keyboard daemon started");
    println!("  AT-SPI integration: active");
    println!("  Auto-show: on focus");
    0
}

fn run_preferences(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: caribou-preferences [OPTIONS]");
        println!("caribou-preferences v0.4 (Slate OS) — Caribou settings");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("caribou-preferences v0.4 (Slate OS)"); return 0; }
    println!("caribou-preferences: settings dialog opened");
    println!("  Keyboard layout: Full");
    println!("  Scanning: disabled");
    println!("  Key size: normal");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "caribou".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "caribou-preferences" => run_preferences(&rest, &prog),
        _ => run_caribou(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_caribou};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/caribou"), "caribou");
        assert_eq!(basename(r"C:\bin\caribou.exe"), "caribou.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("caribou.exe"), "caribou");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_caribou(&["--help".to_string()], "caribou"), 0);
        assert_eq!(run_caribou(&["-h".to_string()], "caribou"), 0);
        let _ = run_caribou(&["--version".to_string()], "caribou");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_caribou(&[], "caribou");
    }
}
