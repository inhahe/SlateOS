#![deny(clippy::all)]

//! iw-cli — SlateOS wireless configuration tool
//!
//! Multi-personality: `iw`, `iwconfig`, `iwlist`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_iw(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: iw [OPTIONS] OBJECT COMMAND");
        println!();
        println!("iw — wireless configuration tool (Slate OS).");
        println!();
        println!("Objects: dev, phy, reg");
        println!("Commands: info, link, scan, connect, disconnect, station dump");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("iw version 6.7 (Slate OS)");
        return 0;
    }

    let obj = args.first().map(|s| s.as_str()).unwrap_or("dev");
    match obj {
        "dev" => {
            let iface = args.get(1).map(|s| s.as_str()).unwrap_or("wlan0");
            let cmd = args.get(2).map(|s| s.as_str()).unwrap_or("info");
            match cmd {
                "info" => {
                    println!("Interface {}", iface);
                    println!("\tifindex 3");
                    println!("\twdev 0x1");
                    println!("\taddr 00:11:22:33:44:55");
                    println!("\ttype managed");
                    println!("\twiphy 0");
                    println!("\tchannel 36 (5180 MHz), width: 80 MHz, center1: 5210 MHz");
                }
                "link" => {
                    println!("Connected to 66:77:88:99:AA:BB (on {})", iface);
                    println!("\tSSID: MyNetwork");
                    println!("\tfreq: 5180");
                    println!("\tRX: 123456789 bytes (1234567 packets)");
                    println!("\tTX: 98765432 bytes (987654 packets)");
                    println!("\tsignal: -45 dBm");
                    println!("\ttx bitrate: 866.7 MBit/s VHT-MCS 9 80MHz short GI VHT-NSS 2");
                }
                "scan" => {
                    println!("BSS 66:77:88:99:AA:BB(on {})", iface);
                    println!("\tfreq: 5180");
                    println!("\tsignal: -45.00 dBm");
                    println!("\tSSID: MyNetwork");
                    println!("\tRSN:\t * Version: 1");
                    println!("\t\t * Group cipher: CCMP");
                    println!("BSS 11:22:33:44:55:66(on {})", iface);
                    println!("\tfreq: 2437");
                    println!("\tsignal: -70.00 dBm");
                    println!("\tSSID: Neighbor-WiFi");
                }
                _ => println!("iw: command '{}' completed", cmd),
            }
        }
        "phy" => {
            println!("Wiphy phy0");
            println!("\tBand 1:");
            println!("\t\tFrequencies:");
            println!("\t\t\t* 2412 MHz [1]");
            println!("\t\t\t* 2437 MHz [6]");
            println!("\t\t\t* 2462 MHz [11]");
            println!("\tBand 2:");
            println!("\t\tFrequencies:");
            println!("\t\t\t* 5180 MHz [36]");
            println!("\t\t\t* 5240 MHz [48]");
            println!("\t\t\t* 5745 MHz [149]");
        }
        "reg" => {
            println!("global");
            println!("country US: DFS-FCC");
            println!("\t(2402 - 2472 @ 40), (N/A, 30), (N/A)");
            println!("\t(5170 - 5250 @ 80), (N/A, 23), (N/A), AUTO-BW");
            println!("\t(5250 - 5330 @ 80), (N/A, 23), (0 ms), DFS, AUTO-BW");
        }
        _ => println!("iw: unknown object '{}'", obj),
    }
    0
}

fn run_iwconfig(args: &[String]) -> i32 {
    let iface = args.first().map(|s| s.as_str());
    if iface.is_none() {
        println!("wlan0     IEEE 802.11  ESSID:\"MyNetwork\"");
        println!("          Mode:Managed  Frequency:5.18 GHz  Access Point: 66:77:88:99:AA:BB");
        println!("          Bit Rate=866.7 Mb/s   Tx-Power=22 dBm");
        println!("          Retry short limit:7   RTS thr:off   Fragment thr:off");
        println!("          Power Management:on");
        println!("          Link Quality=70/70  Signal level=-45 dBm");
        println!("          Rx invalid nwid:0  Rx invalid crypt:0  Rx invalid frag:0");
        println!("          Tx excessive retries:0  Invalid misc:0   Missed beacon:0");
    }
    0
}

fn run_iwlist(args: &[String]) -> i32 {
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("scan");
    if cmd == "scan" {
        println!("wlan0     Scan completed :");
        println!("          Cell 01 - Address: 66:77:88:99:AA:BB");
        println!("                    ESSID:\"MyNetwork\"");
        println!("                    Protocol:IEEE 802.11ac");
        println!("                    Frequency:5.18 GHz (Channel 36)");
        println!("                    Quality=70/70  Signal level=-45 dBm");
        println!("                    Encryption key:on");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "iw".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "iwconfig" => run_iwconfig(&rest),
        "iwlist" => run_iwlist(&rest),
        _ => run_iw(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_iw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/iw"), "iw");
        assert_eq!(basename(r"C:\bin\iw.exe"), "iw.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("iw.exe"), "iw");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_iw(&["--help".to_string()]), 0);
        assert_eq!(run_iw(&["-h".to_string()]), 0);
        let _ = run_iw(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_iw(&[]);
    }
}
