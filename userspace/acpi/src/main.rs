// Slate OS acpi — power management information
//
// Usage:
//   acpi [OPTIONS]
//
// This answered to `acpid` as a second personality until 2026-09-10. That
// personality printed "starting ACPI event daemon", the socket and log paths it
// would have used, and "waiting for events...", having opened none of them, and
// exited 0. design-decisions.md 1006 (Decided by: Operator) deletes commands
// that report success for work they never did, and `userspace/acpid` -- the
// crate that produced the binary -- went with the other 2,284. Removing the
// personality is the other half of that: a name nothing can run is still a
// claim while a program answers to it.
//
// If a real ACPI event daemon is written, it gets its own crate. One
// executable answering to several tool names has to hold the union of all
// their capabilities, which is the least-privilege problem this OS exists to
// avoid (coreutils-canonical-answer.md).

#![cfg_attr(not(test), no_main)]
// BatteryInfo::capacity_now and ::current_now are part of the
// /sys/class/power_supply/BAT0/{charge_now,current_now} sysfs surface
// the real acpi tool must consume to compute instantaneous discharge
// rate and time-to-empty. Dead-code lint cannot see across that future
// boundary.
#![allow(dead_code)]

#[cfg(not(test))]
use std::env;
use std::io::{self, Write};
use std::path::Path;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct BatteryInfo {
    name: String,
    present: bool,
    status: BatteryStatus,
    percentage: Option<u32>,
    capacity_full: Option<u64>,  // microWh
    capacity_now: Option<u64>,   // microWh
    voltage_now: Option<u64>,    // microV
    current_now: Option<i64>,    // microA (negative = discharging)
    time_remaining: Option<u64>, // minutes
    technology: String,
    model: String,
    manufacturer: String,
    serial: String,
    cycle_count: Option<u32>,
    design_capacity: Option<u64>, // microWh
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatteryStatus {
    Charging,
    Discharging,
    Full,
    NotCharging,
    Unknown,
}

impl BatteryStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
            Self::Full => "Full",
            Self::NotCharging => "Not charging",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
struct AcAdapterInfo {
    name: String,
    online: bool,
}

#[derive(Debug, Clone)]
struct ThermalZone {
    name: String,
    temperature: i64, // millidegrees Celsius
    trip_points: Vec<TripPoint>,
    policy: String,
}

#[derive(Debug, Clone)]
struct TripPoint {
    kind: TripType,
    temp: i64, // millidegrees Celsius
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TripType {
    Critical,
    Hot,
    Passive,
    Active,
}

impl TripType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Critical => "critical",
            Self::Hot => "hot",
            Self::Passive => "passive",
            Self::Active => "active",
        }
    }
}

#[derive(Debug, Clone)]
struct CoolingDevice {
    name: String,
    device_type: String,
    cur_state: u32,
    max_state: u32,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Config {
    show_battery: bool,
    show_ac: bool,
    show_thermal: bool,
    show_cooling: bool,
    show_everything: bool,
    verbose: bool,
    fahrenheit: bool,
    show_details: bool,
    show_help: bool,
    show_version: bool,
}

fn parse_args(args: &[String]) -> Result<Config, String> {
    let mut cfg = Config::default();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-b" | "--battery" => cfg.show_battery = true,
            "-a" | "--ac-adapter" => cfg.show_ac = true,
            "-t" | "--thermal" => cfg.show_thermal = true,
            "-c" | "--cooling" => cfg.show_cooling = true,
            "-V" | "--everything" => cfg.show_everything = true,
            "-v" => cfg.verbose = true,
            "-f" | "--fahrenheit" => cfg.fahrenheit = true,
            "-i" | "--details" => cfg.show_details = true,
            "-h" | "--help" => cfg.show_help = true,
            "--version" => cfg.show_version = true,
            other if other.starts_with('-') => {
                return Err(format!("acpi: unknown option: {other}"));
            }
            _ => {}
        }
        i += 1;
    }

    // Default for acpi: show battery if nothing else specified
    if !cfg.show_battery
        && !cfg.show_ac
        && !cfg.show_thermal
        && !cfg.show_cooling
        && !cfg.show_everything
    {
        cfg.show_battery = true;
    }

    Ok(cfg)
}

// ---------------------------------------------------------------------------
// Sysfs parsing
// ---------------------------------------------------------------------------

fn read_sysfs_string(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_sysfs_u64(path: &Path) -> Option<u64> {
    read_sysfs_string(path)?.parse().ok()
}

fn read_sysfs_i64(path: &Path) -> Option<i64> {
    read_sysfs_string(path)?.parse().ok()
}

fn read_sysfs_u32(path: &Path) -> Option<u32> {
    read_sysfs_string(path)?.parse().ok()
}

fn read_batteries() -> Vec<BatteryInfo> {
    let mut batteries = Vec::new();
    let supply_path = Path::new("/sys/class/power_supply");

    if let Ok(entries) = std::fs::read_dir(supply_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let supply_type = read_sysfs_string(&path.join("type"));

            if supply_type.as_deref() != Some("Battery") {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();

            let present = read_sysfs_string(&path.join("present"))
                .map(|s| s == "1")
                .unwrap_or(false);

            let status_str = read_sysfs_string(&path.join("status")).unwrap_or_default();
            let status = match status_str.as_str() {
                "Charging" => BatteryStatus::Charging,
                "Discharging" => BatteryStatus::Discharging,
                "Full" => BatteryStatus::Full,
                "Not charging" => BatteryStatus::NotCharging,
                _ => BatteryStatus::Unknown,
            };

            let percentage = read_sysfs_u32(&path.join("capacity"));
            let capacity_full = read_sysfs_u64(&path.join("energy_full"));
            let capacity_now = read_sysfs_u64(&path.join("energy_now"));
            let voltage_now = read_sysfs_u64(&path.join("voltage_now"));
            let current_now = read_sysfs_i64(&path.join("current_now"));
            let design_capacity = read_sysfs_u64(&path.join("energy_full_design"));
            let cycle_count = read_sysfs_u32(&path.join("cycle_count"));
            let technology = read_sysfs_string(&path.join("technology")).unwrap_or_default();
            let model = read_sysfs_string(&path.join("model_name")).unwrap_or_default();
            let manufacturer = read_sysfs_string(&path.join("manufacturer")).unwrap_or_default();
            let serial = read_sysfs_string(&path.join("serial_number")).unwrap_or_default();

            // Calculate time remaining
            let time_remaining = if let (Some(now), Some(cur)) = (capacity_now, current_now) {
                if cur != 0 {
                    let hours = now as f64 / cur.unsigned_abs() as f64;
                    Some((hours * 60.0) as u64)
                } else {
                    None
                }
            } else {
                None
            };

            batteries.push(BatteryInfo {
                name,
                present,
                status,
                percentage,
                capacity_full,
                capacity_now,
                voltage_now,
                current_now,
                time_remaining,
                technology,
                model,
                manufacturer,
                serial,
                cycle_count,
                design_capacity,
            });
        }
    }

    batteries
}

fn read_ac_adapters() -> Vec<AcAdapterInfo> {
    let mut adapters = Vec::new();
    let supply_path = Path::new("/sys/class/power_supply");

    if let Ok(entries) = std::fs::read_dir(supply_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let supply_type = read_sysfs_string(&path.join("type"));

            if supply_type.as_deref() != Some("Mains") {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();

            let online = read_sysfs_string(&path.join("online"))
                .map(|s| s == "1")
                .unwrap_or(false);

            adapters.push(AcAdapterInfo { name, online });
        }
    }

    adapters
}

fn read_thermal_zones() -> Vec<ThermalZone> {
    let mut zones = Vec::new();
    let thermal_path = Path::new("/sys/class/thermal");

    if let Ok(entries) = std::fs::read_dir(thermal_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("thermal_zone") {
                continue;
            }

            let path = entry.path();
            let temperature = read_sysfs_i64(&path.join("temp")).unwrap_or(0);
            let policy = read_sysfs_string(&path.join("policy")).unwrap_or_default();

            let mut trip_points = Vec::new();
            for tp_idx in 0..20 {
                let temp_path = path.join(format!("trip_point_{tp_idx}_temp"));
                let type_path = path.join(format!("trip_point_{tp_idx}_type"));

                if let (Some(temp), Some(tp_type)) =
                    (read_sysfs_i64(&temp_path), read_sysfs_string(&type_path))
                {
                    let kind = match tp_type.as_str() {
                        "critical" => TripType::Critical,
                        "hot" => TripType::Hot,
                        "passive" => TripType::Passive,
                        _ => TripType::Active,
                    };
                    trip_points.push(TripPoint { kind, temp });
                } else {
                    break;
                }
            }

            zones.push(ThermalZone {
                name,
                temperature,
                trip_points,
                policy,
            });
        }
    }

    zones
}

fn read_cooling_devices() -> Vec<CoolingDevice> {
    let mut devices = Vec::new();
    let thermal_path = Path::new("/sys/class/thermal");

    if let Ok(entries) = std::fs::read_dir(thermal_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("cooling_device") {
                continue;
            }

            let path = entry.path();
            let device_type = read_sysfs_string(&path.join("type")).unwrap_or_default();
            let cur_state = read_sysfs_u32(&path.join("cur_state")).unwrap_or(0);
            let max_state = read_sysfs_u32(&path.join("max_state")).unwrap_or(0);

            devices.push(CoolingDevice {
                name,
                device_type,
                cur_state,
                max_state,
            });
        }
    }

    devices
}

// ---------------------------------------------------------------------------
// Temperature formatting
// ---------------------------------------------------------------------------

fn format_temp(millidegrees: i64, fahrenheit: bool) -> String {
    let celsius = millidegrees as f64 / 1000.0;
    if fahrenheit {
        let fahr = celsius * 9.0 / 5.0 + 32.0;
        format!("{fahr:.1} degrees F")
    } else {
        format!("{celsius:.1} degrees C")
    }
}

fn format_time(minutes: u64) -> String {
    let hours = minutes / 60;
    let mins = minutes % 60;
    format!("{hours:02}:{mins:02}:00")
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

fn run_acpi(cfg: &Config, writer: &mut dyn Write) -> io::Result<()> {
    let show_bat = cfg.show_battery || cfg.show_everything;
    let show_ac = cfg.show_ac || cfg.show_everything;
    let show_thermal = cfg.show_thermal || cfg.show_everything;
    let show_cooling = cfg.show_cooling || cfg.show_everything;

    let mut any_output = false;

    if show_bat {
        let batteries = read_batteries();
        if batteries.is_empty() {
            writeln!(writer, "No battery information available")?;
        }
        for bat in &batteries {
            if !bat.present {
                writeln!(writer, "{}: absent", bat.name)?;
                continue;
            }

            let pct = bat
                .percentage
                .map(|p| format!(", {p}%"))
                .unwrap_or_default();
            let time = bat
                .time_remaining
                .map(|t| format!(", {}", format_time(t)))
                .unwrap_or_default();

            writeln!(
                writer,
                "{}: {}{}{} remaining",
                bat.name,
                bat.status.as_str(),
                pct,
                time
            )?;

            if cfg.verbose || cfg.show_details {
                if !bat.technology.is_empty() {
                    writeln!(writer, "  Technology: {}", bat.technology)?;
                }
                if !bat.model.is_empty() {
                    writeln!(writer, "  Model: {}", bat.model)?;
                }
                if !bat.manufacturer.is_empty() {
                    writeln!(writer, "  Manufacturer: {}", bat.manufacturer)?;
                }
                if !bat.serial.is_empty() {
                    writeln!(writer, "  Serial: {}", bat.serial)?;
                }
                if let Some(design) = bat.design_capacity {
                    writeln!(writer, "  Design capacity: {} mWh", design / 1000)?;
                }
                if let Some(full) = bat.capacity_full {
                    writeln!(writer, "  Last full capacity: {} mWh", full / 1000)?;
                }
                if let Some(cycles) = bat.cycle_count {
                    writeln!(writer, "  Cycle count: {cycles}")?;
                }
                if let Some(voltage) = bat.voltage_now {
                    writeln!(writer, "  Voltage: {:.2} V", voltage as f64 / 1_000_000.0)?;
                }
            }

            any_output = true;
        }
    }

    if show_ac {
        let adapters = read_ac_adapters();
        if adapters.is_empty() && !any_output {
            writeln!(writer, "No AC adapter information available")?;
        }
        for ac in &adapters {
            let status = if ac.online { "on-line" } else { "off-line" };
            writeln!(writer, "Adapter {}: {status}", ac.name)?;
            any_output = true;
        }
    }

    if show_thermal {
        let zones = read_thermal_zones();
        if zones.is_empty() && !any_output {
            writeln!(writer, "No thermal information available")?;
        }
        for zone in &zones {
            writeln!(
                writer,
                "Thermal {}: ok, {}",
                zone.name,
                format_temp(zone.temperature, cfg.fahrenheit)
            )?;

            if cfg.verbose {
                for tp in &zone.trip_points {
                    writeln!(
                        writer,
                        "  trip point {} at {}",
                        tp.kind.as_str(),
                        format_temp(tp.temp, cfg.fahrenheit)
                    )?;
                }
                if !zone.policy.is_empty() {
                    writeln!(writer, "  policy: {}", zone.policy)?;
                }
            }

            any_output = true;
        }
    }

    if show_cooling {
        let devices = read_cooling_devices();
        if devices.is_empty() && !any_output {
            writeln!(writer, "No cooling device information available")?;
        }
        for dev in &devices {
            writeln!(
                writer,
                "Cooling {} {}: {}/{}",
                dev.name, dev.device_type, dev.cur_state, dev.max_state
            )?;
            any_output = true;
        }
    }

    if !any_output && !show_bat {
        writeln!(writer, "No support for device type")?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Help / version
// ---------------------------------------------------------------------------

fn print_help() {
    println!("Usage: acpi [OPTIONS]");
    println!();
    println!("Show battery, AC adapter, thermal, and cooling device status.");
    println!();
    println!("Options:");
    println!("  -b, --battery      Show battery information");
    println!("  -a, --ac-adapter   Show AC adapter information");
    println!("  -t, --thermal      Show thermal zone information");
    println!("  -c, --cooling      Show cooling device information");
    println!("  -V, --everything   Show all device categories");
    println!("  -v                 Show additional details");
    println!("  -f, --fahrenheit   Use Fahrenheit instead of Celsius");
    println!("  -i, --details      Show detailed battery information");
    println!("  -h, --help         Show this help");
    println!("  --version          Show version");
}

fn print_version() {
    println!("acpi (Slate OS) 0.1.0");
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(not(test))]
#[unsafe(no_mangle)]
pub extern "C" fn main(_argc: i32, _argv: *const *const u8) -> i32 {
    let args: Vec<String> = env::args().collect();

    let cfg = match parse_args(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };

    if cfg.show_help {
        print_help();
        return 0;
    }

    if cfg.show_version {
        print_version();
        return 0;
    }

    let stdout = io::stdout();
    let mut writer = stdout.lock();

    match run_acpi(&cfg, &mut writer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("acpi: {e}");
            1
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The `acpid` personality is gone, and its options went with it.
    ///
    /// Replaces `test_detect_personality_acpi`/`_acpid`, which asserted that
    /// argv[0] selected between two behaviours. There is one behaviour now, so
    /// what is worth pinning is that the daemon's flags are no longer accepted
    /// -- an option that is silently ignored would leave `acpid -f` looking
    /// like it had started something.
    #[test]
    fn the_acpi_daemon_options_are_no_longer_accepted() {
        for flag in [
            "--foreground",
            "-l",
            "--logevents",
            "--logfile",
            "--socketfile",
        ] {
            let args = vec!["acpi".to_string(), flag.to_string()];
            assert!(
                parse_args(&args).is_err(),
                "{flag} was accepted, so the daemon personality is not fully gone"
            );
        }
    }

    /// argv[0] no longer selects anything.
    #[test]
    fn the_program_behaves_the_same_whatever_it_is_invoked_as() {
        let as_acpi = parse_args(&["acpi".to_string()]).unwrap();
        let as_acpid = parse_args(&["/usr/bin/acpid".to_string()]).unwrap();
        assert_eq!(as_acpi.show_battery, as_acpid.show_battery);
        assert_eq!(as_acpi.show_help, as_acpid.show_help);
    }

    #[test]
    fn test_parse_args_default() {
        let args = vec!["acpi".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_battery); // default shows battery
        assert!(!cfg.show_ac);
    }

    #[test]
    fn test_parse_args_battery() {
        let args = vec!["acpi".to_string(), "-b".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_battery);
    }

    #[test]
    fn test_parse_args_ac() {
        let args = vec!["acpi".to_string(), "-a".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_ac);
    }

    #[test]
    fn test_parse_args_thermal() {
        let args = vec!["acpi".to_string(), "-t".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_thermal);
    }

    #[test]
    fn test_parse_args_cooling() {
        let args = vec!["acpi".to_string(), "-c".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_cooling);
    }

    #[test]
    fn test_parse_args_everything() {
        let args = vec!["acpi".to_string(), "-V".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_everything);
    }

    #[test]
    fn test_parse_args_verbose() {
        let args = vec!["acpi".to_string(), "-v".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.verbose);
    }

    #[test]
    fn test_parse_args_fahrenheit() {
        let args = vec!["acpi".to_string(), "-f".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.fahrenheit);
    }

    #[test]
    fn test_parse_args_help() {
        let args = vec!["acpi".to_string(), "--help".to_string()];
        let cfg = parse_args(&args).unwrap();
        assert!(cfg.show_help);
    }

    #[test]
    fn test_format_temp_celsius() {
        assert_eq!(format_temp(45000, false), "45.0 degrees C");
        assert_eq!(format_temp(0, false), "0.0 degrees C");
        assert_eq!(format_temp(100500, false), "100.5 degrees C");
    }

    #[test]
    fn test_format_temp_fahrenheit() {
        let result = format_temp(100000, true); // 100C = 212F
        assert!(result.contains("212.0"));
        assert!(result.contains("F"));
    }

    #[test]
    fn test_format_temp_freezing() {
        assert_eq!(format_temp(0, true), "32.0 degrees F");
    }

    #[test]
    fn test_format_time() {
        assert_eq!(format_time(90), "01:30:00");
        assert_eq!(format_time(0), "00:00:00");
        assert_eq!(format_time(60), "01:00:00");
        assert_eq!(format_time(125), "02:05:00");
    }

    #[test]
    fn test_battery_status_str() {
        assert_eq!(BatteryStatus::Charging.as_str(), "Charging");
        assert_eq!(BatteryStatus::Discharging.as_str(), "Discharging");
        assert_eq!(BatteryStatus::Full.as_str(), "Full");
    }

    #[test]
    fn test_trip_type_str() {
        assert_eq!(TripType::Critical.as_str(), "critical");
        assert_eq!(TripType::Hot.as_str(), "hot");
        assert_eq!(TripType::Passive.as_str(), "passive");
        assert_eq!(TripType::Active.as_str(), "active");
    }

    #[test]
    fn test_run_acpi_battery() {
        let cfg = Config {
            show_battery: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_acpi(&cfg, &mut buf).unwrap();
        // Should output something (at least "No battery" message)
        assert!(!buf.is_empty());
    }

    #[test]
    fn test_run_acpi_ac() {
        let cfg = Config {
            show_ac: true,
            show_battery: false,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_acpi(&cfg, &mut buf).unwrap();
    }

    #[test]
    fn test_run_acpi_thermal() {
        let cfg = Config {
            show_thermal: true,
            show_battery: false,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_acpi(&cfg, &mut buf).unwrap();
    }

    #[test]
    fn test_run_acpi_everything() {
        let cfg = Config {
            show_everything: true,
            show_battery: false,
            ..Default::default()
        };
        let mut buf = Vec::new();
        run_acpi(&cfg, &mut buf).unwrap();
    }

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert!(!cfg.show_battery);
        assert!(!cfg.show_ac);
        assert!(!cfg.fahrenheit);
    }
}
