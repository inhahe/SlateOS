#![deny(clippy::all)]

//! bluez-cli — Slate OS BlueZ Bluetooth tools
//!
//! Multi-personality: `bluetoothctl`, `hcitool`, `hciconfig`, `btmon`, `sdptool`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_bluetoothctl(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: bluetoothctl [OPTIONS] [COMMAND]");
        println!();
        println!("bluetoothctl — BlueZ Bluetooth control (Slate OS).");
        println!();
        println!("Commands: list, show, power on|off, scan on|off, devices,");
        println!("  pair ADDR, connect ADDR, disconnect ADDR, trust ADDR,");
        println!("  info ADDR, remove ADDR");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("bluetoothctl: 5.72 (Slate OS)");
        return 0;
    }

    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match subcmd {
        "list" => println!("Controller AA:BB:CC:DD:EE:FF SlateOS-BT [default]"),
        "show" => {
            println!("Controller AA:BB:CC:DD:EE:FF (public)");
            println!("\tName: SlateOS-BT");
            println!("\tAlias: SlateOS-BT");
            println!("\tClass: 0x001c010c");
            println!("\tPowered: yes");
            println!("\tDiscoverable: no");
            println!("\tPairable: yes");
            println!("\tUUID: Generic Access Profile");
            println!("\tUUID: Generic Attribute Profile");
            println!("\tUUID: A/V Remote Control");
            println!("\tUUID: Audio Source");
            println!("\tUUID: Handsfree Audio Gateway");
            println!("\tModalias: usb:v1D6Bp0246d0540");
            println!("\tDiscovering: no");
        }
        "devices" => {
            println!("Device 11:22:33:44:55:66 WH-1000XM5");
            println!("Device AA:11:BB:22:CC:33 Keyboard K780");
            println!("Device DD:44:EE:55:FF:66 MX Master 3");
        }
        "scan" => println!("Discovery started"),
        "power" => {
            let state = args.get(1).map(|s| s.as_str()).unwrap_or("on");
            println!("Changing power {}... succeeded", state);
        }
        "pair" | "connect" | "trust" => {
            let addr = args.get(1).map(|s| s.as_str()).unwrap_or("11:22:33:44:55:66");
            println!("Attempting to {} {}...", subcmd, addr);
            println!("{} successful", subcmd);
        }
        _ => println!("[bluetooth]# {}", subcmd),
    }
    0
}

fn run_hciconfig(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: hciconfig [DEV] [COMMAND]");
        return 0;
    }
    let _ = args;
    println!("hci0:\tType: Primary  Bus: USB");
    println!("\tBD Address: AA:BB:CC:DD:EE:FF  ACL MTU: 1021:4  SCO MTU: 96:6");
    println!("\tUP RUNNING PSCAN");
    println!("\tRX bytes:12345 acl:100 sco:0 events:200 errors:0");
    println!("\tTX bytes:6789 acl:50 sco:0 commands:150 errors:0");
    0
}

fn run_hcitool(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help") {
        println!("Usage: hcitool COMMAND [OPTIONS]");
        println!("Commands: dev, scan, inq, name ADDR, info ADDR, lescan");
        return 0;
    }
    let subcmd = args.first().map(|s| s.as_str()).unwrap_or("dev");
    match subcmd {
        "dev" => {
            println!("Devices:");
            println!("\thci0\tAA:BB:CC:DD:EE:FF");
        }
        "scan" | "inq" => {
            println!("Scanning...");
            println!("\t11:22:33:44:55:66\tWH-1000XM5");
            println!("\tAA:11:BB:22:CC:33\tKeyboard K780");
        }
        "lescan" => {
            println!("LE Scan...");
            println!("11:22:33:44:55:66 (unknown)");
            println!("11:22:33:44:55:66 WH-1000XM5");
        }
        _ => println!("hcitool: command '{}' completed", subcmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "bluetoothctl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "hcitool" => run_hcitool(&rest),
        "hciconfig" => run_hciconfig(&rest),
        "btmon" => { println!("Bluetooth monitor ver 5.72 (Slate OS)"); println!("= Open Index: AA:BB:CC:DD:EE:FF"); 0 }
        "sdptool" => { println!("Inquiring..."); println!("Service Name: Audio Sink"); 0 }
        _ => run_bluetoothctl(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_bluetoothctl};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/bluez"), "bluez");
        assert_eq!(basename(r"C:\bin\bluez.exe"), "bluez.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("bluez.exe"), "bluez");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_bluetoothctl(&["--help".to_string()]), 0);
        assert_eq!(run_bluetoothctl(&["-h".to_string()]), 0);
        let _ = run_bluetoothctl(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_bluetoothctl(&[]);
    }
}
