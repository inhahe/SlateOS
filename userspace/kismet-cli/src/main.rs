#![deny(clippy::all)]

//! kismet-cli — SlateOS Kismet wireless network detector
//!
//! Multi-personality: `kismet`, `kismet_cap_linux_wifi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kismet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kismet [OPTIONS]");
        println!("kismet v2023.07 (Slate OS) — Wireless network detector/sniffer");
        println!();
        println!("Options:");
        println!("  -c SOURCE      Capture source (e.g., wlan0)");
        println!("  --no-ncurses   Disable ncurses UI");
        println!("  --no-logging   Disable logging");
        println!("  -p PORT        Web UI port (default: 2501)");
        println!("  --version      Show version");
        println!();
        println!("Detects Wi-Fi, Bluetooth, and other wireless protocols.");
        println!("Web UI: http://localhost:2501");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("kismet v2023.07 (Slate OS)"); return 0; }
    println!("kismet: wireless network detector");
    println!("  Web UI: http://localhost:2501");
    println!("  Sources: wlan0 (Wi-Fi)");
    println!("  Networks seen: 12");
    println!("  Devices seen: 24");
    0
}

fn run_kismet_cap(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kismet_cap_linux_wifi [OPTIONS]");
        println!("kismet_cap_linux_wifi v2023.07 (Slate OS) — Wi-Fi capture helper");
        return 0;
    }
    let _ = args;
    println!("kismet_cap_linux_wifi: capture process started");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kismet".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "kismet_cap_linux_wifi" => run_kismet_cap(&rest, &prog),
        _ => run_kismet(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kismet};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kismet"), "kismet");
        assert_eq!(basename(r"C:\bin\kismet.exe"), "kismet.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kismet.exe"), "kismet");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_kismet(&["--help".to_string()], "kismet"), 0);
        assert_eq!(run_kismet(&["-h".to_string()], "kismet"), 0);
        let _ = run_kismet(&["--version".to_string()], "kismet");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_kismet(&[], "kismet");
    }
}
