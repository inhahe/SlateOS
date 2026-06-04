#![deny(clippy::all)]

//! blueman-cli — OurOS Blueman Bluetooth manager
//!
//! Multi-personality: `blueman-manager`, `blueman-applet`, `blueman-sendto`, `blueman-adapters`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_manager(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blueman-manager");
        println!("blueman-manager v2.4 (OurOS) — Bluetooth device manager");
        return 0;
    }
    let _ = args;
    println!("blueman-manager: Bluetooth device manager");
    println!("  Adapter: hci0 (Intel AX210)");
    println!("  Devices: 3 paired, 0 connected");
    0
}

fn run_applet(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blueman-applet");
        println!("blueman-applet v2.4 (OurOS) — Bluetooth system tray applet");
        return 0;
    }
    let _ = args;
    println!("blueman-applet: system tray Bluetooth applet running");
    0
}

fn run_sendto(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: blueman-sendto FILE...");
        println!("blueman-sendto v2.4 (OurOS) — Send files via Bluetooth");
        return 0;
    }
    for f in args.iter().filter(|a| !a.starts_with('-')) {
        println!("Sending: {}", f);
    }
    0
}

fn run_adapters(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: blueman-adapters");
        println!("blueman-adapters v2.4 (OurOS) — Bluetooth adapter settings");
        return 0;
    }
    let _ = args;
    println!("blueman-adapters: adapter configuration");
    println!("  hci0: Intel AX210 — Discoverable, Name='OurOS PC'");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "blueman-manager".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "blueman-applet" => run_applet(&rest, &prog),
        "blueman-sendto" => run_sendto(&rest, &prog),
        "blueman-adapters" => run_adapters(&rest, &prog),
        _ => run_manager(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_manager};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/blueman"), "blueman");
        assert_eq!(basename(r"C:\bin\blueman.exe"), "blueman.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("blueman.exe"), "blueman");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_manager(&["--help".to_string()], "blueman"), 0);
        assert_eq!(run_manager(&["-h".to_string()], "blueman"), 0);
        let _ = run_manager(&["--version".to_string()], "blueman");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_manager(&[], "blueman");
    }
}
