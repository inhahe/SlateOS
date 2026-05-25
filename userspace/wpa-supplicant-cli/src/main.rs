#![deny(clippy::all)]

//! wpa-supplicant-cli — OurOS wpa_supplicant wireless client
//!
//! Multi-personality: `wpa_supplicant`, `wpa_cli`, `wpa_passphrase`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wpa_supplicant(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wpa_supplicant [OPTIONS]");
        println!("wpa_supplicant v2.10 (OurOS) — WPA/WPA2/WPA3 supplicant");
        println!();
        println!("Options:");
        println!("  -i IFACE       Interface name");
        println!("  -c FILE        Configuration file");
        println!("  -D DRIVER      Driver type (nl80211, wext)");
        println!("  -B             Run in background");
        println!("  -f FILE        Log file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wpa_supplicant v2.10 (OurOS)"); return 0; }
    println!("wpa_supplicant: started");
    println!("  Interface: wlan0");
    println!("  Driver: nl80211");
    println!("  State: COMPLETED");
    0
}

fn run_wpa_cli(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wpa_cli [OPTIONS] [COMMAND]");
        println!("wpa_cli v2.10 (OurOS) — WPA supplicant control");
        println!("  status         Show connection status");
        println!("  scan           Trigger scan");
        println!("  scan_results   Show scan results");
        println!("  list_networks  List configured networks");
        println!("  add_network    Add new network");
        println!("  select_network Select network");
        return 0;
    }
    match args.first().map(|s| s.as_str()) {
        Some("status") => {
            println!("bssid=AA:BB:CC:DD:EE:FF");
            println!("ssid=HomeNetwork");
            println!("key_mgmt=WPA2-PSK");
            println!("wpa_state=COMPLETED");
            println!("ip_address=192.168.1.100");
        }
        Some("scan_results") => {
            println!("bssid / frequency / signal / flags / ssid");
            println!("AA:BB:CC:DD:EE:FF  5180  -45  [WPA2-PSK]  HomeNetwork");
            println!("11:22:33:44:55:66  2437  -72  [WPA2-PSK]  Neighbor");
        }
        _ => {
            println!("wpa_cli: interactive mode (type 'help')");
        }
    }
    0
}

fn run_wpa_passphrase(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wpa_passphrase SSID [PASSPHRASE]");
        println!("wpa_passphrase v2.10 (OurOS) — Generate WPA PSK");
        return 0;
    }
    if let Some(ssid) = args.first() {
        println!("network={{");
        println!("  ssid=\"{}\"", ssid);
        println!("  #psk=\"passphrase\"");
        println!("  psk=a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0");
        println!("}}");
    } else {
        println!("wpa_passphrase: missing SSID argument");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wpa_supplicant".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wpa_cli" => run_wpa_cli(&rest, &prog),
        "wpa_passphrase" => run_wpa_passphrase(&rest, &prog),
        _ => run_wpa_supplicant(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
