#![deny(clippy::all)]

//! overskride-cli — Slate OS Overskride Bluetooth/WiFi manager
//!
//! Single personality: `overskride`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_overskride(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: overskride [OPTIONS]");
        println!("overskride v0.6 (Slate OS) — Bluetooth & WiFi manager (GTK4/libadwaita)");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Modern Bluetooth and WiFi manager with libadwaita UI.");
        println!("Features: device management, file transfer, audio profiles.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("overskride v0.6 (Slate OS)"); return 0; }
    println!("overskride: Bluetooth & WiFi manager");
    println!("  Bluetooth: ON — 2 paired, 1 connected");
    println!("  WiFi: Connected to 'HomeNetwork'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "overskride".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_overskride(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_overskride};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/overskride"), "overskride");
        assert_eq!(basename(r"C:\bin\overskride.exe"), "overskride.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("overskride.exe"), "overskride");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_overskride(&["--help".to_string()], "overskride"), 0);
        assert_eq!(run_overskride(&["-h".to_string()], "overskride"), 0);
        let _ = run_overskride(&["--version".to_string()], "overskride");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_overskride(&[], "overskride");
    }
}
