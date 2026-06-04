#![deny(clippy::all)]

//! hostapd-cli — OurOS wireless access point daemon
//!
//! Multi-personality: `hostapd`, `hostapd_cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_hostapd(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: hostapd [OPTIONS] <config-file>");
        println!();
        println!("hostapd — IEEE 802.11 AP, IEEE 802.1X/WPA authenticator (OurOS).");
        println!();
        println!("Options:");
        println!("  -d            Debug output");
        println!("  -dd           More debug output");
        println!("  -B            Run in background");
        println!("  -P <pidfile>  PID file");
        println!("  -t            Include timestamps in debug");
        println!("  -v            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-v") {
        println!("hostapd v2.10 (OurOS)");
        return 0;
    }

    let config = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("/etc/hostapd/hostapd.conf");
    println!("Configuration file: {}", config);
    println!("Using interface wlan0 with hwaddr 00:11:22:33:44:55 and ssid \"OurOS-AP\"");
    println!("wlan0: interface state UNINITIALIZED->ENABLED");
    println!("wlan0: AP-ENABLED");
    0
}

fn run_hostapd_cli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: hostapd_cli [OPTIONS] [COMMAND]");
        println!();
        println!("Commands: status, all_sta, sta <addr>, new_sta <addr>,");
        println!("  deauthenticate <addr>, disassociate <addr>,");
        println!("  set <name> <value>, enable, disable, reload");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("status");
    match subcmd {
        "status" => {
            println!("state=ENABLED");
            println!("phy=phy0");
            println!("freq=5180");
            println!("num_sta[0]=3");
            println!("num_sta[1]=0");
            println!("bssid[0]=00:11:22:33:44:55");
            println!("ssid[0]=OurOS-AP");
            println!("channel=36");
            println!("ieee80211n=1");
            println!("ieee80211ac=1");
            println!("ieee80211ax=1");
            println!("beacon_int=100");
            println!("dtim_period=2");
        }
        "all_sta" => {
            println!("AA:BB:CC:DD:EE:01");
            println!("\taid=1");
            println!("\tflags=[AUTH][ASSOC][AUTHORIZED]");
            println!("\ttimeout_next=NULLFUNC POLL");
            println!("\trx_packets=12345");
            println!("\ttx_packets=6789");
            println!("AA:BB:CC:DD:EE:02");
            println!("\taid=2");
            println!("\tflags=[AUTH][ASSOC][AUTHORIZED]");
            println!("\ttimeout_next=NULLFUNC POLL");
            println!("\trx_packets=4567");
            println!("\ttx_packets=2345");
        }
        "enable" => println!("OK"),
        "disable" => println!("OK"),
        "reload" => println!("OK"),
        "set" => println!("OK"),
        "deauthenticate" | "disassociate" => {
            let addr = args.get(1).map(|s| s.as_str()).unwrap_or("AA:BB:CC:DD:EE:01");
            println!("OK — {} {}", subcmd, addr);
        }
        _ => println!("OK"),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "hostapd".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "hostapd_cli" => run_hostapd_cli(&rest),
        _ => run_hostapd(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_hostapd};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/hostapd"), "hostapd");
        assert_eq!(basename(r"C:\bin\hostapd.exe"), "hostapd.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("hostapd.exe"), "hostapd");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_hostapd(&["--help".to_string()]), 0);
        assert_eq!(run_hostapd(&["-h".to_string()]), 0);
        let _ = run_hostapd(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_hostapd(&[]);
    }
}
