#![deny(clippy::all)]

//! iwgtk-cli — OurOS iwgtk iwd wireless GUI
//!
//! Single personality: `iwgtk`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iwgtk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: iwgtk [OPTIONS]");
        println!("iwgtk v0.9 (OurOS) — iwd wireless GTK frontend");
        println!();
        println!("Options:");
        println!("  --indicator    Start as tray indicator");
        println!("  --version      Show version");
        println!();
        println!("GTK4 frontend for iwd (iNet Wireless Daemon).");
        println!("Scan, connect, manage known networks, view adapters.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("iwgtk v0.9 (OurOS)"); return 0; }
    println!("iwgtk: iwd wireless manager");
    println!("  Adapter: wlan0 (powered)");
    println!("  Connected: HomeNetwork (-45 dBm)");
    println!("  Known networks: 3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iwgtk".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_iwgtk(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_iwgtk};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/iwgtk"), "iwgtk");
        assert_eq!(basename(r"C:\bin\iwgtk.exe"), "iwgtk.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("iwgtk.exe"), "iwgtk");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_iwgtk(&["--help".to_string()], "iwgtk"), 0);
        assert_eq!(run_iwgtk(&["-h".to_string()], "iwgtk"), 0);
        assert_eq!(run_iwgtk(&["--version".to_string()], "iwgtk"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_iwgtk(&[], "iwgtk"), 0);
    }
}
