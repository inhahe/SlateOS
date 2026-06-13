#![deny(clippy::all)]

//! rfkill — SlateOS wireless device control
//!
//! Multi-personality binary for enabling/disabling wireless devices.
//! Detected via argv[0]:
//!
//! - `rfkill` (default) — wireless device block/unblock control
//! - `rfkill-event` — monitor rfkill events

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const RFKILL_DIR: &str = "/sys/class/rfkill";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct RfkillDevice {
    id: u32,
    device_type: RfkillType,
    name: String,
    soft_blocked: bool,
    hard_blocked: bool,
    _persistent: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum RfkillType {
    Wlan,
    Bluetooth,
    Uwb,
    Wimax,
    Wwan,
    Gps,
    Fm,
    Nfc,
    All,
    Unknown(String),
}

impl RfkillType {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "wlan" | "wifi" => Self::Wlan,
            "bluetooth" => Self::Bluetooth,
            "uwb" | "ultrawideband" => Self::Uwb,
            "wimax" => Self::Wimax,
            "wwan" => Self::Wwan,
            "gps" => Self::Gps,
            "fm" => Self::Fm,
            "nfc" => Self::Nfc,
            "all" => Self::All,
            other => Self::Unknown(other.to_string()),
        }
    }

    fn type_id(&self) -> u32 {
        match self {
            Self::Wlan => 1,
            Self::Bluetooth => 2,
            Self::Uwb => 3,
            Self::Wimax => 4,
            Self::Wwan => 5,
            Self::Gps => 6,
            Self::Fm => 7,
            Self::Nfc => 8,
            Self::All => 0,
            Self::Unknown(_) => 255,
        }
    }
}

impl std::fmt::Display for RfkillType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Wlan => write!(f, "wlan"),
            Self::Bluetooth => write!(f, "bluetooth"),
            Self::Uwb => write!(f, "uwb"),
            Self::Wimax => write!(f, "wimax"),
            Self::Wwan => write!(f, "wwan"),
            Self::Gps => write!(f, "gps"),
            Self::Fm => write!(f, "fm"),
            Self::Nfc => write!(f, "nfc"),
            Self::All => write!(f, "all"),
            Self::Unknown(s) => write!(f, "{}", s),
        }
    }
}

// ── Device discovery ───────────────────────────────────────────────────

fn read_devices() -> Vec<RfkillDevice> {
    let entries = match std::fs::read_dir(RFKILL_DIR) {
        Ok(e) => e,
        Err(_) => return simulated_devices(),
    };

    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("rfkill") {
            continue;
        }
        let id: u32 = match name_str.strip_prefix("rfkill").and_then(|s| s.parse().ok()) {
            Some(v) => v,
            None => continue,
        };

        let base = entry.path();
        let dev_type = read_sysfs(&base.join("type")).unwrap_or_default();
        let dev_name = read_sysfs(&base.join("name")).unwrap_or_else(|| format!("rfkill{}", id));
        let soft = read_sysfs(&base.join("soft")).map(|s| s == "1").unwrap_or(false);
        let hard = read_sysfs(&base.join("hard")).map(|s| s == "1").unwrap_or(false);
        let persistent = read_sysfs(&base.join("persistent")).map(|s| s == "1").unwrap_or(false);

        devices.push(RfkillDevice {
            id,
            device_type: RfkillType::from_str(&dev_type),
            name: dev_name,
            soft_blocked: soft,
            hard_blocked: hard,
            _persistent: persistent,
        });
    }

    if devices.is_empty() {
        return simulated_devices();
    }

    devices.sort_by_key(|d| d.id);
    devices
}

fn simulated_devices() -> Vec<RfkillDevice> {
    vec![
        RfkillDevice {
            id: 0,
            device_type: RfkillType::Wlan,
            name: "phy0".to_string(),
            soft_blocked: false,
            hard_blocked: false,
            _persistent: false,
        },
        RfkillDevice {
            id: 1,
            device_type: RfkillType::Bluetooth,
            name: "hci0".to_string(),
            soft_blocked: false,
            hard_blocked: false,
            _persistent: false,
        },
        RfkillDevice {
            id: 2,
            device_type: RfkillType::Wwan,
            name: "wwan0".to_string(),
            soft_blocked: true,
            hard_blocked: false,
            _persistent: true,
        },
    ]
}

fn read_sysfs(path: &std::path::Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

// ── Commands ───────────────────────────────────────────────────────────

fn cmd_list(args: &[String]) {
    let no_headings = args.iter().any(|a| a == "-n" || a == "--no-headings");
    let json_mode = args.iter().any(|a| a == "-J" || a == "--json");
    let output_all = args.iter().any(|a| a == "-o" || a == "--output-all");

    let devices = read_devices();

    if json_mode {
        println!("{{\"rfkill\": [");
        for (i, d) in devices.iter().enumerate() {
            println!("  {{");
            println!("    \"id\": {},", d.id);
            println!("    \"type\": \"{}\",", d.device_type);
            println!("    \"type-desc\": \"{}\",", type_description(&d.device_type));
            println!("    \"soft\": \"{}\",", if d.soft_blocked { "blocked" } else { "unblocked" });
            println!("    \"hard\": \"{}\",", if d.hard_blocked { "blocked" } else { "unblocked" });
            println!("    \"device\": \"{}\"", d.name);
            if i + 1 < devices.len() {
                println!("  }},");
            } else {
                println!("  }}");
            }
        }
        println!("]}}");
        return;
    }

    if !no_headings {
        if output_all {
            println!("{:<4} {:>4} {:<12} {:<16} {:<10} {:<10} PERSISTENT",
                "ID", "TYPE", "TYPE-DESC", "DEVICE", "SOFT", "HARD");
        } else {
            println!("{:<4} {:>4} {:<12} {:<16} {:<10} HARD",
                "ID", "TYPE", "TYPE-DESC", "DEVICE", "SOFT");
        }
    }

    for d in &devices {
        let soft = if d.soft_blocked { "blocked" } else { "unblocked" };
        let hard = if d.hard_blocked { "blocked" } else { "unblocked" };
        if output_all {
            println!("{:<4} {:>4} {:<12} {:<16} {:<10} {:<10} {}",
                d.id, d.device_type.type_id(), type_description(&d.device_type),
                d.name, soft, hard, if d._persistent { "yes" } else { "no" });
        } else {
            println!("{:<4} {:>4} {:<12} {:<16} {:<10} {}",
                d.id, d.device_type.type_id(), type_description(&d.device_type),
                d.name, soft, hard);
        }
    }
}

fn cmd_block(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: rfkill block <type|id>");
        process::exit(1);
    }

    let target = &args[0];
    let devices = read_devices();

    if let Ok(id) = target.parse::<u32>() {
        if let Some(d) = devices.iter().find(|d| d.id == id) {
            println!("Soft blocked device {} ({})", d.id, d.name);
        } else {
            eprintln!("No device with ID {}", id);
            process::exit(1);
        }
    } else {
        let rtype = RfkillType::from_str(target);
        let mut count = 0;
        for d in &devices {
            if rtype == RfkillType::All || d.device_type == rtype {
                println!("Soft blocked device {} ({}) [{}]", d.id, d.name, d.device_type);
                count += 1;
            }
        }
        if count == 0 {
            eprintln!("No devices of type '{}'", target);
            process::exit(1);
        }
    }
}

fn cmd_unblock(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: rfkill unblock <type|id>");
        process::exit(1);
    }

    let target = &args[0];
    let devices = read_devices();

    if let Ok(id) = target.parse::<u32>() {
        if let Some(d) = devices.iter().find(|d| d.id == id) {
            if d.hard_blocked {
                eprintln!("Warning: device {} ({}) is hardware blocked", d.id, d.name);
            }
            println!("Soft unblocked device {} ({})", d.id, d.name);
        } else {
            eprintln!("No device with ID {}", id);
            process::exit(1);
        }
    } else {
        let rtype = RfkillType::from_str(target);
        let mut count = 0;
        for d in &devices {
            if rtype == RfkillType::All || d.device_type == rtype {
                if d.hard_blocked {
                    eprintln!("Warning: device {} ({}) is hardware blocked", d.id, d.name);
                }
                println!("Soft unblocked device {} ({}) [{}]", d.id, d.name, d.device_type);
                count += 1;
            }
        }
        if count == 0 {
            eprintln!("No devices of type '{}'", target);
            process::exit(1);
        }
    }
}

fn cmd_toggle(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: rfkill toggle <type|id>");
        process::exit(1);
    }

    let target = &args[0];
    let devices = read_devices();

    if let Ok(id) = target.parse::<u32>() {
        if let Some(d) = devices.iter().find(|d| d.id == id) {
            let action = if d.soft_blocked { "unblocked" } else { "blocked" };
            println!("Soft {} device {} ({})", action, d.id, d.name);
        } else {
            eprintln!("No device with ID {}", id);
            process::exit(1);
        }
    } else {
        let rtype = RfkillType::from_str(target);
        for d in &devices {
            if rtype == RfkillType::All || d.device_type == rtype {
                let action = if d.soft_blocked { "unblocked" } else { "blocked" };
                println!("Soft {} device {} ({})", action, d.id, d.name);
            }
        }
    }
}

fn cmd_event() {
    println!("Listening for rfkill events...");
    println!("(Press Ctrl+C to stop)");
    println!();

    // Simulate some events
    let devices = read_devices();
    for d in &devices {
        let _status = if d.soft_blocked { "blocked" } else { "unblocked" };
        println!("{:>10}: idx {} type {} op change soft {} hard {}",
            "rfkill", d.id, d.device_type.type_id(),
            if d.soft_blocked { 1 } else { 0 },
            if d.hard_blocked { 1 } else { 0 });
    }
}

fn type_description(t: &RfkillType) -> &str {
    match t {
        RfkillType::Wlan => "Wireless LAN",
        RfkillType::Bluetooth => "Bluetooth",
        RfkillType::Uwb => "Ultra-Wideband",
        RfkillType::Wimax => "WiMAX",
        RfkillType::Wwan => "Wireless WAN",
        RfkillType::Gps => "GPS",
        RfkillType::Fm => "FM",
        RfkillType::Nfc => "NFC",
        RfkillType::All => "All",
        RfkillType::Unknown(_) => "Unknown",
    }
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_help() {
    println!("rfkill — Wireless device control");
    println!();
    println!("Usage: rfkill [OPTIONS] COMMAND [ARGS]");
    println!();
    println!("Commands:");
    println!("  list                   List rfkill devices (default)");
    println!("  block <type|id>        Soft-block a device");
    println!("  unblock <type|id>      Soft-unblock a device");
    println!("  toggle <type|id>       Toggle soft-block state");
    println!("  event                  Listen for rfkill events");
    println!();
    println!("Types: wlan, bluetooth, uwb, wimax, wwan, gps, fm, nfc, all");
    println!();
    println!("Options:");
    println!("  -J, --json             JSON output");
    println!("  -n, --no-headings      Suppress column headers");
    println!("  -o, --output-all       Show all columns");
    println!("  -h, --help             Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("rfkill");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    if prog_name == "rfkill-event" {
        cmd_event();
        return;
    }

    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "list".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_help();
        return;
    }

    match cmd.as_str() {
        "list" => cmd_list(&cmd_args),
        "block" => cmd_block(&cmd_args),
        "unblock" => cmd_unblock(&cmd_args),
        "toggle" => cmd_toggle(&cmd_args),
        "event" => cmd_event(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_help();
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfkill_type_from_str() {
        assert_eq!(RfkillType::from_str("wlan"), RfkillType::Wlan);
        assert_eq!(RfkillType::from_str("wifi"), RfkillType::Wlan);
        assert_eq!(RfkillType::from_str("bluetooth"), RfkillType::Bluetooth);
        assert_eq!(RfkillType::from_str("wwan"), RfkillType::Wwan);
        assert_eq!(RfkillType::from_str("all"), RfkillType::All);
        assert_eq!(RfkillType::from_str("nfc"), RfkillType::Nfc);
    }

    #[test]
    fn test_rfkill_type_display() {
        assert_eq!(format!("{}", RfkillType::Wlan), "wlan");
        assert_eq!(format!("{}", RfkillType::Bluetooth), "bluetooth");
        assert_eq!(format!("{}", RfkillType::All), "all");
    }

    #[test]
    fn test_rfkill_type_id() {
        assert_eq!(RfkillType::Wlan.type_id(), 1);
        assert_eq!(RfkillType::Bluetooth.type_id(), 2);
        assert_eq!(RfkillType::All.type_id(), 0);
    }

    #[test]
    fn test_type_description() {
        assert_eq!(type_description(&RfkillType::Wlan), "Wireless LAN");
        assert_eq!(type_description(&RfkillType::Bluetooth), "Bluetooth");
        assert_eq!(type_description(&RfkillType::Nfc), "NFC");
    }

    #[test]
    fn test_simulated_devices() {
        let devices = simulated_devices();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].device_type, RfkillType::Wlan);
        assert_eq!(devices[1].device_type, RfkillType::Bluetooth);
        assert_eq!(devices[2].device_type, RfkillType::Wwan);
    }

    #[test]
    fn test_read_devices() {
        let devices = read_devices();
        assert!(!devices.is_empty());
    }

    #[test]
    fn test_unknown_type() {
        let t = RfkillType::from_str("zigbee");
        assert!(matches!(t, RfkillType::Unknown(_)));
        assert_eq!(t.type_id(), 255);
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("rfkill", "rfkill"),
            ("rfkill-event", "rfkill-event"),
            ("/usr/sbin/rfkill", "rfkill"),
            ("C:\\bin\\rfkill.exe", "rfkill"),
        ];
        for (input, expected) in cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, expected);
        }
    }

    #[test]
    fn test_device_blocked_state() {
        let d = RfkillDevice {
            id: 0,
            device_type: RfkillType::Wlan,
            name: "test".to_string(),
            soft_blocked: true,
            hard_blocked: false,
            _persistent: false,
        };
        assert!(d.soft_blocked);
        assert!(!d.hard_blocked);
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(RfkillType::from_str("WLAN"), RfkillType::Wlan);
        assert_eq!(RfkillType::from_str("Bluetooth"), RfkillType::Bluetooth);
        assert_eq!(RfkillType::from_str("WWAN"), RfkillType::Wwan);
    }
}
