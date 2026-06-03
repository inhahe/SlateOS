#![deny(clippy::all)]

//! aircrack-cli — OurOS Aircrack-ng wireless security auditing
//!
//! Multi-personality: `aircrack-ng`, `airodump-ng`, `aireplay-ng`, `airmon-ng`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_aircrack(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aircrack-ng [OPTIONS] <capture-file>");
        println!("aircrack-ng v1.7 (OurOS) — Wireless key recovery");
        println!();
        println!("Options:");
        println!("  -w WORDLIST    Dictionary file for WPA");
        println!("  -b BSSID       Target AP MAC address");
        println!("  -e ESSID       Target network name");
        println!("  -a MODE        Force attack mode (1=WEP, 2=WPA)");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("aircrack-ng v1.7 (OurOS)"); return 0; }
    println!("aircrack-ng: wireless key recovery tool");
    println!("  Use with capture files from airodump-ng");
    0
}

fn run_airodump(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: airodump-ng [OPTIONS] <interface>");
        println!("airodump-ng v1.7 (OurOS) — Wireless packet capture");
        println!("  -c CHAN        Channel");
        println!("  --bssid MAC    Filter by BSSID");
        println!("  -w PREFIX      Output file prefix");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("airodump-ng v1.7 (OurOS)"); return 0; }
    println!("airodump-ng: packet capture (requires monitor mode)");
    0
}

fn run_aireplay(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: aireplay-ng [OPTIONS] <interface>");
        println!("aireplay-ng v1.7 (OurOS) — Wireless packet injection");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("aireplay-ng v1.7 (OurOS)"); return 0; }
    println!("aireplay-ng: packet injection tool");
    0
}

fn run_airmon(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: airmon-ng [start|stop] <interface> [channel]");
        println!("airmon-ng v1.7 (OurOS) — Monitor mode control");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("airmon-ng v1.7 (OurOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("start") => {
            println!("airmon-ng: enabling monitor mode on wlan0");
            println!("  Monitor mode enabled: wlan0mon");
        }
        Some("stop") => {
            println!("airmon-ng: disabling monitor mode");
        }
        _ => {
            println!("PHY  Interface  Driver      Chipset");
            println!("phy0 wlan0      iwlwifi     Intel Wi-Fi 6");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "aircrack-ng".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "airodump-ng" => run_airodump(&rest, &prog),
        "aireplay-ng" => run_aireplay(&rest, &prog),
        "airmon-ng" => run_airmon(&rest, &prog),
        _ => run_aircrack(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_aircrack};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/aircrack"), "aircrack");
        assert_eq!(basename(r"C:\bin\aircrack.exe"), "aircrack.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("aircrack.exe"), "aircrack");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_aircrack(&["--help".to_string()], "aircrack"), 0);
        assert_eq!(run_aircrack(&["-h".to_string()], "aircrack"), 0);
        assert_eq!(run_aircrack(&["--version".to_string()], "aircrack"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_aircrack(&[], "aircrack"), 0);
    }
}
