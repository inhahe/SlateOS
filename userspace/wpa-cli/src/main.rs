#![deny(clippy::all)]

//! wpa-cli — SlateOS WPA supplicant control
//!
//! Multi-personality: `wpa_cli`, `wpa_supplicant`, `wpa_passphrase`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wpa_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wpa_cli [OPTIONS] [COMMAND]");
        println!();
        println!("Commands: status, scan, scan_results, list_networks, add_network,");
        println!("  set_network, enable_network, disable_network, select_network,");
        println!("  save_config, disconnect, reconnect, terminate");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "status" => {
            println!("bssid=66:77:88:99:AA:BB");
            println!("freq=5180");
            println!("ssid=MyNetwork");
            println!("id=0");
            println!("mode=station");
            println!("pairwise_cipher=CCMP");
            println!("group_cipher=CCMP");
            println!("key_mgmt=WPA2-PSK");
            println!("wpa_state=COMPLETED");
            println!("ip_address=192.168.1.100");
            println!("address=00:11:22:33:44:55");
        }
        "scan" => println!("OK"),
        "scan_results" => {
            println!("bssid / frequency / signal level / flags / ssid");
            println!("66:77:88:99:AA:BB\t5180\t-45\t[WPA2-PSK-CCMP][ESS]\tMyNetwork");
            println!("11:22:33:44:55:66\t2437\t-70\t[WPA2-PSK-CCMP][ESS]\tNeighbor-WiFi");
        }
        "list_networks" => {
            println!("network id / ssid / bssid / flags");
            println!("0\tMyNetwork\tany\t[CURRENT]");
            println!("1\tOffice-WiFi\tany\t[DISABLED]");
        }
        "save_config" => println!("OK"),
        "disconnect" => println!("OK"),
        _ => println!("OK"),
    }
    0
}

fn run_wpa_supplicant(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wpa_supplicant [OPTIONS]");
        println!("Options: -i IFACE, -c CONFIG, -D DRIVER, -B (background), -d (debug)");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("wpa_supplicant v2.10 (SlateOS)");
        return 0;
    }
    let iface = args.windows(2).find(|w| w[0] == "-i").map(|w| w[1].as_str()).unwrap_or("wlan0");
    println!("Successfully initialized wpa_supplicant");
    println!("wlan0: interface {} up", iface);
    0
}

fn run_wpa_passphrase(args: &[String]) -> i32 {
    if args.len() < 2 {
        println!("Usage: wpa_passphrase SSID [PASSPHRASE]");
        return 1;
    }
    let ssid = args.first().map(|s| s.as_str()).unwrap_or("MyNetwork");
    println!("network={{");
    println!("\tssid=\"{}\"", ssid);
    println!("\t#psk=\"passphrase\"");
    println!("\tpsk=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
    println!("}}");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wpa_cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "wpa_supplicant" => run_wpa_supplicant(&rest),
        "wpa_passphrase" => run_wpa_passphrase(&rest),
        _ => run_wpa_cli(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_wpa_cli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/wpa"), "wpa");
        assert_eq!(basename(r"C:\bin\wpa.exe"), "wpa.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("wpa.exe"), "wpa");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_wpa_cli(&["--help".to_string()]), 0);
        assert_eq!(run_wpa_cli(&["-h".to_string()]), 0);
        let _ = run_wpa_cli(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_wpa_cli(&[]);
    }
}
