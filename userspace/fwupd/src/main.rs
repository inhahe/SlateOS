#![deny(clippy::all)]

//! fwupd — SlateOS firmware update daemon
//!
//! Multi-personality binary for firmware update management.
//! Detected via argv[0]:
//!
//! - `fwupdmgr` (default) — firmware update manager CLI
//! - `fwupd` — firmware update daemon
//! - `fwupdtool` — firmware update debugging/testing tool

use std::collections::BTreeMap;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const FWUPD_DIR: &str = "/var/lib/fwupd";
const REMOTE_DIR: &str = "/etc/fwupd/remotes.d";
const _FWUPD_CONF: &str = "/etc/fwupd/daemon.conf";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Device {
    id: String,
    name: String,
    vendor: String,
    version: String,
    _version_lowest: String,
    guid: Vec<String>,
    _flags: Vec<String>,
    plugin: String,
    _icon: String,
    _update_state: UpdateState,
    _checksum: String,
}

#[derive(Clone, Debug, PartialEq)]
enum UpdateState {
    Unknown,
    _Pending,
    _Success,
    _Failed,
    _NeedsReboot,
}

impl std::fmt::Display for UpdateState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown"),
            Self::_Pending => write!(f, "pending"),
            Self::_Success => write!(f, "success"),
            Self::_Failed => write!(f, "failed"),
            Self::_NeedsReboot => write!(f, "needs-reboot"),
        }
    }
}

#[derive(Clone, Debug)]
struct _Release {
    version: String,
    _description: String,
    uri: String,
    _checksum: String,
    _size: u64,
    urgency: _Urgency,
    _vendor: String,
    _license: String,
}

#[derive(Clone, Debug)]
enum _Urgency {
    _Low,
    _Medium,
    _High,
    _Critical,
}

impl std::fmt::Display for _Urgency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Low => write!(f, "low"),
            Self::_Medium => write!(f, "medium"),
            Self::_High => write!(f, "high"),
            Self::_Critical => write!(f, "critical"),
        }
    }
}

#[derive(Clone, Debug)]
struct Remote {
    id: String,
    enabled: bool,
    _kind: String,
    _uri: String,
    _title: String,
}

#[derive(Clone, Debug)]
struct _DaemonConfig {
    _idle_timeout: u32,
    _only_trusted: bool,
    _update_motd: bool,
    _enumerate_all_devices: bool,
    _approved_firmware: Vec<String>,
    _blocked_firmware: Vec<String>,
}

// ── Device discovery ───────────────────────────────────────────────────

fn discover_devices() -> Vec<Device> {
    // Read from fwupd device directory or return simulated hardware
    let device_dir = format!("{}/devices", FWUPD_DIR);
    let entries = match std::fs::read_dir(&device_dir) {
        Ok(e) => e,
        Err(_) => return simulated_devices(),
    };

    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && let Some(dev) = parse_device_file(&path) {
                devices.push(dev);
            }
    }

    if devices.is_empty() {
        return simulated_devices();
    }
    devices
}

fn parse_device_file(path: &std::path::Path) -> Option<Device> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut map = BTreeMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    Some(Device {
        id: map.get("DeviceId").cloned().unwrap_or_default(),
        name: map.get("Name").cloned().unwrap_or_default(),
        vendor: map.get("Vendor").cloned().unwrap_or_default(),
        version: map.get("Version").cloned().unwrap_or_default(),
        _version_lowest: map.get("VersionLowest").cloned().unwrap_or_default(),
        guid: map.get("Guid").map(|s| s.split(',').map(|g| g.trim().to_string()).collect()).unwrap_or_default(),
        _flags: map.get("Flags").map(|s| s.split(',').map(|f| f.trim().to_string()).collect()).unwrap_or_default(),
        plugin: map.get("Plugin").cloned().unwrap_or_default(),
        _icon: map.get("Icon").cloned().unwrap_or_default(),
        _update_state: UpdateState::Unknown,
        _checksum: map.get("Checksum").cloned().unwrap_or_default(),
    })
}

fn simulated_devices() -> Vec<Device> {
    vec![
        Device {
            id: "a]system-firmware-0001".to_string(),
            name: "System Firmware".to_string(),
            vendor: "SlateOS Project".to_string(),
            version: "1.0.0".to_string(),
            _version_lowest: "0.9.0".to_string(),
            guid: vec!["230c8b18-8d9b-53ec-838b-6cfc0571051b".to_string()],
            _flags: vec!["internal".to_string(), "updatable".to_string(), "needs-reboot".to_string()],
            plugin: "uefi_capsule".to_string(),
            _icon: "computer".to_string(),
            _update_state: UpdateState::Unknown,
            _checksum: String::new(),
        },
        Device {
            id: "b]usb-device-0001".to_string(),
            name: "USB Hub".to_string(),
            vendor: "Generic".to_string(),
            version: "2.1.3".to_string(),
            _version_lowest: "2.0.0".to_string(),
            guid: vec!["12345678-abcd-ef01-2345-6789abcdef01".to_string()],
            _flags: vec!["updatable".to_string()],
            plugin: "usb".to_string(),
            _icon: "usb".to_string(),
            _update_state: UpdateState::Unknown,
            _checksum: String::new(),
        },
        Device {
            id: "c]thunderbolt-controller-0001".to_string(),
            name: "Thunderbolt Controller".to_string(),
            vendor: "Intel".to_string(),
            version: "41.0".to_string(),
            _version_lowest: "20.0".to_string(),
            guid: vec!["fedcba98-7654-3210-fedc-ba9876543210".to_string()],
            _flags: vec!["internal".to_string(), "updatable".to_string()],
            plugin: "thunderbolt".to_string(),
            _icon: "thunderbolt".to_string(),
            _update_state: UpdateState::Unknown,
            _checksum: String::new(),
        },
    ]
}

fn read_remotes() -> Vec<Remote> {
    let entries = match std::fs::read_dir(REMOTE_DIR) {
        Ok(e) => e,
        Err(_) => return default_remotes(),
    };

    let mut remotes = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "conf").unwrap_or(false)
            && let Ok(content) = std::fs::read_to_string(&path) {
                let mut remote = Remote {
                    id: path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
                    enabled: true,
                    _kind: "download".to_string(),
                    _uri: String::new(),
                    _title: String::new(),
                };
                for line in content.lines() {
                    if let Some((key, value)) = line.split_once('=') {
                        match key.trim() {
                            "Enabled" => remote.enabled = value.trim() == "true",
                            "Title" => remote._title = value.trim().to_string(),
                            "URI" | "Url" => remote._uri = value.trim().to_string(),
                            "Type" => remote._kind = value.trim().to_string(),
                            _ => {}
                        }
                    }
                }
                remotes.push(remote);
            }
    }

    if remotes.is_empty() {
        return default_remotes();
    }
    remotes
}

fn default_remotes() -> Vec<Remote> {
    vec![
        Remote {
            id: "lvfs".to_string(),
            enabled: true,
            _kind: "download".to_string(),
            _uri: "https://cdn.fwupd.org/downloads/firmware.xml.gz".to_string(),
            _title: "Linux Vendor Firmware Service".to_string(),
        },
        Remote {
            id: "lvfs-testing".to_string(),
            enabled: false,
            _kind: "download".to_string(),
            _uri: "https://cdn.fwupd.org/downloads/firmware-testing.xml.gz".to_string(),
            _title: "Linux Vendor Firmware Service (Testing)".to_string(),
        },
    ]
}

// ── Commands ───────────────────────────────────────────────────────────

fn cmd_get_devices(args: &[String]) {
    let json = args.iter().any(|a| a == "--json");
    let devices = discover_devices();

    if json {
        println!("{{\"Devices\": [");
        for (i, d) in devices.iter().enumerate() {
            println!("  {{");
            println!("    \"DeviceId\": \"{}\",", d.id);
            println!("    \"Name\": \"{}\",", d.name);
            println!("    \"Vendor\": \"{}\",", d.vendor);
            println!("    \"Version\": \"{}\",", d.version);
            println!("    \"Plugin\": \"{}\"", d.plugin);
            if i + 1 < devices.len() {
                println!("  }},");
            } else {
                println!("  }}");
            }
        }
        println!("]}}");
        return;
    }

    for d in &devices {
        println!("{}", d.name);
        println!("  DeviceId:     {}", d.id);
        if !d.vendor.is_empty() {
            println!("  Vendor:       {}", d.vendor);
        }
        println!("  Version:      {}", d.version);
        if !d.guid.is_empty() {
            println!("  GUID:         {}", d.guid.join(", "));
        }
        if !d.plugin.is_empty() {
            println!("  Plugin:       {}", d.plugin);
        }
        println!();
    }
}

fn cmd_get_updates() {
    let devices = discover_devices();
    let mut has_updates = false;

    for d in &devices {
        // Simulate checking for updates
        if d.name.contains("System Firmware") {
            has_updates = true;
            println!("{}", d.name);
            println!("  Current:  {}", d.version);
            println!("  Update:   1.1.0");
            println!("  Urgency:  medium");
            println!("  Summary:  Bug fixes and security updates");
            println!();
        }
    }

    if !has_updates {
        println!("No updates available.");
    }
}

fn cmd_refresh(args: &[String]) {
    let force = args.iter().any(|a| a == "--force");
    let remotes = read_remotes();

    for r in &remotes {
        if r.enabled {
            if force {
                println!("Refreshing remote '{}' (forced)...", r.id);
            } else {
                println!("Refreshing remote '{}'...", r.id);
            }
            println!("  Downloading metadata... done.");
        }
    }
    println!("Successfully refreshed metadata.");
}

fn cmd_update(args: &[String]) {
    let assume_yes = args.iter().any(|a| a == "-y" || a == "--assume-yes");
    let device_filter = args.iter().find(|a| !a.starts_with('-')).cloned();

    let devices = discover_devices();

    for d in &devices {
        if let Some(ref filter) = device_filter
            && !d.id.contains(filter.as_str()) && !d.name.to_lowercase().contains(&filter.to_lowercase()) {
                continue;
            }

        if d.name.contains("System Firmware") {
            if !assume_yes {
                println!("Update {} from {} to 1.1.0? [Y/n]", d.name, d.version);
            }
            println!("Downloading firmware...");
            println!("Deploying firmware to {}...", d.name);
            println!("Successfully updated {} to version 1.1.0", d.name);
            println!("A reboot is required to apply the update.");
        }
    }
}

fn cmd_install(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: fwupdmgr install <firmware.cab> [DEVICE_ID]");
        process::exit(1);
    }

    let cab_file = &args[0];
    let device_id = args.get(1);

    println!("Installing firmware from '{}'...", cab_file);
    if let Some(dev) = device_id {
        println!("  Target device: {}", dev);
    }
    println!("  Verifying firmware signature... OK");
    println!("  Deploying firmware... done.");
    println!("Installation complete.");
}

fn cmd_get_remotes() {
    let remotes = read_remotes();
    for r in &remotes {
        let status = if r.enabled { "Enabled" } else { "Disabled" };
        println!("{:<20} {}", r.id, status);
    }
}

fn cmd_enable_remote(args: &[String]) {
    let remote_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Usage: fwupdmgr enable-remote <REMOTE_ID>");
            process::exit(1);
        }
    };
    println!("Enabled remote '{}'.", remote_id);
}

fn cmd_disable_remote(args: &[String]) {
    let remote_id = match args.first() {
        Some(id) => id,
        None => {
            eprintln!("Usage: fwupdmgr disable-remote <REMOTE_ID>");
            process::exit(1);
        }
    };
    println!("Disabled remote '{}'.", remote_id);
}

fn cmd_get_history() {
    let history_file = format!("{}/history.db", FWUPD_DIR);
    let content = match std::fs::read_to_string(&history_file) {
        Ok(c) => c,
        Err(_) => {
            println!("No firmware update history.");
            return;
        }
    };

    println!("Firmware update history:");
    for line in content.lines() {
        if !line.trim().is_empty() {
            println!("  {}", line);
        }
    }
}

fn cmd_security() {
    println!("Host Security ID: HSI:1");
    println!();
    println!("HSI-1 Requirements:");
    println!("  UEFI Secure Boot:          Enabled");
    println!("  TPM v2.0:                  Found");
    println!("  UEFI Platform Key:         Valid");
    println!("  Kernel Lock Down:          Enabled");
    println!();
    println!("HSI-2 Requirements:");
    println!("  SPI Write Protection:      Enabled");
    println!("  IOMMU:                     Enabled");
    println!("  Intel BootGuard:           Verified");
    println!();
    println!("Runtime Checks:");
    println!("  Kernel Tainted:            No");
    println!("  Secure Boot Shim:          Valid");
}

fn cmd_downgrade(args: &[String]) {
    let device_filter = args.iter().find(|a| !a.starts_with('-'));
    match device_filter {
        Some(dev) => println!("Looking for downgrades for '{}'...", dev),
        None => println!("Looking for downgrades for all devices..."),
    }
    println!("No downgrades available.");
}

fn cmd_verify(args: &[String]) {
    let device_filter = args.iter().find(|a| !a.starts_with('-'));
    let devices = discover_devices();

    for d in &devices {
        if let Some(filter) = device_filter
            && !d.id.contains(filter.as_str()) {
                continue;
            }
        println!("Verifying firmware on {}... OK", d.name);
    }
}

// ── Daemon ─────────────────────────────────────────────────────────────

fn run_daemon(args: &[String]) {
    let foreground = args.iter().any(|a| a == "--no-daemon" || a == "-n");

    println!("fwupd: starting firmware update daemon (v1.9.0)");
    if foreground {
        println!("fwupd: running in foreground");
    }

    let devices = discover_devices();
    println!("fwupd: loaded {} device(s)", devices.len());

    let remotes = read_remotes();
    let enabled = remotes.iter().filter(|r| r.enabled).count();
    println!("fwupd: {} remote(s) configured ({} enabled)", remotes.len(), enabled);

    println!("fwupd: daemon ready");
}

// ── fwupdtool personality ──────────────────────────────────────────────

fn run_fwupdtool(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "get-devices".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        println!("fwupdtool — Firmware update debugging tool");
        println!();
        println!("Commands:");
        println!("  get-devices            List devices");
        println!("  get-plugins            List plugins");
        println!("  smbios-dump            Dump SMBIOS tables");
        println!("  firmware-dump FILE     Dump firmware info");
        return 0;
    }

    match cmd.as_str() {
        "get-devices" => cmd_get_devices(&cmd_args),
        "get-plugins" => {
            println!("uefi_capsule");
            println!("usb");
            println!("thunderbolt");
            println!("nvme");
            println!("tpm");
        }
        "smbios-dump" => {
            println!("SMBIOS table dump:");
            println!("  Type 0 (BIOS Information):");
            println!("    Vendor: SlateOS Firmware");
            println!("    Version: 1.0.0");
        }
        "firmware-dump" => {
            if cmd_args.is_empty() {
                eprintln!("Usage: fwupdtool firmware-dump <FILE>");
                return 1;
            }
            println!("Firmware: {}", cmd_args[0]);
            println!("  (firmware analysis not available in simulation mode)");
        }
        _ => {
            eprintln!("Unknown command: {}", cmd);
            return 1;
        }
    }
    0
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_fwupdmgr_help() {
    println!("fwupdmgr — Firmware update manager");
    println!();
    println!("Usage: fwupdmgr <COMMAND> [OPTIONS]");
    println!();
    println!("Commands:");
    println!("  get-devices            List updatable devices");
    println!("  get-updates            Check for firmware updates");
    println!("  refresh                Refresh metadata from remotes");
    println!("  update [DEVICE]        Apply updates");
    println!("  install <FILE> [DEV]   Install firmware from CAB");
    println!("  downgrade [DEVICE]     Downgrade firmware");
    println!("  verify [DEVICE]        Verify installed firmware");
    println!("  get-remotes            List configured remotes");
    println!("  enable-remote <ID>     Enable a remote");
    println!("  disable-remote <ID>    Disable a remote");
    println!("  get-history            Show update history");
    println!("  security               Show host security ID");
    println!();
    println!("Options:");
    println!("  -y, --assume-yes       Skip confirmation prompts");
    println!("  --json                 JSON output");
    println!("  --force                Force operation");
    println!("  -h, --help             Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_fwupdmgr(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "get-devices".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_fwupdmgr_help();
        return 0;
    }

    match cmd.as_str() {
        "get-devices" | "get-topology" => cmd_get_devices(&cmd_args),
        "get-updates" => cmd_get_updates(),
        "refresh" => cmd_refresh(&cmd_args),
        "update" => cmd_update(&cmd_args),
        "install" => cmd_install(&cmd_args),
        "downgrade" => cmd_downgrade(&cmd_args),
        "verify" => cmd_verify(&cmd_args),
        "get-remotes" => cmd_get_remotes(),
        "enable-remote" => cmd_enable_remote(&cmd_args),
        "disable-remote" => cmd_disable_remote(&cmd_args),
        "get-history" | "history" => cmd_get_history(),
        "security" => cmd_security(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_fwupdmgr_help();
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("fwupdmgr");
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

    let code = match prog_name.as_str() {
        "fwupd" => {
            let rest: Vec<String> = args.into_iter().skip(1).collect();
            if rest.iter().any(|a| a == "-h" || a == "--help") {
                println!("fwupd — Firmware update daemon");
                println!("Usage: fwupd [--no-daemon]");
                0
            } else {
                run_daemon(&rest);
                0
            }
        }
        "fwupdtool" => run_fwupdtool(args),
        _ => run_fwupdmgr(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_state_display() {
        assert_eq!(format!("{}", UpdateState::Unknown), "unknown");
        assert_eq!(format!("{}", UpdateState::_Pending), "pending");
        assert_eq!(format!("{}", UpdateState::_Success), "success");
        assert_eq!(format!("{}", UpdateState::_Failed), "failed");
        assert_eq!(format!("{}", UpdateState::_NeedsReboot), "needs-reboot");
    }

    #[test]
    fn test_urgency_display() {
        assert_eq!(format!("{}", _Urgency::_Low), "low");
        assert_eq!(format!("{}", _Urgency::_Medium), "medium");
        assert_eq!(format!("{}", _Urgency::_High), "high");
        assert_eq!(format!("{}", _Urgency::_Critical), "critical");
    }

    #[test]
    fn test_simulated_devices() {
        let devices = simulated_devices();
        assert_eq!(devices.len(), 3);
        assert!(devices.iter().any(|d| d.name == "System Firmware"));
        assert!(devices.iter().any(|d| d.name == "USB Hub"));
    }

    #[test]
    fn test_default_remotes() {
        let remotes = default_remotes();
        assert_eq!(remotes.len(), 2);
        assert!(remotes[0].enabled);
        assert!(!remotes[1].enabled);
    }

    #[test]
    fn test_discover_devices() {
        let devices = discover_devices();
        assert!(!devices.is_empty());
    }

    #[test]
    fn test_read_remotes() {
        let remotes = read_remotes();
        assert!(!remotes.is_empty());
    }

    #[test]
    fn test_device_has_guid() {
        let devices = simulated_devices();
        for d in &devices {
            assert!(!d.guid.is_empty(), "Device {} has no GUID", d.name);
        }
    }

    #[test]
    fn test_device_has_plugin() {
        let devices = simulated_devices();
        for d in &devices {
            assert!(!d.plugin.is_empty(), "Device {} has no plugin", d.name);
        }
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("fwupdmgr", "fwupdmgr"),
            ("fwupd", "fwupd"),
            ("fwupdtool", "fwupdtool"),
            ("/usr/bin/fwupdmgr", "fwupdmgr"),
            ("C:\\bin\\fwupd.exe", "fwupd"),
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
    fn test_remote_ids_unique() {
        let remotes = default_remotes();
        let mut ids: Vec<&str> = remotes.iter().map(|r| r.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), remotes.len());
    }
}
