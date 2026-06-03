#![deny(clippy::all)]

//! liquidctl-cli — OurOS liquid cooler control
//!
//! Single personality: `liquidctl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_liquidctl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: liquidctl COMMAND [OPTIONS]");
        println!("liquidctl v1.13 (OurOS) — Liquid cooler and RGB control");
        println!();
        println!("Commands:");
        println!("  list              List connected devices");
        println!("  initialize        Initialize device");
        println!("  status            Show device status");
        println!("  set               Set speed/color");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("liquidctl v1.13 (OurOS)"); return 0; }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match cmd {
        "list" => {
            println!("Device #0: NZXT Kraken X63");
            println!("  Bus: USB  Address: 1-4");
            println!("  Driver: KrakenX3");
            println!();
            println!("Device #1: Corsair Commander Pro");
            println!("  Bus: USB  Address: 1-7");
            println!("  Driver: CommanderPro");
        }
        "initialize" => {
            println!("Device #0: NZXT Kraken X63");
            println!("  Firmware: 1.0.7");
            println!("  Initialized successfully");
        }
        "status" => {
            println!("Device #0: NZXT Kraken X63");
            println!("  Liquid temperature:    32.4 \u{00b0}C");
            println!("  Pump speed:           2100 RPM");
            println!("  Pump duty:              60 %");
            println!("  Fan speed:            1050 RPM");
            println!("  Fan duty:               45 %");
        }
        "set" => {
            let target = args.get(1).map(|s| s.as_str()).unwrap_or("fan");
            println!("Device #0: set {} profile applied", target);
        }
        _ => println!("liquidctl: unknown command: {}", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "liquidctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_liquidctl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_liquidctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/liquidctl"), "liquidctl");
        assert_eq!(basename(r"C:\bin\liquidctl.exe"), "liquidctl.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("liquidctl.exe"), "liquidctl");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_liquidctl(&["--help".to_string()], "liquidctl"), 0);
        assert_eq!(run_liquidctl(&["-h".to_string()], "liquidctl"), 0);
        assert_eq!(run_liquidctl(&["--version".to_string()], "liquidctl"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_liquidctl(&[], "liquidctl"), 0);
    }
}
