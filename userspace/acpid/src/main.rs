#![deny(clippy::all)]

//! acpid — Slate OS ACPI event daemon
//!
//! Multi-personality binary for handling ACPI events (power button, lid,
//! AC adapter, thermal, sleep/wake).
//! Detected via argv[0]:
//!
//! - `acpid` (default) — ACPI event daemon
//! - `acpi_listen` — listen for and display ACPI events
//! - `acpi` — show battery/thermal/AC adapter status

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _ACPID_SOCKET: &str = "/var/run/acpid.socket";
const _ACPID_CONF_DIR: &str = "/etc/acpi/events";
const _ACPID_ACTION_DIR: &str = "/etc/acpi/actions";
const _PROC_ACPI: &str = "/proc/acpi";
const _SYS_POWER: &str = "/sys/power/state";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct AcpiEvent {
    device_class: String,
    bus_id: String,
    event_type: u32,
    event_data: u32,
}

impl std::fmt::Display for AcpiEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} {:08x} {:08x}",
            self.device_class, self.bus_id,
            self.event_type, self.event_data)
    }
}

#[derive(Clone, Debug)]
struct EventRule {
    name: String,
    event_pattern: String,
    action: String,
}

#[derive(Clone, Debug)]
struct BatteryInfo {
    name: String,
    present: bool,
    state: BatteryState,
    percent: u32,
    _voltage_mv: u32,
    _rate_mw: u32,
    remaining_min: Option<u32>,
    _design_capacity_mah: u32,
    _last_full_mah: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum BatteryState {
    _Charging,
    Discharging,
    _Full,
    _NotCharging,
    _Unknown,
}

impl std::fmt::Display for BatteryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Charging => write!(f, "Charging"),
            Self::Discharging => write!(f, "Discharging"),
            Self::_Full => write!(f, "Full"),
            Self::_NotCharging => write!(f, "Not charging"),
            Self::_Unknown => write!(f, "Unknown"),
        }
    }
}

#[derive(Clone, Debug)]
struct ThermalZone {
    name: String,
    temp_mc: i32, // millicelsius
    _trip_points: Vec<TripPoint>,
    _cooling_devices: Vec<String>,
}

#[derive(Clone, Debug)]
struct TripPoint {
    _name: String,
    _temp_mc: i32,
    _trip_type: String,
}

#[derive(Clone, Debug)]
struct AcAdapter {
    name: String,
    online: bool,
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_batteries() -> Vec<BatteryInfo> {
    vec![
        BatteryInfo {
            name: "BAT0".to_string(),
            present: true,
            state: BatteryState::Discharging,
            percent: 72,
            _voltage_mv: 11400,
            _rate_mw: 15000,
            remaining_min: Some(180),
            _design_capacity_mah: 5000,
            _last_full_mah: 4800,
        },
    ]
}

fn read_thermal_zones() -> Vec<ThermalZone> {
    vec![
        ThermalZone {
            name: "thermal_zone0 (x86_pkg_temp)".to_string(),
            temp_mc: 52000,
            _trip_points: vec![
                TripPoint {
                    _name: "passive".to_string(),
                    _temp_mc: 85000,
                    _trip_type: "passive".to_string(),
                },
                TripPoint {
                    _name: "critical".to_string(),
                    _temp_mc: 100000,
                    _trip_type: "critical".to_string(),
                },
            ],
            _cooling_devices: vec!["Fan0".to_string()],
        },
        ThermalZone {
            name: "thermal_zone1 (acpitz)".to_string(),
            temp_mc: 45000,
            _trip_points: vec![],
            _cooling_devices: vec![],
        },
    ]
}

fn read_ac_adapters() -> Vec<AcAdapter> {
    vec![
        AcAdapter {
            name: "AC0".to_string(),
            online: true,
        },
    ]
}

fn default_event_rules() -> Vec<EventRule> {
    vec![
        EventRule {
            name: "power-button".to_string(),
            event_pattern: "button/power.*".to_string(),
            action: "/etc/acpi/actions/power.sh".to_string(),
        },
        EventRule {
            name: "lid-close".to_string(),
            event_pattern: "button/lid LID close".to_string(),
            action: "/etc/acpi/actions/lid.sh".to_string(),
        },
        EventRule {
            name: "lid-open".to_string(),
            event_pattern: "button/lid LID open".to_string(),
            action: "/etc/acpi/actions/lid.sh".to_string(),
        },
        EventRule {
            name: "ac-adapter".to_string(),
            event_pattern: "ac_adapter.*".to_string(),
            action: "/etc/acpi/actions/ac.sh".to_string(),
        },
        EventRule {
            name: "battery".to_string(),
            event_pattern: "battery.*".to_string(),
            action: "/etc/acpi/actions/battery.sh".to_string(),
        },
    ]
}

fn sample_events() -> Vec<AcpiEvent> {
    vec![
        AcpiEvent {
            device_class: "button/power".to_string(),
            bus_id: "PWRB".to_string(),
            event_type: 0x80,
            event_data: 0x01,
        },
        AcpiEvent {
            device_class: "button/lid".to_string(),
            bus_id: "LID".to_string(),
            event_type: 0x80,
            event_data: 0x00,
        },
        AcpiEvent {
            device_class: "ac_adapter".to_string(),
            bus_id: "AC0".to_string(),
            event_type: 0x80,
            event_data: 0x01,
        },
        AcpiEvent {
            device_class: "battery".to_string(),
            bus_id: "BAT0".to_string(),
            event_type: 0x80,
            event_data: 0x00,
        },
    ]
}

// ── acpid personality ─────────────────────────────────────────────────

fn run_acpid(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--help-check".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: acpid [OPTIONS]");
            println!();
            println!("ACPI event daemon. Listens for ACPI events and dispatches actions.");
            println!();
            println!("Options:");
            println!("  -f, --foreground   Run in foreground (don't daemonize)");
            println!("  -l, --logevents    Log all events to syslog");
            println!("  -c DIR             Config directory (default: {})", _ACPID_CONF_DIR);
            println!("  -s SOCKET          Socket path (default: {})", _ACPID_SOCKET);
            println!("  -d                 Debug mode (implies -f -l)");
            println!("  --status           Show daemon status");
            println!("  --rules            Show loaded event rules");
            println!("  --version          Show version");
            0
        }
        "--version" | "-v" => {
            println!("acpid 0.1.0 (Slate OS)");
            0
        }
        "--status" | "status" => daemon_status(),
        "--rules" | "rules" => show_rules(),
        "-f" | "--foreground" | "-d" | "--daemon" | "--help-check" => run_daemon(&args),
        other => {
            // Check if it starts with - (flag for daemon mode)
            if other.starts_with('-') {
                run_daemon(&args)
            } else {
                eprintln!("acpid: unknown option '{}'", other);
                1
            }
        }
    }
}

fn run_daemon(args: &[String]) -> i32 {
    let foreground = args.iter().any(|a| a == "-f" || a == "--foreground" || a == "-d");
    let debug = args.iter().any(|a| a == "-d");
    let log_events = args.iter().any(|a| a == "-l" || a == "--logevents") || debug;

    println!("acpid: starting ACPI event daemon");
    println!("  Socket: {}", _ACPID_SOCKET);
    println!("  Config: {}", _ACPID_CONF_DIR);
    println!("  Mode: {}", if foreground { "foreground" } else { "daemon" });
    if log_events {
        println!("  Event logging: enabled");
    }
    if debug {
        println!("  Debug: enabled");
    }
    println!();

    // Load rules
    let rules = default_event_rules();
    println!("Loaded {} event rules:", rules.len());
    for r in &rules {
        println!("  {} -> {}", r.event_pattern, r.action);
    }
    println!();

    println!("acpid: listening for ACPI events (simulated)");
    println!();

    // Simulate processing a few events
    let events = sample_events();
    for event in &events {
        if log_events {
            println!("[event] {}", event);
        }
        // Find matching rule
        for r in &rules {
            if event.device_class.starts_with(r.event_pattern.split('*').next().unwrap_or("")) {
                if debug {
                    println!("  -> matched rule '{}', action: {}", r.name, r.action);
                }
                break;
            }
        }
    }

    println!();
    println!("acpid: daemon ready (simulated, would block on /proc/acpi/event)");
    0
}

fn daemon_status() -> i32 {
    println!("acpid status:");
    println!("  Running: yes (simulated)");
    println!("  Socket: {}", _ACPID_SOCKET);
    println!("  Connected clients: 1");
    println!("  Events processed: 42");
    println!("  Rules loaded: {}", default_event_rules().len());
    0
}

fn show_rules() -> i32 {
    let rules = default_event_rules();
    println!("ACPI Event Rules ({} loaded):", rules.len());
    println!();
    println!("{:<20} {:<30} Action", "Name", "Pattern");
    println!("{}", "-".repeat(80));
    for r in &rules {
        println!("{:<20} {:<30} {}", r.name, r.event_pattern, r.action);
    }
    0
}

// ── acpi_listen personality ───────────────────────────────────────────

fn run_listen(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "listen".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: acpi_listen [OPTIONS]");
            println!();
            println!("Listen for and display ACPI events from acpid.");
            println!();
            println!("Options:");
            println!("  -c COUNT    Exit after receiving COUNT events");
            println!("  -t SECS     Timeout after SECS seconds");
            println!("  -s SOCKET   Connect to SOCKET (default: {})", _ACPID_SOCKET);
            0
        }
        _ => {
            let count: Option<usize> = args.iter()
                .position(|a| a == "-c")
                .and_then(|i| args.get(i + 1))
                .and_then(|s| s.parse().ok());

            println!("Listening for ACPI events (Ctrl+C to stop)...");
            println!();

            let events = sample_events();
            let limit = count.unwrap_or(events.len());

            for event in events.iter().take(limit) {
                println!("{}", event);
            }

            if count.is_some() {
                println!();
                println!("Received {} events, exiting.", limit);
            }
            0
        }
    }
}

// ── acpi personality ──────────────────────────────────────────────────

fn run_acpi(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--everything".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: acpi [OPTIONS]");
            println!();
            println!("Show ACPI information (battery, thermal, AC adapter).");
            println!();
            println!("Options:");
            println!("  -b, --battery     Show battery status");
            println!("  -t, --thermal     Show thermal zones");
            println!("  -a, --ac-adapter  Show AC adapter status");
            println!("  -V, --everything  Show everything (default)");
            println!("  -i, --details     Show extra details");
            println!("  -s, --show-empty  Show empty slots too");
            println!("  --version         Show version");
            0
        }
        "--version" => {
            println!("acpi 0.1.0 (Slate OS)");
            0
        }
        "-b" | "--battery" => show_battery(false),
        "-t" | "--thermal" => show_thermal(false),
        "-a" | "--ac-adapter" => show_ac(),
        "-i" | "--details" => {
            show_battery(true);
            show_thermal(true);
            show_ac();
            0
        }
        "-V" | "--everything" => {
            show_battery(false);
            show_thermal(false);
            show_ac();
            0
        }
        other => {
            if other.starts_with('-') {
                // Try to handle combined flags
                show_battery(false);
                show_thermal(false);
                show_ac();
                0
            } else {
                eprintln!("acpi: unknown option '{}'", other);
                1
            }
        }
    }
}

fn show_battery(details: bool) -> i32 {
    let batteries = read_batteries();
    for bat in &batteries {
        if !bat.present {
            println!("{}: absent", bat.name);
            continue;
        }

        let time_str = match bat.remaining_min {
            Some(min) => format!("{:02}:{:02}:00 remaining", min / 60, min % 60),
            None => "rate information unavailable".to_string(),
        };

        println!("{}: {}, {}%, {}", bat.name, bat.state, bat.percent, time_str);

        if details {
            println!("  Design capacity:    {} mAh", bat._design_capacity_mah);
            println!("  Last full capacity: {} mAh", bat._last_full_mah);
            println!("  Present voltage:    {} mV", bat._voltage_mv);
            println!("  Discharge rate:     {} mW", bat._rate_mw);
            let health = (bat._last_full_mah as f64 / bat._design_capacity_mah as f64) * 100.0;
            println!("  Battery health:     {:.1}%", health);
        }
    }
    0
}

fn show_thermal(details: bool) -> i32 {
    let zones = read_thermal_zones();
    for zone in &zones {
        let temp_c = zone.temp_mc as f64 / 1000.0;
        println!("{}: {:.1} degrees C", zone.name, temp_c);

        if details {
            for tp in &zone._trip_points {
                let trip_c = tp._temp_mc as f64 / 1000.0;
                println!("  {} trip point: {:.1} degrees C ({})", tp._name, trip_c, tp._trip_type);
            }
            if !zone._cooling_devices.is_empty() {
                println!("  Cooling: {}", zone._cooling_devices.join(", "));
            }
        }
    }
    0
}

fn show_ac() -> i32 {
    let adapters = read_ac_adapters();
    for ac in &adapters {
        println!("{}: {}", ac.name, if ac.online { "on-line" } else { "off-line" });
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("acpid");
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
        "acpi_listen" => run_listen(rest),
        "acpi" => run_acpi(rest),
        _ => run_acpid(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_info() {
        let bats = read_batteries();
        assert_eq!(bats.len(), 1);
        assert!(bats[0].present);
        assert_eq!(bats[0].state, BatteryState::Discharging);
        assert_eq!(bats[0].percent, 72);
    }

    #[test]
    fn test_thermal_zones() {
        let zones = read_thermal_zones();
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].temp_mc, 52000);
        assert!(zones[0]._trip_points.len() >= 2);
    }

    #[test]
    fn test_ac_adapters() {
        let acs = read_ac_adapters();
        assert_eq!(acs.len(), 1);
        assert!(acs[0].online);
    }

    #[test]
    fn test_event_display() {
        let event = AcpiEvent {
            device_class: "button/power".to_string(),
            bus_id: "PWRB".to_string(),
            event_type: 0x80,
            event_data: 0x01,
        };
        let s = format!("{}", event);
        assert!(s.contains("button/power"));
        assert!(s.contains("PWRB"));
        assert!(s.contains("00000080"));
    }

    #[test]
    fn test_battery_state_display() {
        assert_eq!(format!("{}", BatteryState::_Charging), "Charging");
        assert_eq!(format!("{}", BatteryState::Discharging), "Discharging");
        assert_eq!(format!("{}", BatteryState::_Full), "Full");
    }

    #[test]
    fn test_default_event_rules() {
        let rules = default_event_rules();
        assert!(rules.len() >= 4);
        assert!(rules.iter().any(|r| r.name == "power-button"));
        assert!(rules.iter().any(|r| r.name == "lid-close"));
        assert!(rules.iter().any(|r| r.name == "ac-adapter"));
    }

    #[test]
    fn test_sample_events() {
        let events = sample_events();
        assert!(events.len() >= 3);
        assert!(events.iter().any(|e| e.device_class.contains("power")));
        assert!(events.iter().any(|e| e.device_class.contains("lid")));
        assert!(events.iter().any(|e| e.device_class.contains("battery")));
    }

    #[test]
    fn test_thermal_temp_conversion() {
        let zone = &read_thermal_zones()[0];
        let temp_c = zone.temp_mc as f64 / 1000.0;
        assert!((temp_c - 52.0).abs() < 0.001);
    }

    #[test]
    fn test_battery_health_calc() {
        let bat = &read_batteries()[0];
        let health = (bat._last_full_mah as f64 / bat._design_capacity_mah as f64) * 100.0;
        assert!(health > 90.0 && health <= 100.0);
    }
}
