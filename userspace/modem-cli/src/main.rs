#![deny(clippy::all)]

//! modem-cli — OurOS modem/mobile broadband tools
//!
//! Multi-personality: `mmcli`, `nmcli` (ModemManager / NetworkManager)

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mmcli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mmcli [OPTIONS]");
        println!();
        println!("mmcli — ModemManager CLI (OurOS).");
        println!();
        println!("Options:");
        println!("  -L, --list-modems     List available modems");
        println!("  -m, --modem=N         Specify modem");
        println!("  -w, --monitor-modems  Monitor modems");
        println!("  --simple-status       Show simple status");
        println!("  --signal-get          Get signal quality");
        println!("  --location-get        Get location info");
        println!("  --messaging-list-sms  List SMS messages");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("mmcli 1.22 (OurOS)");
        return 0;
    }

    if args.iter().any(|a| a == "-L" || a == "--list-modems") {
        println!("Found 1 modems:");
        println!("  /org/freedesktop/ModemManager1/Modem/0 [Quectel] EM05-G");
        return 0;
    }

    if args.iter().any(|a| a == "--simple-status") {
        println!("  ----------------------------");
        println!("  Status |   state: connected");
        println!("         |   power state: on");
        println!("         |   signal quality: 78% (recent)");
        println!("         |   access tech: lte");
        println!("  ----------------------------");
        println!("  3GPP   |   operator name: OurOS Mobile");
        println!("         |   registration: home");
        return 0;
    }

    if args.iter().any(|a| a == "--signal-get") {
        println!("  -----------");
        println!("  LTE   | rssi: -67.00 dBm");
        println!("        | rsrp: -95.00 dBm");
        println!("        | rsrq: -10.00 dB");
        println!("        | snr: 12.50 dB");
        return 0;
    }

    if args.iter().any(|a| a == "--location-get") {
        println!("  -----------------");
        println!("  3GPP |   mcc: 310");
        println!("       |   mnc: 260");
        println!("       |   lac: 0x1234");
        println!("       |   cid: 0x5678ABCD");
        println!("       |   tac: 0x1234");
        println!("  GPS  |   utc: 2024-01-15T10:30:00Z");
        println!("       |   lat: 37.7749");
        println!("       |   long: -122.4194");
        return 0;
    }

    // Default: show modem details
    println!("  --------------------------------");
    println!("  General  | path: /org/freedesktop/ModemManager1/Modem/0");
    println!("           | device id: abcdef1234567890");
    println!("  --------------------------------");
    println!("  Hardware | manufacturer: Quectel");
    println!("           | model: EM05-G");
    println!("           | firmware: EM05GLAR07A07M1G");
    println!("  --------------------------------");
    println!("  System   | device: /sys/devices/pci0000:00/0000:00:14.0/usb1");
    println!("           | drivers: qmi_wwan, option");
    println!("  --------------------------------");
    println!("  Status   | state: connected");
    println!("           | power state: on");
    println!("           | signal quality: 78% (recent)");
    println!("  --------------------------------");
    println!("  Modes    | supported: allowed: 4g; preferred: none");
    println!("           | current: allowed: 4g; preferred: none");
    println!("  --------------------------------");
    println!("  Bands    | supported: eUtran-1, eUtran-3, eUtran-7, eUtran-20");
    println!("           | current: eUtran-3");
    println!("  --------------------------------");
    println!("  SIM      | path: /org/freedesktop/ModemManager1/SIM/0");
    0
}

fn run_nmcli(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: nmcli [OPTIONS] OBJECT COMMAND");
        println!();
        println!("nmcli — NetworkManager CLI (OurOS).");
        println!();
        println!("Objects:");
        println!("  general      NetworkManager status");
        println!("  networking   Overall networking control");
        println!("  connection   Manage connections");
        println!("  device       Manage devices");
        println!("  radio        Manage radio switches");
        println!("  monitor      Monitor changes");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("nmcli tool, version 1.44.0 (OurOS)");
        return 0;
    }

    let obj = args.first().map(|s| s.as_str()).unwrap_or("general");
    match obj {
        "general" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match cmd {
                "status" => {
                    println!("STATE      CONNECTIVITY  WIFI-HW  WIFI     WWAN-HW  WWAN");
                    println!("connected  full          enabled  enabled  enabled  enabled");
                }
                "hostname" => println!("ouros-desktop"),
                _ => println!("nmcli: general command '{}' completed", cmd),
            }
        }
        "connection" | "con" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");
            match cmd {
                "show" => {
                    println!("NAME        UUID                                  TYPE      DEVICE");
                    println!("Wired       a1b2c3d4-e5f6-7890-abcd-ef1234567890  ethernet  eth0");
                    println!("MyNetwork   11223344-5566-7788-99aa-bbccddeeff00  wifi      wlan0");
                    println!("VPN-Work    aabbccdd-eeff-0011-2233-445566778899  vpn       --");
                }
                "up" | "down" => {
                    let name = args.get(2).map(|s| s.as_str()).unwrap_or("Wired");
                    println!("Connection successfully {}.", if cmd == "up" { "activated" } else { "deactivated" });
                    println!("(D-Bus path: /org/freedesktop/NetworkManager/ActiveConnection/{})", name.len());
                }
                _ => println!("nmcli: connection command '{}' completed", cmd),
            }
        }
        "device" | "dev" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("status");
            match cmd {
                "status" => {
                    println!("DEVICE   TYPE      STATE         CONNECTION");
                    println!("eth0     ethernet  connected     Wired");
                    println!("wlan0    wifi      connected     MyNetwork");
                    println!("wwan0    gsm       connected     Mobile");
                    println!("lo       loopback  unmanaged     --");
                }
                "wifi" => {
                    let subcmd = args.get(2).map(|s| s.as_str()).unwrap_or("list");
                    if subcmd == "list" {
                        println!("IN-USE  BSSID              SSID          MODE   CHAN  RATE        SIGNAL  BARS  SECURITY");
                        println!("*       66:77:88:99:AA:BB  MyNetwork     Infra  36    540 Mbit/s  92      ▂▄▆█  WPA2");
                        println!("        11:22:33:44:55:66  Neighbor      Infra  6     270 Mbit/s  45      ▂▄__  WPA2");
                        println!("        AA:BB:CC:DD:EE:FF  CoffeeShop    Infra  1     54 Mbit/s   30      ▂___  WPA2");
                    } else {
                        println!("nmcli: wifi {} completed", subcmd);
                    }
                }
                _ => println!("nmcli: device command '{}' completed", cmd),
            }
        }
        "radio" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("all");
            if cmd == "all" {
                println!("WIFI-HW  WIFI     WWAN-HW  WWAN");
                println!("enabled  enabled  enabled  enabled");
            } else {
                println!("enabled");
            }
        }
        "networking" => {
            let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("connectivity");
            match cmd {
                "on" | "off" => println!("Networking {}.", cmd),
                "connectivity" => println!("full"),
                _ => println!("nmcli: networking command '{}' completed", cmd),
            }
        }
        _ => println!("nmcli: unknown object '{}'", obj),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mmcli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "nmcli" => run_nmcli(&rest),
        _ => run_mmcli(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mmcli};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/modem"), "modem");
        assert_eq!(basename(r"C:\bin\modem.exe"), "modem.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("modem.exe"), "modem");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mmcli(&["--help".to_string()]), 0);
        assert_eq!(run_mmcli(&["-h".to_string()]), 0);
        let _ = run_mmcli(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mmcli(&[]);
    }
}
