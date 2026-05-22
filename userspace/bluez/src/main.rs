#![deny(clippy::all)]

//! bluez — OurOS Bluetooth management
//!
//! Multi-personality binary for Bluetooth device management.
//! Detected via argv[0]:
//!
//! - `bluetoothctl` (default) — interactive Bluetooth control
//! - `hciconfig` — HCI device configuration
//! - `hcitool` — HCI tool for device discovery and connection
//! - `btmon` — Bluetooth monitor/sniffer
//! - `rfcomm` — RFCOMM channel management

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _BT_CONF: &str = "/etc/bluetooth/main.conf";
const _BT_DEVICES_DIR: &str = "/var/lib/bluetooth";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct HciDevice {
    _id: u32,
    name: String,
    address: String,
    _dev_type: HciType,
    state: HciState,
    _bus: String,
    features: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HciType {
    Primary,
    _Amp,
}

impl std::fmt::Display for HciType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Primary => write!(f, "Primary"),
            Self::_Amp => write!(f, "AMP"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum HciState {
    Up,
    _Down,
}

impl std::fmt::Display for HciState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Up => write!(f, "UP RUNNING"),
            Self::_Down => write!(f, "DOWN"),
        }
    }
}

#[derive(Clone, Debug)]
struct BtDevice {
    address: String,
    name: String,
    _alias: String,
    device_class: DeviceClass,
    paired: bool,
    bonded: bool,
    trusted: bool,
    connected: bool,
    rssi: Option<i16>,
    _icon: String,
    _uuids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DeviceClass {
    Phone,
    Computer,
    AudioVideo,
    _Peripheral,
    _Imaging,
    _Wearable,
    _Toy,
    _Health,
    _Unknown,
}

impl std::fmt::Display for DeviceClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Phone => write!(f, "Phone"),
            Self::Computer => write!(f, "Computer"),
            Self::AudioVideo => write!(f, "Audio/Video"),
            Self::_Peripheral => write!(f, "Peripheral"),
            Self::_Imaging => write!(f, "Imaging"),
            Self::_Wearable => write!(f, "Wearable"),
            Self::_Toy => write!(f, "Toy"),
            Self::_Health => write!(f, "Health"),
            Self::_Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Clone, Debug)]
struct _BtAgent {
    _capability: _AgentCapability,
    _registered: bool,
    _default: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum _AgentCapability {
    _DisplayOnly,
    DisplayYesNo,
    _KeyboardOnly,
    _NoInputNoOutput,
    _KeyboardDisplay,
}

impl std::fmt::Display for _AgentCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_DisplayOnly => write!(f, "DisplayOnly"),
            Self::DisplayYesNo => write!(f, "DisplayYesNo"),
            Self::_KeyboardOnly => write!(f, "KeyboardOnly"),
            Self::_NoInputNoOutput => write!(f, "NoInputNoOutput"),
            Self::_KeyboardDisplay => write!(f, "KeyboardDisplay"),
        }
    }
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_hci_devices() -> Vec<HciDevice> {
    vec![
        HciDevice {
            _id: 0,
            name: "hci0".to_string(),
            address: "00:1A:7D:DA:71:13".to_string(),
            _dev_type: HciType::Primary,
            state: HciState::Up,
            _bus: "USB".to_string(),
            features: vec![
                "LE".to_string(),
                "BR/EDR".to_string(),
                "SCO".to_string(),
                "eSCO".to_string(),
                "Extended Inquiry".to_string(),
                "Secure Simple Pairing".to_string(),
            ],
        },
    ]
}

fn read_bt_devices() -> Vec<BtDevice> {
    vec![
        BtDevice {
            address: "FC:58:FA:A1:26:FC".to_string(),
            name: "Sony WH-1000XM4".to_string(),
            _alias: "Sony WH-1000XM4".to_string(),
            device_class: DeviceClass::AudioVideo,
            paired: true,
            bonded: true,
            trusted: true,
            connected: true,
            rssi: Some(-45),
            _icon: "audio-headset".to_string(),
            _uuids: vec![
                "0000110b-0000-1000-8000-00805f9b34fb".to_string(), // A2DP Sink
                "0000110e-0000-1000-8000-00805f9b34fb".to_string(), // AVRCP
                "0000111e-0000-1000-8000-00805f9b34fb".to_string(), // HFP
            ],
        },
        BtDevice {
            address: "D4:38:9C:12:AB:CD".to_string(),
            name: "Logitech MX Master 3".to_string(),
            _alias: "MX Master 3".to_string(),
            device_class: DeviceClass::Computer,
            paired: true,
            bonded: true,
            trusted: true,
            connected: false,
            rssi: None,
            _icon: "input-mouse".to_string(),
            _uuids: vec![
                "00001124-0000-1000-8000-00805f9b34fb".to_string(), // HID
            ],
        },
        BtDevice {
            address: "A8:B1:D4:56:78:9A".to_string(),
            name: "Pixel 8".to_string(),
            _alias: "My Phone".to_string(),
            device_class: DeviceClass::Phone,
            paired: false,
            bonded: false,
            trusted: false,
            connected: false,
            rssi: Some(-72),
            _icon: "phone".to_string(),
            _uuids: vec![],
        },
    ]
}

// ── bluetoothctl personality ──────────────────────────────────────────

fn run_bluetoothctl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "show".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: bluetoothctl [COMMAND] [ARGS]");
            println!();
            println!("Bluetooth control utility.");
            println!();
            println!("Commands:");
            println!("  show [ctrl]           Show controller info (default)");
            println!("  list                  List available controllers");
            println!("  select <ctrl>         Select default controller");
            println!("  devices               List known devices");
            println!("  paired-devices        List paired devices");
            println!("  scan <on|off>         Start/stop device discovery");
            println!("  pair <dev>            Pair with a device");
            println!("  unpair <dev>          Remove pairing");
            println!("  trust <dev>           Trust a device");
            println!("  untrust <dev>         Untrust a device");
            println!("  connect <dev>         Connect to a device");
            println!("  disconnect <dev>      Disconnect from a device");
            println!("  info <dev>            Show device info");
            println!("  remove <dev>          Remove a device");
            println!("  power <on|off>        Set controller power");
            println!("  discoverable <on|off> Set discoverable mode");
            println!("  pairable <on|off>     Set pairable mode");
            println!("  agent <cap>           Register agent");
            println!("  default-agent         Set registered agent as default");
            println!("  --version             Show version");
            0
        }
        "--version" | "version" => {
            println!("bluetoothctl 0.1.0 (OurOS)");
            0
        }
        "show" => bt_show(&cmd_args),
        "list" => bt_list_controllers(),
        "devices" => bt_list_devices(false),
        "paired-devices" => bt_list_devices(true),
        "scan" => bt_scan(&cmd_args),
        "pair" => bt_pair(&cmd_args),
        "unpair" | "remove" => bt_remove(&cmd_args),
        "trust" => bt_trust(&cmd_args, true),
        "untrust" => bt_trust(&cmd_args, false),
        "connect" => bt_connect(&cmd_args),
        "disconnect" => bt_disconnect(&cmd_args),
        "info" => bt_info(&cmd_args),
        "power" => bt_power(&cmd_args),
        "discoverable" => bt_discoverable(&cmd_args),
        "pairable" => bt_pairable(&cmd_args),
        "agent" => {
            let cap = cmd_args.first().map(|s| s.as_str()).unwrap_or("DisplayYesNo");
            println!("Agent registered: {}", cap);
            0
        }
        "default-agent" => {
            println!("Default agent request successful");
            0
        }
        other => {
            eprintln!("bluetoothctl: unknown command '{}'", other);
            1
        }
    }
}

fn bt_show(_args: &[String]) -> i32 {
    let hci = &read_hci_devices()[0];
    println!("Controller {} [default]", hci.address);
    println!("  Name: OurOS-Desktop");
    println!("  Alias: OurOS-Desktop");
    println!("  Class: 0x000000");
    println!("  Powered: yes");
    println!("  Discoverable: no");
    println!("  DiscoverableTimeout: 0x00000000");
    println!("  Pairable: yes");
    println!("  UUID: Generic Attribute Profile (00001801-0000-1000-8000-00805f9b34fb)");
    println!("  UUID: Generic Access Profile    (00001800-0000-1000-8000-00805f9b34fb)");
    println!("  UUID: A/V Remote Control        (0000110e-0000-1000-8000-00805f9b34fb)");
    println!("  UUID: Audio Sink                (0000110b-0000-1000-8000-00805f9b34fb)");
    println!("  UUID: Audio Source              (0000110a-0000-1000-8000-00805f9b34fb)");
    println!("  Modalias: usb:v1D6Bp0246d0542");
    0
}

fn bt_list_controllers() -> i32 {
    let hcis = read_hci_devices();
    for hci in &hcis {
        println!("Controller {} OurOS-Desktop [default]", hci.address);
    }
    0
}

fn bt_list_devices(paired_only: bool) -> i32 {
    let devices = read_bt_devices();
    for dev in &devices {
        if paired_only && !dev.paired {
            continue;
        }
        println!("Device {} {}", dev.address, dev.name);
    }
    0
}

fn bt_scan(args: &[String]) -> i32 {
    let mode = args.first().map(|s| s.as_str()).unwrap_or("on");
    match mode {
        "on" => {
            println!("[CHG] Controller 00:1A:7D:DA:71:13 Discovering: yes");
            println!("Discovery started");
            // Show some "found" devices
            let devices = read_bt_devices();
            for dev in &devices {
                if !dev.paired {
                    println!("[NEW] Device {} {}", dev.address, dev.name);
                }
            }
        }
        "off" => {
            println!("[CHG] Controller 00:1A:7D:DA:71:13 Discovering: no");
            println!("Discovery stopped");
        }
        _ => {
            eprintln!("bluetoothctl: scan requires 'on' or 'off'");
            return 1;
        }
    }
    0
}

fn bt_pair(args: &[String]) -> i32 {
    let addr = match args.first() {
        Some(a) => a.as_str(),
        None => { eprintln!("bluetoothctl: pair requires device address"); return 1; }
    };
    println!("Attempting to pair with {}", addr);
    println!("[CHG] Device {} Paired: yes", addr);
    println!("Pairing successful");
    0
}

fn bt_remove(args: &[String]) -> i32 {
    let addr = match args.first() {
        Some(a) => a.as_str(),
        None => { eprintln!("bluetoothctl: remove requires device address"); return 1; }
    };
    println!("[DEL] Device {} removed", addr);
    println!("Device has been removed");
    0
}

fn bt_trust(args: &[String], trust: bool) -> i32 {
    let addr = match args.first() {
        Some(a) => a.as_str(),
        None => { eprintln!("bluetoothctl: requires device address"); return 1; }
    };
    println!("[CHG] Device {} Trusted: {}", addr, if trust { "yes" } else { "no" });
    println!("Changing {} {} trust succeeded", addr, if trust { "trust" } else { "untrust" });
    0
}

fn bt_connect(args: &[String]) -> i32 {
    let addr = match args.first() {
        Some(a) => a.as_str(),
        None => { eprintln!("bluetoothctl: connect requires device address"); return 1; }
    };
    println!("Attempting to connect to {}", addr);
    println!("[CHG] Device {} Connected: yes", addr);
    println!("Connection successful");
    0
}

fn bt_disconnect(args: &[String]) -> i32 {
    let addr = match args.first() {
        Some(a) => a.as_str(),
        None => { eprintln!("bluetoothctl: disconnect requires device address"); return 1; }
    };
    println!("[CHG] Device {} Connected: no", addr);
    println!("Successful disconnected");
    0
}

fn bt_info(args: &[String]) -> i32 {
    let addr = match args.first() {
        Some(a) => a.as_str(),
        None => { eprintln!("bluetoothctl: info requires device address"); return 1; }
    };

    let devices = read_bt_devices();
    match devices.iter().find(|d| d.address == addr) {
        Some(dev) => {
            println!("Device {}", dev.address);
            println!("  Name: {}", dev.name);
            println!("  Alias: {}", dev._alias);
            println!("  Class: {}", dev.device_class);
            println!("  Icon: {}", dev._icon);
            println!("  Paired: {}", if dev.paired { "yes" } else { "no" });
            println!("  Bonded: {}", if dev.bonded { "yes" } else { "no" });
            println!("  Trusted: {}", if dev.trusted { "yes" } else { "no" });
            println!("  Connected: {}", if dev.connected { "yes" } else { "no" });
            if let Some(rssi) = dev.rssi {
                println!("  RSSI: {} dBm", rssi);
            }
            for uuid in &dev._uuids {
                println!("  UUID: {}", uuid);
            }
            0
        }
        None => {
            eprintln!("Device {} not found", addr);
            1
        }
    }
}

fn bt_power(args: &[String]) -> i32 {
    let mode = args.first().map(|s| s.as_str()).unwrap_or("on");
    println!("[CHG] Controller 00:1A:7D:DA:71:13 Powered: {}", mode == "on");
    println!("Changing power {} succeeded", mode);
    0
}

fn bt_discoverable(args: &[String]) -> i32 {
    let mode = args.first().map(|s| s.as_str()).unwrap_or("on");
    println!("[CHG] Controller 00:1A:7D:DA:71:13 Discoverable: {}", mode == "on");
    println!("Changing discoverable {} succeeded", mode);
    0
}

fn bt_pairable(args: &[String]) -> i32 {
    let mode = args.first().map(|s| s.as_str()).unwrap_or("on");
    println!("[CHG] Controller 00:1A:7D:DA:71:13 Pairable: {}", mode == "on");
    println!("Changing pairable {} succeeded", mode);
    0
}

// ── hciconfig personality ─────────────────────────────────────────────

fn run_hciconfig(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "list".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: hciconfig [hciX] [COMMAND]");
            println!();
            println!("HCI device configuration.");
            println!();
            println!("Commands:");
            println!("  (no args)    List HCI devices (default)");
            println!("  up           Bring HCI device up");
            println!("  down         Bring HCI device down");
            println!("  reset        Reset HCI device");
            println!("  name [NAME]  Get/set local name");
            println!("  class        Show device class");
            println!("  features     Show device features");
            println!("  --version    Show version");
            0
        }
        "--version" | "-V" => {
            println!("hciconfig 0.1.0 (OurOS)");
            0
        }
        s if s.starts_with("hci") => {
            // hciconfig hciX <command>
            let subcmd = cmd_args.first().map(|s| s.as_str()).unwrap_or("info");
            hci_device_cmd(s, subcmd)
        }
        "up" | "down" | "reset" | "name" | "class" | "features" => {
            hci_device_cmd("hci0", cmd.as_str())
        }
        _ => {
            // Default: list all
            hci_list()
        }
    }
}

fn hci_list() -> i32 {
    let devices = read_hci_devices();
    for hci in &devices {
        println!("{}:\tType: {} Bus: {}", hci.name, hci._dev_type, hci._bus);
        println!("\tBD Address: {} ACL MTU: 1021:8 SCO MTU: 64:1", hci.address);
        println!("\t{}", hci.state);
        println!("\tRX bytes:12345 acl:0 sco:0 events:678 errors:0");
        println!("\tTX bytes:9012 acl:0 sco:0 commands:345 errors:0");
        println!("\tFeatures: {}", hci.features.join(" "));
        println!();
    }
    0
}

fn hci_device_cmd(dev: &str, cmd: &str) -> i32 {
    match cmd {
        "up" => { println!("{}: UP", dev); 0 }
        "down" => { println!("{}: DOWN", dev); 0 }
        "reset" => { println!("{}: Reset", dev); 0 }
        "name" => { println!("{}: Name: 'OurOS-Desktop'", dev); 0 }
        "class" => { println!("{}: Class: 0x000000", dev); 0 }
        "features" => {
            let hcis = read_hci_devices();
            if let Some(hci) = hcis.iter().find(|h| h.name == dev) {
                println!("{}: Features:", dev);
                for f in &hci.features {
                    println!("  {}", f);
                }
            }
            0
        }
        "info" | _ => {
            hci_list()
        }
    }
}

// ── hcitool personality ───────────────────────────────────────────────

fn run_hcitool(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "dev".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: hcitool [COMMAND] [ARGS]");
            println!();
            println!("HCI tool for Bluetooth device operations.");
            println!();
            println!("Commands:");
            println!("  dev              Show local devices (default)");
            println!("  inq              Inquire remote devices");
            println!("  scan             Scan for remote devices (with names)");
            println!("  name <bdaddr>    Get remote device name");
            println!("  info <bdaddr>    Get remote device info");
            println!("  con              List connections");
            println!("  cc <bdaddr>      Create connection");
            println!("  dc <bdaddr>      Disconnect");
            println!("  rssi <bdaddr>    Read RSSI");
            println!("  lescan           LE scan");
            0
        }
        "dev" => {
            let hcis = read_hci_devices();
            println!("Devices:");
            for hci in &hcis {
                println!("\t{}\t{}", hci.name, hci.address);
            }
            0
        }
        "inq" | "scan" => {
            println!("Scanning ...");
            let devices = read_bt_devices();
            for dev in &devices {
                let rssi_str = dev.rssi.map_or(String::new(), |r| format!(" RSSI:{}", r));
                println!("\t{}\t{}{}", dev.address, dev.name, rssi_str);
            }
            0
        }
        "name" => {
            let addr = cmd_args.first().map(|s| s.as_str()).unwrap_or("unknown");
            let devices = read_bt_devices();
            match devices.iter().find(|d| d.address == addr) {
                Some(dev) => { println!("{}", dev.name); 0 }
                None => { eprintln!("hcitool: device {} not found", addr); 1 }
            }
        }
        "con" => {
            let devices = read_bt_devices();
            let connected: Vec<_> = devices.iter().filter(|d| d.connected).collect();
            println!("Connections:");
            for dev in connected {
                println!("\t< ACL {} handle 1 state 1 lm MASTER", dev.address);
            }
            0
        }
        "lescan" => {
            println!("LE Scan ...");
            let devices = read_bt_devices();
            for dev in &devices {
                println!("{} {}", dev.address, dev.name);
            }
            0
        }
        "rssi" => {
            let addr = cmd_args.first().map(|s| s.as_str()).unwrap_or("unknown");
            let devices = read_bt_devices();
            match devices.iter().find(|d| d.address == addr) {
                Some(dev) => {
                    println!("RSSI return value: {}", dev.rssi.unwrap_or(0));
                    0
                }
                None => { eprintln!("hcitool: device {} not found", addr); 1 }
            }
        }
        other => {
            eprintln!("hcitool: unknown command '{}'", other);
            1
        }
    }
}

// ── btmon personality ─────────────────────────────────────────────────

fn run_btmon(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "monitor".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: btmon [OPTIONS]");
            println!();
            println!("Bluetooth monitor. Captures and displays HCI traffic.");
            println!();
            println!("Options:");
            println!("  -w FILE     Write trace to file");
            println!("  -r FILE     Read trace from file");
            println!("  -a          Analyze trace");
            println!("  -s SOCKET   Connect to monitor socket");
            println!("  --version   Show version");
            0
        }
        "--version" | "-V" => {
            println!("btmon 0.1.0 (OurOS)");
            0
        }
        _ => {
            println!("Bluetooth monitor ver 0.1.0");
            println!("= Note: Linux version 6.1.0-ouros (x86_64)");
            println!("= Note: Bluetooth subsystem version 2.22");
            println!();
            println!("@ MGMT Open: bluetoothd (privileged)  {:#06x}", 0x0001);
            println!("@ MGMT Open: btmon (privileged)       {:#06x}", 0x0002);
            println!();
            println!("< HCI Command: Read Local Version (0x04|0x0001) plen 0");
            println!("> HCI Event: Command Complete (0x0e) plen 12");
            println!("      HCI Version: 5.3 (0x0c)");
            println!("      HCI Revision: 0x0100");
            println!("      LMP Version: 5.3 (0x0c)");
            println!("      LMP Subversion: 0x0100");
            println!("      Manufacturer: OurOS Virtual (65535)");
            println!();
            println!("(Press Ctrl+C to stop monitoring)");
            0
        }
    }
}

// ── rfcomm personality ────────────────────────────────────────────────

fn run_rfcomm(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "list".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: rfcomm [COMMAND] [ARGS]");
            println!();
            println!("RFCOMM channel management.");
            println!();
            println!("Commands:");
            println!("  list          List RFCOMM devices");
            println!("  bind DEV ADDR CH  Bind RFCOMM device");
            println!("  release DEV   Release RFCOMM device");
            println!("  connect DEV ADDR CH  Connect RFCOMM");
            0
        }
        "list" => {
            println!("rfcomm0: FC:58:FA:A1:26:FC channel 1 connected [tty-attached]");
            0
        }
        "bind" => {
            let dev = cmd_args.first().map(|s| s.as_str()).unwrap_or("rfcomm0");
            println!("{}: bound (simulated)", dev);
            0
        }
        "release" => {
            let dev = cmd_args.first().map(|s| s.as_str()).unwrap_or("rfcomm0");
            println!("{}: released (simulated)", dev);
            0
        }
        other => {
            eprintln!("rfcomm: unknown command '{}'", other);
            1
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("bluetoothctl");
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

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "hciconfig" => run_hciconfig(rest),
        "hcitool" => run_hcitool(rest),
        "btmon" => run_btmon(rest),
        "rfcomm" => run_rfcomm(rest),
        _ => run_bluetoothctl(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hci_devices() {
        let hcis = read_hci_devices();
        assert_eq!(hcis.len(), 1);
        assert_eq!(hcis[0].name, "hci0");
        assert_eq!(hcis[0].state, HciState::Up);
        assert!(!hcis[0].features.is_empty());
    }

    #[test]
    fn test_bt_devices() {
        let devs = read_bt_devices();
        assert_eq!(devs.len(), 3);
        assert!(devs.iter().any(|d| d.name.contains("Sony")));
        assert!(devs.iter().any(|d| d.name.contains("Logitech")));
    }

    #[test]
    fn test_paired_devices() {
        let devs = read_bt_devices();
        let paired: Vec<_> = devs.iter().filter(|d| d.paired).collect();
        assert_eq!(paired.len(), 2);
    }

    #[test]
    fn test_connected_devices() {
        let devs = read_bt_devices();
        let connected: Vec<_> = devs.iter().filter(|d| d.connected).collect();
        assert_eq!(connected.len(), 1);
        assert!(connected[0].name.contains("Sony"));
    }

    #[test]
    fn test_device_class_display() {
        assert_eq!(format!("{}", DeviceClass::Phone), "Phone");
        assert_eq!(format!("{}", DeviceClass::AudioVideo), "Audio/Video");
        assert_eq!(format!("{}", DeviceClass::Computer), "Computer");
    }

    #[test]
    fn test_hci_type_display() {
        assert_eq!(format!("{}", HciType::Primary), "Primary");
        assert_eq!(format!("{}", HciType::_Amp), "AMP");
    }

    #[test]
    fn test_hci_state_display() {
        assert_eq!(format!("{}", HciState::Up), "UP RUNNING");
        assert_eq!(format!("{}", HciState::_Down), "DOWN");
    }

    #[test]
    fn test_agent_capability_display() {
        assert_eq!(format!("{}", _AgentCapability::DisplayYesNo), "DisplayYesNo");
        assert_eq!(format!("{}", _AgentCapability::_KeyboardOnly), "KeyboardOnly");
    }

    #[test]
    fn test_rssi_values() {
        let devs = read_bt_devices();
        let sony = devs.iter().find(|d| d.name.contains("Sony")).unwrap();
        assert!(sony.rssi.unwrap() < 0);  // RSSI is negative (dBm)
    }

    #[test]
    fn test_unpaired_device() {
        let devs = read_bt_devices();
        let phone = devs.iter().find(|d| d.name.contains("Pixel")).unwrap();
        assert!(!phone.paired);
        assert!(!phone.connected);
        assert!(!phone.trusted);
    }
}
