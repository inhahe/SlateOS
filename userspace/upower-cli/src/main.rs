#![deny(clippy::all)]

//! upower-cli — Slate OS UPower power device enumeration
//!
//! Multi-personality: `upowerd`, `upower`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_upowerd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: upowerd [OPTIONS]");
        println!("upowerd v1.90 (Slate OS) — UPower system daemon");
        println!();
        println!("Options:");
        println!("  --replace         Replace running daemon");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("upowerd v1.90 (Slate OS)"); return 0; }
    println!("upowerd: power management daemon started");
    0
}

fn run_upower(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: upower [OPTIONS]");
        println!("upower v1.90 (Slate OS) — Query power devices");
        println!();
        println!("Options:");
        println!("  -e                Enumerate devices");
        println!("  -i DEVICE         Show device info");
        println!("  -d                Dump all info");
        println!("  -m                Monitor for changes");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("upower v1.90 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-e") {
        println!("/org/freedesktop/UPower/devices/line_power_AC");
        println!("/org/freedesktop/UPower/devices/battery_BAT0");
        println!("/org/freedesktop/UPower/devices/DisplayDevice");
        return 0;
    }
    if args.iter().any(|a| a == "-d") {
        println!("Device: /org/freedesktop/UPower/devices/battery_BAT0");
        println!("  native-path: BAT0");
        println!("  power supply: yes");
        println!("  type: battery");
        println!("  state: charging");
        println!("  percentage: 85%");
        println!("  energy: 43.5 Wh");
        println!("  energy-full: 51.2 Wh");
        println!("  time to full: 45 min");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "upower".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "upowerd" => run_upowerd(&rest, &prog),
        _ => run_upower(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_upowerd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/upower"), "upower");
        assert_eq!(basename(r"C:\bin\upower.exe"), "upower.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("upower.exe"), "upower");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_upowerd(&["--help".to_string()], "upower"), 0);
        assert_eq!(run_upowerd(&["-h".to_string()], "upower"), 0);
        let _ = run_upowerd(&["--version".to_string()], "upower");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_upowerd(&[], "upower");
    }
}
