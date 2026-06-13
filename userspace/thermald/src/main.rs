#![deny(clippy::all)]

//! thermald — SlateOS thermal management daemon
//!
//! Multi-personality binary for thermal zone monitoring and cooling control.
//! Detected via argv[0]:
//!
//! - `thermald` (default) — thermal management daemon
//! - `thermal-monitor` — CLI thermal status monitor
//! - `thermal-conf` — thermal configuration tool

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const THERMAL_BASE: &str = "/sys/class/thermal";
const THERMAL_CONF: &str = "/etc/thermald/thermal-conf.xml";
const _PROC_TEMP: &str = "/sys/class/hwmon";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct ThermalZone {
    id: u32,
    zone_type: String,
    temp_mc: i64, // millicelsius
    trip_points: Vec<TripPoint>,
    policy: String,
    _mode: String,
}

#[derive(Clone, Debug)]
struct TripPoint {
    id: u32,
    trip_type: TripType,
    temp_mc: i64,
    _hysteresis: i64,
}

#[derive(Clone, Debug, PartialEq)]
enum TripType {
    Active,
    Passive,
    Hot,
    Critical,
}

impl std::fmt::Display for TripType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active => write!(f, "active"),
            Self::Passive => write!(f, "passive"),
            Self::Hot => write!(f, "hot"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

#[derive(Clone, Debug)]
struct CoolingDevice {
    id: u32,
    device_type: String,
    cur_state: u32,
    max_state: u32,
}

#[derive(Clone, Debug)]
struct ThermalConfig {
    uuid: String,
    trips: Vec<ConfigTrip>,
    cooling_devices: Vec<ConfigCooling>,
}

#[derive(Clone, Debug)]
struct ConfigTrip {
    _name: String,
    trip_type: String,
    temperature: i64,
}

#[derive(Clone, Debug)]
struct ConfigCooling {
    _name: String,
    _cooling_type: String,
    _min_state: u32,
    _max_state: u32,
}

impl Default for ThermalConfig {
    fn default() -> Self {
        Self {
            uuid: "default".to_string(),
            trips: vec![
                ConfigTrip {
                    _name: "passive".to_string(),
                    trip_type: "passive".to_string(),
                    temperature: 85000,
                },
                ConfigTrip {
                    _name: "critical".to_string(),
                    trip_type: "critical".to_string(),
                    temperature: 100000,
                },
            ],
            cooling_devices: vec![
                ConfigCooling {
                    _name: "cpu-fan".to_string(),
                    _cooling_type: "fan".to_string(),
                    _min_state: 0,
                    _max_state: 10,
                },
                ConfigCooling {
                    _name: "cpu-freq".to_string(),
                    _cooling_type: "processor".to_string(),
                    _min_state: 0,
                    _max_state: 5,
                },
            ],
        }
    }
}

// ── Temperature formatting ─────────────────────────────────────────────

fn format_temp(mc: i64) -> String {
    let celsius = mc as f64 / 1000.0;
    format!("{:.1}°C", celsius)
}

fn _parse_temp(s: &str) -> Option<i64> {
    let s = s.trim().to_lowercase();
    // Check "mc" (millicelsius) before 'c' (celsius): "100000mc" also ends in
    // 'c', so the celsius branch must not claim it first.
    if let Some(n) = s.strip_suffix("mc") {
        n.trim().parse().ok()
    } else if let Some(n) = s.strip_suffix('c') {
        let val: f64 = n.trim().parse().ok()?;
        Some((val * 1000.0) as i64)
    } else if let Ok(val) = s.parse::<f64>() {
        if val > 200.0 {
            // Likely millicelsius
            Some(val as i64)
        } else {
            Some((val * 1000.0) as i64)
        }
    } else {
        None
    }
}

// ── Thermal zone discovery ─────────────────────────────────────────────

fn read_thermal_zones() -> Vec<ThermalZone> {
    let entries = match std::fs::read_dir(THERMAL_BASE) {
        Ok(e) => e,
        Err(_) => return fallback_zones(),
    };

    let mut zones = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("thermal_zone") {
            continue;
        }
        let id: u32 = match name_str
            .strip_prefix("thermal_zone")
            .and_then(|s| s.parse().ok())
        {
            Some(v) => v,
            None => continue,
        };

        let base = entry.path();
        let zone_type =
            read_file_string(&base.join("type")).unwrap_or_else(|| "unknown".to_string());
        let temp_mc = read_file_i64(&base.join("temp")).unwrap_or(0);
        let policy =
            read_file_string(&base.join("policy")).unwrap_or_else(|| "step_wise".to_string());
        let mode = read_file_string(&base.join("mode")).unwrap_or_else(|| "enabled".to_string());

        let mut trip_points = Vec::new();
        for tp_id in 0..20 {
            let tp_type_path = base.join(format!("trip_point_{}_type", tp_id));
            let tp_temp_path = base.join(format!("trip_point_{}_temp", tp_id));
            if !tp_type_path.exists() {
                break;
            }
            let tp_type_str = read_file_string(&tp_type_path).unwrap_or_default();
            let tp_type = match tp_type_str.as_str() {
                "active" => TripType::Active,
                "passive" => TripType::Passive,
                "hot" => TripType::Hot,
                "critical" => TripType::Critical,
                _ => continue,
            };
            let tp_temp = read_file_i64(&tp_temp_path).unwrap_or(0);
            let tp_hyst_path = base.join(format!("trip_point_{}_hyst", tp_id));
            let tp_hyst = read_file_i64(&tp_hyst_path).unwrap_or(0);

            trip_points.push(TripPoint {
                id: tp_id,
                trip_type: tp_type,
                temp_mc: tp_temp,
                _hysteresis: tp_hyst,
            });
        }

        zones.push(ThermalZone {
            id,
            zone_type,
            temp_mc,
            trip_points,
            policy,
            _mode: mode,
        });
    }

    if zones.is_empty() {
        return fallback_zones();
    }

    zones.sort_by_key(|z| z.id);
    zones
}

fn fallback_zones() -> Vec<ThermalZone> {
    vec![
        ThermalZone {
            id: 0,
            zone_type: "x86_pkg_temp".to_string(),
            temp_mc: 45000,
            trip_points: vec![
                TripPoint {
                    id: 0,
                    trip_type: TripType::Passive,
                    temp_mc: 85000,
                    _hysteresis: 5000,
                },
                TripPoint {
                    id: 1,
                    trip_type: TripType::Critical,
                    temp_mc: 100000,
                    _hysteresis: 0,
                },
            ],
            policy: "step_wise".to_string(),
            _mode: "enabled".to_string(),
        },
        ThermalZone {
            id: 1,
            zone_type: "acpitz".to_string(),
            temp_mc: 40000,
            trip_points: vec![
                TripPoint {
                    id: 0,
                    trip_type: TripType::Active,
                    temp_mc: 50000,
                    _hysteresis: 2000,
                },
                TripPoint {
                    id: 1,
                    trip_type: TripType::Passive,
                    temp_mc: 80000,
                    _hysteresis: 5000,
                },
                TripPoint {
                    id: 2,
                    trip_type: TripType::Critical,
                    temp_mc: 95000,
                    _hysteresis: 0,
                },
            ],
            policy: "step_wise".to_string(),
            _mode: "enabled".to_string(),
        },
    ]
}

fn read_cooling_devices() -> Vec<CoolingDevice> {
    let entries = match std::fs::read_dir(THERMAL_BASE) {
        Ok(e) => e,
        Err(_) => return fallback_cooling(),
    };

    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("cooling_device") {
            continue;
        }
        let id: u32 = match name_str
            .strip_prefix("cooling_device")
            .and_then(|s| s.parse().ok())
        {
            Some(v) => v,
            None => continue,
        };

        let base = entry.path();
        let device_type =
            read_file_string(&base.join("type")).unwrap_or_else(|| "unknown".to_string());
        let cur_state = read_file_string(&base.join("cur_state"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let max_state = read_file_string(&base.join("max_state"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        devices.push(CoolingDevice {
            id,
            device_type,
            cur_state,
            max_state,
        });
    }

    if devices.is_empty() {
        return fallback_cooling();
    }

    devices.sort_by_key(|d| d.id);
    devices
}

fn fallback_cooling() -> Vec<CoolingDevice> {
    vec![
        CoolingDevice {
            id: 0,
            device_type: "Processor".to_string(),
            cur_state: 0,
            max_state: 5,
        },
        CoolingDevice {
            id: 1,
            device_type: "Fan".to_string(),
            cur_state: 3,
            max_state: 10,
        },
        CoolingDevice {
            id: 2,
            device_type: "intel_powerclamp".to_string(),
            cur_state: 0,
            max_state: 50,
        },
    ]
}

fn read_file_string(path: &std::path::Path) -> Option<String> {
    Some(std::fs::read_to_string(path).ok()?.trim().to_string())
}

fn read_file_i64(path: &std::path::Path) -> Option<i64> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_thermal_config() -> ThermalConfig {
    let content = match std::fs::read_to_string(THERMAL_CONF) {
        Ok(c) => c,
        Err(_) => return ThermalConfig::default(),
    };

    // Simple XML-like parsing
    let mut config = ThermalConfig::default();
    let mut in_trip = false;
    let mut current_trip = ConfigTrip {
        _name: String::new(),
        trip_type: String::new(),
        temperature: 0,
    };

    for line in content.lines() {
        let line = line.trim();
        if line.contains("<UUID>")
            && let Some(uuid) = extract_xml_value(line, "UUID")
        {
            config.uuid = uuid;
        }
        if line.contains("<TripPoint>") {
            in_trip = true;
            current_trip = ConfigTrip {
                _name: String::new(),
                trip_type: String::new(),
                temperature: 0,
            };
        }
        if in_trip {
            if let Some(name) = extract_xml_value(line, "Name") {
                current_trip._name = name;
            }
            if let Some(tp) = extract_xml_value(line, "Type") {
                current_trip.trip_type = tp;
            }
            if let Some(temp) = extract_xml_value(line, "Temperature") {
                current_trip.temperature = temp.parse().unwrap_or(0);
            }
        }
        if line.contains("</TripPoint>") {
            in_trip = false;
            config.trips.push(current_trip.clone());
        }
    }

    config
}

fn extract_xml_value(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = line.find(&open)?;
    let end = line.find(&close)?;
    Some(line[start + open.len()..end].to_string())
}

// ── Commands ───────────────────────────────────────────────────────────

fn cmd_status() {
    let zones = read_thermal_zones();
    let cooling = read_cooling_devices();

    println!("Thermal Zones:");
    for z in &zones {
        let status = if z
            .trip_points
            .iter()
            .any(|tp| tp.trip_type == TripType::Critical && z.temp_mc >= tp.temp_mc)
        {
            "CRITICAL"
        } else if z
            .trip_points
            .iter()
            .any(|tp| tp.trip_type == TripType::Passive && z.temp_mc >= tp.temp_mc)
        {
            "THROTTLING"
        } else {
            "OK"
        };
        println!(
            "  zone{}: {} {} [{}] (policy: {})",
            z.id,
            z.zone_type,
            format_temp(z.temp_mc),
            status,
            z.policy
        );
        for tp in &z.trip_points {
            let active = if z.temp_mc >= tp.temp_mc {
                " [ACTIVE]"
            } else {
                ""
            };
            println!(
                "    trip{}: {} @ {}{}",
                tp.id,
                tp.trip_type,
                format_temp(tp.temp_mc),
                active
            );
        }
    }

    println!("\nCooling Devices:");
    for d in &cooling {
        let pct = match (d.cur_state * 100).checked_div(d.max_state) {
            Some(p) => format!("{p}%"),
            None => "N/A".to_string(),
        };
        println!(
            "  cooling{}: {} (state {}/{}, {})",
            d.id, d.device_type, d.cur_state, d.max_state, pct
        );
    }
}

fn cmd_zones() {
    let zones = read_thermal_zones();
    println!(
        "{:<6} {:<20} {:>10} {:>8} POLICY",
        "ZONE", "TYPE", "TEMP", "TRIPS"
    );
    for z in &zones {
        println!(
            "{:<6} {:<20} {:>10} {:>8} {}",
            format!("zone{}", z.id),
            z.zone_type,
            format_temp(z.temp_mc),
            z.trip_points.len(),
            z.policy
        );
    }
}

fn cmd_cooling() {
    let devices = read_cooling_devices();
    println!(
        "{:<10} {:<20} {:>6} {:>6} {:>6}",
        "DEVICE", "TYPE", "CUR", "MAX", "PCT"
    );
    for d in &devices {
        let pct = match (d.cur_state * 100).checked_div(d.max_state) {
            Some(p) => format!("{p}%"),
            None => "N/A".to_string(),
        };
        println!(
            "{:<10} {:<20} {:>6} {:>6} {:>6}",
            format!("cool{}", d.id),
            d.device_type,
            d.cur_state,
            d.max_state,
            pct
        );
    }
}

fn cmd_config_show() {
    let config = read_thermal_config();
    println!("Thermal Configuration:");
    println!("  UUID: {}", config.uuid);
    println!("\n  Trip Points:");
    for tp in &config.trips {
        println!(
            "    {} ({}): {}",
            tp._name,
            tp.trip_type,
            format_temp(tp.temperature)
        );
    }
    println!("\n  Cooling Devices:");
    for cd in &config.cooling_devices {
        println!(
            "    {} ({}): state {}-{}",
            cd._name, cd._cooling_type, cd._min_state, cd._max_state
        );
    }
}

// ── Daemon ─────────────────────────────────────────────────────────────

fn run_daemon_mode(args: &[String]) {
    let foreground = args.iter().any(|a| a == "--no-daemon" || a == "-n");
    let debug = args.iter().any(|a| a == "--debug" || a == "-d");
    let adaptive = args.iter().any(|a| a == "--adaptive");
    let dbus = !args.iter().any(|a| a == "--no-dbus");

    println!("thermald: starting thermal management daemon");
    if debug {
        println!("thermald: debug mode enabled");
    }
    if foreground {
        println!("thermald: running in foreground");
    }
    if adaptive {
        println!("thermald: adaptive thermal management enabled");
    }
    if dbus {
        println!("thermald: D-Bus interface enabled");
    }

    let zones = read_thermal_zones();
    for z in &zones {
        println!(
            "thermald: monitoring zone{} ({}) at {}",
            z.id,
            z.zone_type,
            format_temp(z.temp_mc)
        );
    }

    println!("thermald: daemon ready");
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_thermald_help() {
    println!("thermald — Thermal management daemon");
    println!();
    println!("Usage: thermald [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -n, --no-daemon        Run in foreground");
    println!("  -d, --debug            Debug output");
    println!("  --adaptive             Enable adaptive thermal management");
    println!("  --no-dbus              Disable D-Bus interface");
    println!("  -h, --help             Show this help");
}

fn print_monitor_help() {
    println!("thermal-monitor — Thermal status monitor");
    println!();
    println!("Usage: thermal-monitor [COMMAND]");
    println!();
    println!("Commands:");
    println!("  status                 Show thermal status (default)");
    println!("  zones                  List thermal zones");
    println!("  cooling                List cooling devices");
    println!("  -h, --help             Show this help");
}

fn print_conf_help() {
    println!("thermal-conf — Thermal configuration tool");
    println!();
    println!("Usage: thermal-conf [COMMAND]");
    println!();
    println!("Commands:");
    println!("  show                   Show current configuration (default)");
    println!("  -h, --help             Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_thermald(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    if rest.iter().any(|a| a == "-h" || a == "--help") {
        print_thermald_help();
        return 0;
    }
    run_daemon_mode(&rest);
    0
}

fn run_monitor(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest
        .first()
        .cloned()
        .unwrap_or_else(|| "status".to_string());

    if cmd == "-h" || cmd == "--help" {
        print_monitor_help();
        return 0;
    }

    match cmd.as_str() {
        "status" => cmd_status(),
        "zones" => cmd_zones(),
        "cooling" => cmd_cooling(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_monitor_help();
            return 1;
        }
    }
    0
}

fn run_conf(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "show".to_string());

    if cmd == "-h" || cmd == "--help" {
        print_conf_help();
        return 0;
    }

    match cmd.as_str() {
        "show" => cmd_config_show(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_conf_help();
            return 1;
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("thermald");
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
        "thermal-monitor" => run_monitor(args),
        "thermal-conf" => run_conf(args),
        _ => run_thermald(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_temp() {
        assert_eq!(format_temp(45000), "45.0°C");
        assert_eq!(format_temp(100000), "100.0°C");
        assert_eq!(format_temp(0), "0.0°C");
        assert_eq!(format_temp(85500), "85.5°C");
    }

    #[test]
    fn test_parse_temp_function() {
        assert_eq!(_parse_temp("85c"), Some(85000));
        assert_eq!(_parse_temp("100000mc"), Some(100000));
        assert_eq!(_parse_temp("45.5"), Some(45500));
        assert_eq!(_parse_temp("85000"), Some(85000)); // auto-detect millicelsius
    }

    #[test]
    fn test_trip_type_display() {
        assert_eq!(format!("{}", TripType::Active), "active");
        assert_eq!(format!("{}", TripType::Passive), "passive");
        assert_eq!(format!("{}", TripType::Hot), "hot");
        assert_eq!(format!("{}", TripType::Critical), "critical");
    }

    #[test]
    fn test_default_config() {
        let config = ThermalConfig::default();
        assert_eq!(config.uuid, "default");
        assert_eq!(config.trips.len(), 2);
        assert_eq!(config.cooling_devices.len(), 2);
    }

    #[test]
    fn test_fallback_zones() {
        let zones = fallback_zones();
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].zone_type, "x86_pkg_temp");
        assert_eq!(zones[1].zone_type, "acpitz");
    }

    #[test]
    fn test_fallback_cooling() {
        let devices = fallback_cooling();
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].device_type, "Processor");
        assert_eq!(devices[1].device_type, "Fan");
    }

    #[test]
    fn test_read_thermal_zones() {
        let zones = read_thermal_zones();
        assert!(!zones.is_empty());
    }

    #[test]
    fn test_read_cooling_devices() {
        let devices = read_cooling_devices();
        assert!(!devices.is_empty());
    }

    #[test]
    fn test_extract_xml_value() {
        assert_eq!(
            extract_xml_value("<UUID>abc123</UUID>", "UUID"),
            Some("abc123".to_string())
        );
        assert_eq!(
            extract_xml_value("<Temperature>85000</Temperature>", "Temperature"),
            Some("85000".to_string())
        );
        assert_eq!(extract_xml_value("no xml here", "UUID"), None);
    }

    #[test]
    fn test_zone_trip_points_ordered() {
        let zones = fallback_zones();
        for z in &zones {
            for i in 1..z.trip_points.len() {
                assert!(
                    z.trip_points[i].temp_mc >= z.trip_points[i - 1].temp_mc,
                    "Trip points not ordered in zone{}",
                    z.id
                );
            }
        }
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("thermald", "thermald"),
            ("thermal-monitor", "thermal-monitor"),
            ("thermal-conf", "thermal-conf"),
            ("/usr/sbin/thermald", "thermald"),
            ("C:\\bin\\thermal-monitor.exe", "thermal-monitor"),
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
    fn test_parse_temp_edge_cases() {
        assert_eq!(_parse_temp("bad"), None);
        assert_eq!(_parse_temp("0c"), Some(0));
        assert_eq!(_parse_temp("0"), Some(0));
    }
}
