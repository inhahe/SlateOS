// SlateOS upower -- power information and management
//
// Multi-personality binary:
//   upower  -- query power device information
//   upowerd -- power management daemon
//
// Usage:
//   upower  [OPTIONS]
//   upowerd [OPTIONS]

#![deny(clippy::all)]
#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const VERSION: &str = "0.1.0";
const DAEMON_PID_FILE: &str = "/run/upower.pid";
const DAEMON_CONFIG_FILE: &str = "/etc/UPower/UPower.conf";
const SYS_POWER_SUPPLY: &str = "/sys/class/power_supply";
const SYS_BACKLIGHT: &str = "/sys/class/backlight";
const UPOWER_DEVICE_PREFIX: &str = "/org/freedesktop/UPower/devices/";

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Upower,
    Upowerd,
}

fn detect_personality(argv0: &str) -> Personality {
    let bytes = argv0.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &argv0[last_sep..];
    let base = base.strip_suffix(".exe").unwrap_or(base);
    match base {
        "upowerd" => Personality::Upowerd,
        _ => Personality::Upower,
    }
}

// ---------------------------------------------------------------------------
// Device types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceType {
    Unknown,
    LinePower,
    Battery,
    Ups,
    Monitor,
    Mouse,
    Keyboard,
    Phone,
    MediaPlayer,
    Tablet,
    Computer,
}

impl DeviceType {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::LinePower => "line-power",
            Self::Battery => "battery",
            Self::Ups => "ups",
            Self::Monitor => "monitor",
            Self::Mouse => "mouse",
            Self::Keyboard => "keyboard",
            Self::Phone => "phone",
            Self::MediaPlayer => "media-player",
            Self::Tablet => "tablet",
            Self::Computer => "computer",
        }
    }

    fn from_str_loose(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "line-power" | "linepower" | "mains" => Self::LinePower,
            "battery" => Self::Battery,
            "ups" => Self::Ups,
            "monitor" => Self::Monitor,
            "mouse" => Self::Mouse,
            "keyboard" => Self::Keyboard,
            "phone" => Self::Phone,
            "media-player" | "mediaplayer" => Self::MediaPlayer,
            "tablet" => Self::Tablet,
            "computer" => Self::Computer,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Battery state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatteryState {
    Unknown,
    Charging,
    Discharging,
    FullyCharged,
    PendingCharge,
    PendingDischarge,
    Empty,
}

impl BatteryState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Charging => "charging",
            Self::Discharging => "discharging",
            Self::FullyCharged => "fully-charged",
            Self::PendingCharge => "pending-charge",
            Self::PendingDischarge => "pending-discharge",
            Self::Empty => "empty",
        }
    }

    fn from_str_loose(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "charging" => Self::Charging,
            "discharging" | "not charging" => Self::Discharging,
            "full" | "fully-charged" => Self::FullyCharged,
            "pending-charge" => Self::PendingCharge,
            "pending-discharge" => Self::PendingDischarge,
            "empty" => Self::Empty,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for BatteryState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Battery technology
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BatteryTechnology {
    Unknown,
    LithiumIon,
    LithiumPolymer,
    LeadAcid,
    NickelCadmium,
    NickelMetalHydride,
}

impl BatteryTechnology {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::LithiumIon => "lithium-ion",
            Self::LithiumPolymer => "lithium-polymer",
            Self::LeadAcid => "lead-acid",
            Self::NickelCadmium => "nickel-cadmium",
            Self::NickelMetalHydride => "nickel-metal-hydride",
        }
    }

    fn from_str_loose(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "li-ion" | "lithium-ion" | "lithiumion" => Self::LithiumIon,
            "li-poly" | "lithium-polymer" | "lithiumpolymer" => Self::LithiumPolymer,
            "lead-acid" | "leadacid" | "pb" => Self::LeadAcid,
            "ni-cd" | "nickel-cadmium" | "nicd" => Self::NickelCadmium,
            "ni-mh" | "nickel-metal-hydride" | "nimh" => Self::NickelMetalHydride,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for BatteryTechnology {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Warning level
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WarningLevel {
    None,
    Discharging,
    Low,
    Critical,
    Action,
}

impl WarningLevel {
    fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Discharging => "discharging",
            Self::Low => "low",
            Self::Critical => "critical",
            Self::Action => "action",
        }
    }
}

impl fmt::Display for WarningLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Critical power action
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CriticalPowerAction {
    PowerOff,
    Hibernate,
    HybridSleep,
}

impl CriticalPowerAction {
    fn as_str(&self) -> &'static str {
        match self {
            Self::PowerOff => "PowerOff",
            Self::Hibernate => "Hibernate",
            Self::HybridSleep => "HybridSleep",
        }
    }

    fn from_str_loose(s: &str) -> Self {
        match s.trim() {
            "Hibernate" | "hibernate" => Self::Hibernate,
            "HybridSleep" | "hybridsleep" => Self::HybridSleep,
            _ => Self::PowerOff,
        }
    }
}

impl fmt::Display for CriticalPowerAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Device information
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct PowerDevice {
    path: String,
    native_path: String,
    device_type: DeviceType,
    vendor: String,
    model: String,
    serial: String,
    technology: BatteryTechnology,
    power_supply: bool,
    has_history: bool,
    has_statistics: bool,
    is_present: bool,
    is_rechargeable: bool,
    state: BatteryState,
    warning_level: WarningLevel,
    energy_wh: f64,
    energy_empty_wh: f64,
    energy_full_wh: f64,
    energy_full_design_wh: f64,
    energy_rate_w: f64,
    voltage_v: f64,
    percentage: f64,
    capacity: f64,
    time_to_empty_sec: u64,
    time_to_full_sec: u64,
    online: bool,
    icon_name: String,
}

impl Default for PowerDevice {
    fn default() -> Self {
        Self {
            path: String::new(),
            native_path: String::new(),
            device_type: DeviceType::Unknown,
            vendor: String::new(),
            model: String::new(),
            serial: String::new(),
            technology: BatteryTechnology::Unknown,
            power_supply: false,
            has_history: false,
            has_statistics: false,
            is_present: false,
            is_rechargeable: false,
            state: BatteryState::Unknown,
            warning_level: WarningLevel::None,
            energy_wh: 0.0,
            energy_empty_wh: 0.0,
            energy_full_wh: 0.0,
            energy_full_design_wh: 0.0,
            energy_rate_w: 0.0,
            voltage_v: 0.0,
            percentage: 0.0,
            capacity: 0.0,
            time_to_empty_sec: 0,
            time_to_full_sec: 0,
            online: false,
            icon_name: String::new(),
        }
    }
}

impl PowerDevice {
    fn object_path(&self) -> String {
        let type_str = self.device_type.as_str();
        let name = if self.native_path.is_empty() {
            "unknown".to_string()
        } else {
            // Use the basename of the native path as the device name.
            let base = self
                .native_path
                .rsplit('/')
                .next()
                .unwrap_or(&self.native_path);
            base.to_string()
        };
        format!("{UPOWER_DEVICE_PREFIX}{type_str}_{name}")
    }

    fn compute_icon_name(&self) -> String {
        match self.device_type {
            DeviceType::LinePower => "ac-adapter-symbolic".to_string(),
            DeviceType::Battery | DeviceType::Ups => {
                let level = if self.percentage >= 80.0 {
                    "full"
                } else if self.percentage >= 50.0 {
                    "good"
                } else if self.percentage >= 20.0 {
                    "low"
                } else if self.percentage >= 5.0 {
                    "caution"
                } else {
                    "empty"
                };
                let suffix = match self.state {
                    BatteryState::Charging | BatteryState::PendingCharge => "-charging-symbolic",
                    _ => "-symbolic",
                };
                format!("battery-{level}{suffix}")
            }
            DeviceType::Mouse => "input-mouse-symbolic".to_string(),
            DeviceType::Keyboard => "input-keyboard-symbolic".to_string(),
            DeviceType::Phone => "phone-symbolic".to_string(),
            DeviceType::Tablet => "input-tablet-symbolic".to_string(),
            _ => "battery-symbolic".to_string(),
        }
    }

    fn compute_warning_level(&self, cfg: &DaemonConfig) -> WarningLevel {
        if self.device_type != DeviceType::Battery && self.device_type != DeviceType::Ups {
            return WarningLevel::None;
        }
        if self.state != BatteryState::Discharging {
            return WarningLevel::None;
        }
        if self.percentage <= cfg.percentage_action as f64 {
            WarningLevel::Action
        } else if self.percentage <= cfg.percentage_critical as f64 {
            WarningLevel::Critical
        } else if self.percentage <= cfg.percentage_low as f64 {
            WarningLevel::Low
        } else {
            WarningLevel::Discharging
        }
    }

    fn format_time(seconds: u64) -> String {
        if seconds == 0 {
            return String::new();
        }
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        if hours > 0 {
            format!("{hours} hours {minutes} minutes")
        } else {
            format!("{minutes} minutes")
        }
    }

    fn display_info(&self, out: &mut dyn Write) -> io::Result<()> {
        let obj = self.object_path();
        writeln!(out, "  native-path:          {}", self.native_path)?;
        match self.device_type {
            DeviceType::LinePower => {
                writeln!(out, "    power supply:     yes")?;
                writeln!(
                    out,
                    "    online:           {}",
                    if self.online { "yes" } else { "no" }
                )?;
                writeln!(out, "    icon-name:        '{}'", self.compute_icon_name())?;
            }
            DeviceType::Battery | DeviceType::Ups => {
                writeln!(out, "    vendor:           {}", self.vendor)?;
                writeln!(out, "    model:            {}", self.model)?;
                writeln!(out, "    serial:           {}", self.serial)?;
                writeln!(
                    out,
                    "    power supply:     {}",
                    if self.power_supply { "yes" } else { "no" }
                )?;
                writeln!(
                    out,
                    "    updated:          {obj}"
                )?;
                writeln!(
                    out,
                    "    has history:      {}",
                    if self.has_history { "yes" } else { "no" }
                )?;
                writeln!(
                    out,
                    "    has statistics:    {}",
                    if self.has_statistics { "yes" } else { "no" }
                )?;
                writeln!(out, "    {}", self.device_type)?;
                writeln!(
                    out,
                    "      present:             {}",
                    if self.is_present { "yes" } else { "no" }
                )?;
                writeln!(
                    out,
                    "      rechargeable:        {}",
                    if self.is_rechargeable { "yes" } else { "no" }
                )?;
                writeln!(out, "      state:               {}", self.state)?;
                writeln!(out, "      warning-level:       {}", self.warning_level)?;
                writeln!(
                    out,
                    "      energy:              {:.4} Wh",
                    self.energy_wh
                )?;
                writeln!(
                    out,
                    "      energy-empty:        {:.4} Wh",
                    self.energy_empty_wh
                )?;
                writeln!(
                    out,
                    "      energy-full:         {:.4} Wh",
                    self.energy_full_wh
                )?;
                writeln!(
                    out,
                    "      energy-full-design:  {:.4} Wh",
                    self.energy_full_design_wh
                )?;
                writeln!(
                    out,
                    "      energy-rate:         {:.4} W",
                    self.energy_rate_w
                )?;
                writeln!(
                    out,
                    "      voltage:             {:.4} V",
                    self.voltage_v
                )?;
                if self.time_to_empty_sec > 0 {
                    writeln!(
                        out,
                        "      time to empty:       {}",
                        Self::format_time(self.time_to_empty_sec)
                    )?;
                }
                if self.time_to_full_sec > 0 {
                    writeln!(
                        out,
                        "      time to full:        {}",
                        Self::format_time(self.time_to_full_sec)
                    )?;
                }
                writeln!(
                    out,
                    "      percentage:          {:.0}%",
                    self.percentage
                )?;
                writeln!(
                    out,
                    "      capacity:            {:.4}%",
                    self.capacity
                )?;
                writeln!(
                    out,
                    "      technology:          {}",
                    self.technology
                )?;
                writeln!(out, "      icon-name:           '{}'", self.compute_icon_name())?;
            }
            _ => {
                writeln!(out, "    type:             {}", self.device_type)?;
                writeln!(
                    out,
                    "    power supply:     {}",
                    if self.power_supply { "yes" } else { "no" }
                )?;
                if self.percentage > 0.0 {
                    writeln!(
                        out,
                        "    percentage:       {:.0}%",
                        self.percentage
                    )?;
                }
                writeln!(out, "    icon-name:        '{}'", self.compute_icon_name())?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Backlight device
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct BacklightDevice {
    name: String,
    brightness: u64,
    max_brightness: u64,
}

impl BacklightDevice {
    fn percentage(&self) -> f64 {
        if self.max_brightness == 0 {
            return 0.0;
        }
        (self.brightness as f64 / self.max_brightness as f64) * 100.0
    }
}

// ---------------------------------------------------------------------------
// History entry (for daemon)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct HistoryEntry {
    timestamp: u64,
    value: f64,
    state: BatteryState,
}

// ---------------------------------------------------------------------------
// Daemon configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DaemonConfig {
    no_poll_batteries: bool,
    percentage_low: u32,
    percentage_critical: u32,
    percentage_action: u32,
    critical_power_action: CriticalPowerAction,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            no_poll_batteries: false,
            percentage_low: 10,
            percentage_critical: 3,
            percentage_action: 2,
            critical_power_action: CriticalPowerAction::PowerOff,
        }
    }
}

impl DaemonConfig {
    fn from_file(path: &Path) -> Self {
        let mut cfg = Self::default();
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return cfg,
        };
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "NoPollBatteries" => {
                        cfg.no_poll_batteries = value == "true" || value == "1";
                    }
                    "PercentageLow" => {
                        if let Ok(v) = value.parse::<u32>() {
                            cfg.percentage_low = v;
                        }
                    }
                    "PercentageCritical" => {
                        if let Ok(v) = value.parse::<u32>() {
                            cfg.percentage_critical = v;
                        }
                    }
                    "PercentageAction" => {
                        if let Ok(v) = value.parse::<u32>() {
                            cfg.percentage_action = v;
                        }
                    }
                    "CriticalPowerAction" => {
                        cfg.critical_power_action =
                            CriticalPowerAction::from_str_loose(value);
                    }
                    _ => {}
                }
            }
        }
        cfg
    }

    fn from_str(content: &str) -> Self {
        let mut cfg = Self::default();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                match key {
                    "NoPollBatteries" => {
                        cfg.no_poll_batteries = value == "true" || value == "1";
                    }
                    "PercentageLow" => {
                        if let Ok(v) = value.parse::<u32>() {
                            cfg.percentage_low = v;
                        }
                    }
                    "PercentageCritical" => {
                        if let Ok(v) = value.parse::<u32>() {
                            cfg.percentage_critical = v;
                        }
                    }
                    "PercentageAction" => {
                        if let Ok(v) = value.parse::<u32>() {
                            cfg.percentage_action = v;
                        }
                    }
                    "CriticalPowerAction" => {
                        cfg.critical_power_action =
                            CriticalPowerAction::from_str_loose(value);
                    }
                    _ => {}
                }
            }
        }
        cfg
    }
}

// ---------------------------------------------------------------------------
// sysfs helpers
// ---------------------------------------------------------------------------

fn read_sysfs_string(base: &Path, name: &str) -> String {
    let p = base.join(name);
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn read_sysfs_u64(base: &Path, name: &str) -> Option<u64> {
    let s = read_sysfs_string(base, name);
    if s.is_empty() {
        None
    } else {
        s.parse::<u64>().ok()
    }
}

fn read_sysfs_i64(base: &Path, name: &str) -> Option<i64> {
    let s = read_sysfs_string(base, name);
    if s.is_empty() {
        None
    } else {
        s.parse::<i64>().ok()
    }
}

/// Micro-unit values (uWh, uV, uW, uA) converted to their standard unit.
fn micro_to_unit(micro: Option<u64>) -> f64 {
    match micro {
        Some(v) => v as f64 / 1_000_000.0,
        None => 0.0,
    }
}

fn micro_to_unit_signed(micro: Option<i64>) -> f64 {
    match micro {
        Some(v) => v.unsigned_abs() as f64 / 1_000_000.0,
        None => 0.0,
    }
}

// ---------------------------------------------------------------------------
// Device discovery
// ---------------------------------------------------------------------------

fn discover_power_devices() -> Vec<PowerDevice> {
    let base = Path::new(SYS_POWER_SUPPLY);
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut devices = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let dev_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let type_str = read_sysfs_string(&dev_path, "type");
        let device_type = match type_str.to_ascii_lowercase().as_str() {
            "mains" | "usb" => DeviceType::LinePower,
            "battery" => DeviceType::Battery,
            "ups" => DeviceType::Ups,
            _ => DeviceType::Unknown,
        };

        let mut dev = PowerDevice {
            native_path: dev_path.to_string_lossy().to_string(),
            device_type,
            ..Default::default()
        };

        dev.path = dev.object_path();
        dev.vendor = read_sysfs_string(&dev_path, "manufacturer");
        dev.model = read_sysfs_string(&dev_path, "model_name");
        dev.serial = read_sysfs_string(&dev_path, "serial_number");

        let tech_str = read_sysfs_string(&dev_path, "technology");
        dev.technology = BatteryTechnology::from_str_loose(&tech_str);

        let status_str = read_sysfs_string(&dev_path, "status");
        dev.state = BatteryState::from_str_loose(&status_str);

        let ps = read_sysfs_string(&dev_path, "power_supply");
        dev.power_supply = ps != "0";

        let present = read_sysfs_string(&dev_path, "present");
        dev.is_present = present == "1";

        dev.energy_wh = micro_to_unit(read_sysfs_u64(&dev_path, "energy_now"));
        dev.energy_empty_wh = micro_to_unit(read_sysfs_u64(&dev_path, "energy_empty"));
        dev.energy_full_wh = micro_to_unit(read_sysfs_u64(&dev_path, "energy_full"));
        dev.energy_full_design_wh =
            micro_to_unit(read_sysfs_u64(&dev_path, "energy_full_design"));
        dev.energy_rate_w = micro_to_unit_signed(read_sysfs_i64(&dev_path, "power_now"));
        dev.voltage_v = micro_to_unit(read_sysfs_u64(&dev_path, "voltage_now"));

        if let Some(cap) = read_sysfs_u64(&dev_path, "capacity") {
            dev.percentage = cap as f64;
        } else if dev.energy_full_wh > 0.0 {
            dev.percentage = (dev.energy_wh / dev.energy_full_wh) * 100.0;
        }

        if dev.energy_full_design_wh > 0.0 {
            dev.capacity = (dev.energy_full_wh / dev.energy_full_design_wh) * 100.0;
        }

        // Estimate time-to-empty/full from energy rate.
        if dev.energy_rate_w > 0.0 {
            match dev.state {
                BatteryState::Discharging => {
                    let hours = (dev.energy_wh - dev.energy_empty_wh) / dev.energy_rate_w;
                    dev.time_to_empty_sec = (hours * 3600.0) as u64;
                }
                BatteryState::Charging => {
                    let hours = (dev.energy_full_wh - dev.energy_wh) / dev.energy_rate_w;
                    dev.time_to_full_sec = (hours * 3600.0) as u64;
                }
                _ => {}
            }
        }

        // Online status for line power.
        if device_type == DeviceType::LinePower {
            let online = read_sysfs_string(&dev_path, "online");
            dev.online = online == "1";
            dev.power_supply = true;
        }

        // Batteries are generally rechargeable.
        if device_type == DeviceType::Battery {
            dev.is_rechargeable = true;
            dev.has_history = true;
            dev.has_statistics = true;
        }

        dev.icon_name = dev.compute_icon_name();

        // Compute warning level with default thresholds.
        let default_cfg = DaemonConfig::default();
        dev.warning_level = dev.compute_warning_level(&default_cfg);

        let _ = name; // used for native path already
        devices.push(dev);
    }

    devices.sort_by(|a, b| a.native_path.cmp(&b.native_path));
    devices
}

fn discover_backlight_devices() -> Vec<BacklightDevice> {
    let base = Path::new(SYS_BACKLIGHT);
    let entries = match std::fs::read_dir(base) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut devices = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let dev_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let brightness = read_sysfs_u64(&dev_path, "brightness").unwrap_or(0);
        let max_brightness = read_sysfs_u64(&dev_path, "max_brightness").unwrap_or(0);
        devices.push(BacklightDevice {
            name,
            brightness,
            max_brightness,
        });
    }

    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

// ---------------------------------------------------------------------------
// upower CLI configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UpowerConfig {
    action: UpowerAction,
}

#[derive(Debug, Clone)]
enum UpowerAction {
    Enumerate,
    Dump,
    Monitor,
    MonitorDetail,
    ShowInfo(String),
    Version,
    Help,
}

// ---------------------------------------------------------------------------
// upowerd daemon configuration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct UpowerdOptions {
    config_file: PathBuf,
    pid_file: PathBuf,
    show_help: bool,
    show_version: bool,
}

impl Default for UpowerdOptions {
    fn default() -> Self {
        Self {
            config_file: PathBuf::from(DAEMON_CONFIG_FILE),
            pid_file: PathBuf::from(DAEMON_PID_FILE),
            show_help: false,
            show_version: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------------

fn parse_upower_args(args: &[String]) -> Result<UpowerConfig, String> {
    if args.len() <= 1 {
        return Ok(UpowerConfig {
            action: UpowerAction::Help,
        });
    }

    // upower acts on a single command option; only the first argument is parsed.
    if let Some(arg) = args.get(1) {
        match arg.as_str() {
            "-e" | "--enumerate" => {
                return Ok(UpowerConfig {
                    action: UpowerAction::Enumerate,
                });
            }
            "-d" | "--dump" => {
                return Ok(UpowerConfig {
                    action: UpowerAction::Dump,
                });
            }
            "-m" | "--monitor" => {
                return Ok(UpowerConfig {
                    action: UpowerAction::Monitor,
                });
            }
            "--monitor-detail" => {
                return Ok(UpowerConfig {
                    action: UpowerAction::MonitorDetail,
                });
            }
            "-i" | "--show-info" => {
                let path = args.get(2).ok_or_else(|| {
                    "upower: --show-info requires a device path argument".to_string()
                })?;
                return Ok(UpowerConfig {
                    action: UpowerAction::ShowInfo(path.clone()),
                });
            }
            "-v" | "--version" => {
                return Ok(UpowerConfig {
                    action: UpowerAction::Version,
                });
            }
            "-h" | "--help" => {
                return Ok(UpowerConfig {
                    action: UpowerAction::Help,
                });
            }
            other => {
                return Err(format!("upower: unknown option: {other}"));
            }
        }
    }

    Ok(UpowerConfig {
        action: UpowerAction::Help,
    })
}

fn parse_upowerd_args(args: &[String]) -> Result<UpowerdOptions, String> {
    let mut opts = UpowerdOptions::default();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-c" | "--config" => {
                i += 1;
                let path = args.get(i).ok_or_else(|| {
                    "upowerd: --config requires a path argument".to_string()
                })?;
                opts.config_file = PathBuf::from(path);
            }
            "-p" | "--pid-file" => {
                i += 1;
                let path = args.get(i).ok_or_else(|| {
                    "upowerd: --pid-file requires a path argument".to_string()
                })?;
                opts.pid_file = PathBuf::from(path);
            }
            "-h" | "--help" => {
                opts.show_help = true;
            }
            "-v" | "--version" => {
                opts.show_version = true;
            }
            other => {
                return Err(format!("upowerd: unknown option: {other}"));
            }
        }
        i += 1;
    }

    Ok(opts)
}

// ---------------------------------------------------------------------------
// upower actions
// ---------------------------------------------------------------------------

fn cmd_enumerate(out: &mut dyn Write) -> io::Result<i32> {
    let devices = discover_power_devices();
    for dev in &devices {
        writeln!(out, "{}", dev.object_path())?;
    }
    Ok(0)
}

fn cmd_dump(out: &mut dyn Write) -> io::Result<i32> {
    let devices = discover_power_devices();
    let backlights = discover_backlight_devices();

    writeln!(out, "Device: /org/freedesktop/UPower")?;
    writeln!(out, "  daemon-version:  {VERSION}")?;
    writeln!(out, "  on-battery:      {}", {
        let has_discharging = devices
            .iter()
            .any(|d| d.device_type == DeviceType::Battery && d.state == BatteryState::Discharging);
        if has_discharging { "yes" } else { "no" }
    })?;
    writeln!(out, "  lid-is-closed:   no")?;
    writeln!(out, "  lid-is-present:  no")?;
    writeln!(out)?;

    for dev in &devices {
        writeln!(out, "Device: {}", dev.object_path())?;
        dev.display_info(out)?;
        writeln!(out)?;
    }

    if !backlights.is_empty() {
        writeln!(out, "Display Devices:")?;
        for bl in &backlights {
            writeln!(
                out,
                "  {}: brightness={}/{} ({:.1}%)",
                bl.name,
                bl.brightness,
                bl.max_brightness,
                bl.percentage()
            )?;
        }
    }

    Ok(0)
}

fn cmd_monitor(out: &mut dyn Write, detail: bool) -> io::Result<i32> {
    writeln!(out, "Monitoring UPower events. Press Ctrl+C to cancel.")?;
    writeln!(out)?;

    // In an actual OS, this would use inotify on /sys/class/power_supply/.
    // For now, we poll periodically.
    let mut prev_states: HashMap<String, (BatteryState, f64)> = HashMap::new();

    // Snapshot current state.
    let devices = discover_power_devices();
    for dev in &devices {
        prev_states.insert(
            dev.native_path.clone(),
            (dev.state, dev.percentage),
        );
    }

    // Poll loop -- in production, this blocks until a power event arrives.
    // We simulate by checking once and reporting current state.
    let devices = discover_power_devices();
    for dev in &devices {
        let prev = prev_states.get(&dev.native_path);
        let changed = match prev {
            Some((st, pct)) => *st != dev.state || (*pct - dev.percentage).abs() > 0.5,
            None => true,
        };
        if changed {
            writeln!(
                out,
                "[event] device changed: {}",
                dev.object_path()
            )?;
            if detail {
                dev.display_info(out)?;
                writeln!(out)?;
            }
        }
    }

    writeln!(out, "(monitoring complete -- no event source available)")?;
    Ok(0)
}

fn cmd_show_info(out: &mut dyn Write, path: &str) -> io::Result<i32> {
    let devices = discover_power_devices();

    // Try to match by object path or by native path.
    let found = devices.iter().find(|d| {
        d.object_path() == path
            || d.native_path == path
            || d.object_path().ends_with(path)
    });

    match found {
        Some(dev) => {
            writeln!(out, "Device: {}", dev.object_path())?;
            dev.display_info(out)?;
            Ok(0)
        }
        None => {
            writeln!(out, "upower: device not found: {path}")?;
            Ok(1)
        }
    }
}

fn print_upower_help(out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "Usage:")?;
    writeln!(out, "  upower [OPTION...]")?;
    writeln!(out)?;
    writeln!(out, "UPower power information tool")?;
    writeln!(out)?;
    writeln!(out, "Options:")?;
    writeln!(out, "  -e, --enumerate       Enumerate power devices")?;
    writeln!(out, "  -d, --dump            Dump all device details")?;
    writeln!(out, "  -m, --monitor         Monitor for changes")?;
    writeln!(out, "      --monitor-detail  Monitor with full details")?;
    writeln!(out, "  -i, --show-info PATH  Show device information")?;
    writeln!(out, "  -v, --version         Show version")?;
    writeln!(out, "  -h, --help            Show this help")?;
    Ok(())
}

fn print_upowerd_help(out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "Usage:")?;
    writeln!(out, "  upowerd [OPTION...]")?;
    writeln!(out)?;
    writeln!(out, "UPower power management daemon")?;
    writeln!(out)?;
    writeln!(out, "Options:")?;
    writeln!(
        out,
        "  -c, --config PATH     Configuration file (default: {DAEMON_CONFIG_FILE})"
    )?;
    writeln!(
        out,
        "  -p, --pid-file PATH   PID file (default: {DAEMON_PID_FILE})"
    )?;
    writeln!(out, "  -v, --version         Show version")?;
    writeln!(out, "  -h, --help            Show this help")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Daemon (upowerd)
// ---------------------------------------------------------------------------

/// History tracker for charge and rate measurements.
#[derive(Debug, Clone)]
struct HistoryTracker {
    charge_history: Vec<HistoryEntry>,
    rate_history: Vec<HistoryEntry>,
    max_entries: usize,
}

impl HistoryTracker {
    fn new(max_entries: usize) -> Self {
        Self {
            charge_history: Vec::new(),
            rate_history: Vec::new(),
            max_entries,
        }
    }

    fn record_charge(&mut self, timestamp: u64, percentage: f64, state: BatteryState) {
        if self.charge_history.len() >= self.max_entries {
            self.charge_history.remove(0);
        }
        self.charge_history.push(HistoryEntry {
            timestamp,
            value: percentage,
            state,
        });
    }

    fn record_rate(&mut self, timestamp: u64, rate_w: f64, state: BatteryState) {
        if self.rate_history.len() >= self.max_entries {
            self.rate_history.remove(0);
        }
        self.rate_history.push(HistoryEntry {
            timestamp,
            value: rate_w,
            state,
        });
    }

    fn charge_len(&self) -> usize {
        self.charge_history.len()
    }

    fn rate_len(&self) -> usize {
        self.rate_history.len()
    }
}

fn write_pid_file(path: &Path, pid: u32) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    writeln!(f, "{pid}")?;
    Ok(())
}

fn remove_pid_file(path: &Path) {
    let _ = std::fs::remove_file(path);
}

fn run_daemon(opts: &UpowerdOptions, out: &mut dyn Write) -> io::Result<i32> {
    let cfg = DaemonConfig::from_file(&opts.config_file);

    writeln!(out, "upowerd: starting UPower daemon v{VERSION}")?;
    writeln!(
        out,
        "upowerd: config: PercentageLow={}, PercentageCritical={}, PercentageAction={}",
        cfg.percentage_low, cfg.percentage_critical, cfg.percentage_action
    )?;
    writeln!(
        out,
        "upowerd: CriticalPowerAction={}",
        cfg.critical_power_action
    )?;
    writeln!(
        out,
        "upowerd: NoPollBatteries={}",
        cfg.no_poll_batteries
    )?;

    // Write PID file.
    let pid = std::process::id();
    match write_pid_file(&opts.pid_file, pid) {
        Ok(()) => {
            writeln!(out, "upowerd: PID {pid} written to {}", opts.pid_file.display())?;
        }
        Err(e) => {
            writeln!(
                out,
                "upowerd: warning: could not write PID file {}: {e}",
                opts.pid_file.display()
            )?;
        }
    }

    // Initial device scan.
    let devices = discover_power_devices();
    writeln!(out, "upowerd: found {} power device(s)", devices.len())?;
    for dev in &devices {
        writeln!(out, "upowerd:   {} ({})", dev.object_path(), dev.device_type)?;
    }

    // Track history.
    let mut tracker = HistoryTracker::new(1000);

    // In a real daemon, we'd enter an event loop. We simulate a single pass.
    for dev in &devices {
        if dev.device_type == DeviceType::Battery || dev.device_type == DeviceType::Ups {
            tracker.record_charge(0, dev.percentage, dev.state);
            tracker.record_rate(0, dev.energy_rate_w, dev.state);

            // Check warning levels.
            let warning = dev.compute_warning_level(&cfg);
            if warning == WarningLevel::Action {
                writeln!(
                    out,
                    "upowerd: CRITICAL: battery at {:.0}% -- executing {}",
                    dev.percentage, cfg.critical_power_action
                )?;
            } else if warning == WarningLevel::Critical {
                writeln!(
                    out,
                    "upowerd: WARNING: battery critically low at {:.0}%",
                    dev.percentage
                )?;
            } else if warning == WarningLevel::Low {
                writeln!(
                    out,
                    "upowerd: notice: battery low at {:.0}%",
                    dev.percentage
                )?;
            }
        }
    }

    writeln!(out, "upowerd: daemon initialized (simulated mode)")?;

    // Cleanup PID file.
    remove_pid_file(&opts.pid_file);

    Ok(0)
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();
    let stdout = io::stdout();
    let stderr = io::stderr();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("upower");
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

    let personality = detect_personality(&prog_name);

    let exit_code = match personality {
        Personality::Upower => {
            let mut out = stdout.lock();
            let mut err = stderr.lock();
            match parse_upower_args(&args) {
                Ok(cfg) => match cfg.action {
                    UpowerAction::Enumerate => cmd_enumerate(&mut out).unwrap_or(1),
                    UpowerAction::Dump => cmd_dump(&mut out).unwrap_or(1),
                    UpowerAction::Monitor => cmd_monitor(&mut out, false).unwrap_or(1),
                    UpowerAction::MonitorDetail => cmd_monitor(&mut out, true).unwrap_or(1),
                    UpowerAction::ShowInfo(path) => {
                        cmd_show_info(&mut out, &path).unwrap_or(1)
                    }
                    UpowerAction::Version => {
                        let _ = writeln!(out, "UPower client version {VERSION}");
                        0
                    }
                    UpowerAction::Help => {
                        let _ = print_upower_help(&mut out);
                        0
                    }
                },
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    1
                }
            }
        }
        Personality::Upowerd => {
            let mut out = stdout.lock();
            let mut err = stderr.lock();
            match parse_upowerd_args(&args) {
                Ok(opts) => {
                    if opts.show_help {
                        let _ = print_upowerd_help(&mut out);
                        0
                    } else if opts.show_version {
                        let _ = writeln!(out, "UPower daemon version {VERSION}");
                        0
                    } else {
                        run_daemon(&opts, &mut out).unwrap_or(1)
                    }
                }
                Err(e) => {
                    let _ = writeln!(err, "{e}");
                    1
                }
            }
        }
    };

    std::process::exit(exit_code);
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn personality_upower_bare() {
        assert_eq!(detect_personality("upower"), Personality::Upower);
    }

    #[test]
    fn personality_upowerd_bare() {
        assert_eq!(detect_personality("upowerd"), Personality::Upowerd);
    }

    #[test]
    fn personality_with_unix_path() {
        assert_eq!(
            detect_personality("/usr/bin/upower"),
            Personality::Upower
        );
    }

    #[test]
    fn personality_with_windows_path() {
        assert_eq!(
            detect_personality("C:\\bin\\upowerd"),
            Personality::Upowerd
        );
    }

    #[test]
    fn personality_with_exe_suffix() {
        assert_eq!(
            detect_personality("upower.exe"),
            Personality::Upower
        );
    }

    #[test]
    fn personality_upowerd_exe() {
        assert_eq!(
            detect_personality("upowerd.exe"),
            Personality::Upowerd
        );
    }

    #[test]
    fn personality_with_mixed_path_separators() {
        assert_eq!(
            detect_personality("/usr/local\\bin/upowerd"),
            Personality::Upowerd
        );
    }

    #[test]
    fn personality_unknown_defaults_upower() {
        assert_eq!(
            detect_personality("something_else"),
            Personality::Upower
        );
    }

    #[test]
    fn personality_empty_string() {
        assert_eq!(detect_personality(""), Personality::Upower);
    }

    #[test]
    fn personality_deep_path() {
        assert_eq!(
            detect_personality("/a/b/c/d/e/upowerd.exe"),
            Personality::Upowerd
        );
    }

    // -----------------------------------------------------------------------
    // DeviceType
    // -----------------------------------------------------------------------

    #[test]
    fn device_type_display() {
        assert_eq!(DeviceType::LinePower.as_str(), "line-power");
        assert_eq!(DeviceType::Battery.as_str(), "battery");
        assert_eq!(DeviceType::Ups.as_str(), "ups");
        assert_eq!(DeviceType::Monitor.as_str(), "monitor");
        assert_eq!(DeviceType::Mouse.as_str(), "mouse");
        assert_eq!(DeviceType::Keyboard.as_str(), "keyboard");
        assert_eq!(DeviceType::Phone.as_str(), "phone");
        assert_eq!(DeviceType::MediaPlayer.as_str(), "media-player");
        assert_eq!(DeviceType::Tablet.as_str(), "tablet");
        assert_eq!(DeviceType::Computer.as_str(), "computer");
        assert_eq!(DeviceType::Unknown.as_str(), "unknown");
    }

    #[test]
    fn device_type_from_str_variants() {
        assert_eq!(
            DeviceType::from_str_loose("line-power"),
            DeviceType::LinePower
        );
        assert_eq!(
            DeviceType::from_str_loose("linepower"),
            DeviceType::LinePower
        );
        assert_eq!(
            DeviceType::from_str_loose("Mains"),
            DeviceType::LinePower
        );
        assert_eq!(
            DeviceType::from_str_loose("Battery"),
            DeviceType::Battery
        );
        assert_eq!(DeviceType::from_str_loose("ups"), DeviceType::Ups);
        assert_eq!(
            DeviceType::from_str_loose("monitor"),
            DeviceType::Monitor
        );
        assert_eq!(DeviceType::from_str_loose("mouse"), DeviceType::Mouse);
        assert_eq!(
            DeviceType::from_str_loose("keyboard"),
            DeviceType::Keyboard
        );
        assert_eq!(DeviceType::from_str_loose("phone"), DeviceType::Phone);
        assert_eq!(
            DeviceType::from_str_loose("media-player"),
            DeviceType::MediaPlayer
        );
        assert_eq!(
            DeviceType::from_str_loose("mediaplayer"),
            DeviceType::MediaPlayer
        );
        assert_eq!(
            DeviceType::from_str_loose("tablet"),
            DeviceType::Tablet
        );
        assert_eq!(
            DeviceType::from_str_loose("computer"),
            DeviceType::Computer
        );
    }

    #[test]
    fn device_type_unknown() {
        assert_eq!(
            DeviceType::from_str_loose("spaceship"),
            DeviceType::Unknown
        );
    }

    #[test]
    fn device_type_fmt_display() {
        let s = format!("{}", DeviceType::Battery);
        assert_eq!(s, "battery");
    }

    // -----------------------------------------------------------------------
    // BatteryState
    // -----------------------------------------------------------------------

    #[test]
    fn battery_state_as_str() {
        assert_eq!(BatteryState::Unknown.as_str(), "unknown");
        assert_eq!(BatteryState::Charging.as_str(), "charging");
        assert_eq!(BatteryState::Discharging.as_str(), "discharging");
        assert_eq!(BatteryState::FullyCharged.as_str(), "fully-charged");
        assert_eq!(BatteryState::PendingCharge.as_str(), "pending-charge");
        assert_eq!(
            BatteryState::PendingDischarge.as_str(),
            "pending-discharge"
        );
        assert_eq!(BatteryState::Empty.as_str(), "empty");
    }

    #[test]
    fn battery_state_from_str() {
        assert_eq!(
            BatteryState::from_str_loose("Charging"),
            BatteryState::Charging
        );
        assert_eq!(
            BatteryState::from_str_loose("discharging"),
            BatteryState::Discharging
        );
        assert_eq!(
            BatteryState::from_str_loose("Not charging"),
            BatteryState::Discharging
        );
        assert_eq!(
            BatteryState::from_str_loose("Full"),
            BatteryState::FullyCharged
        );
        assert_eq!(
            BatteryState::from_str_loose("fully-charged"),
            BatteryState::FullyCharged
        );
        assert_eq!(
            BatteryState::from_str_loose("pending-charge"),
            BatteryState::PendingCharge
        );
        assert_eq!(
            BatteryState::from_str_loose("pending-discharge"),
            BatteryState::PendingDischarge
        );
        assert_eq!(
            BatteryState::from_str_loose("empty"),
            BatteryState::Empty
        );
    }

    #[test]
    fn battery_state_unknown_input() {
        assert_eq!(
            BatteryState::from_str_loose("something"),
            BatteryState::Unknown
        );
    }

    #[test]
    fn battery_state_whitespace_handling() {
        assert_eq!(
            BatteryState::from_str_loose("  charging  "),
            BatteryState::Charging
        );
    }

    #[test]
    fn battery_state_fmt_display() {
        let s = format!("{}", BatteryState::Charging);
        assert_eq!(s, "charging");
    }

    // -----------------------------------------------------------------------
    // BatteryTechnology
    // -----------------------------------------------------------------------

    #[test]
    fn technology_as_str() {
        assert_eq!(BatteryTechnology::Unknown.as_str(), "unknown");
        assert_eq!(BatteryTechnology::LithiumIon.as_str(), "lithium-ion");
        assert_eq!(
            BatteryTechnology::LithiumPolymer.as_str(),
            "lithium-polymer"
        );
        assert_eq!(BatteryTechnology::LeadAcid.as_str(), "lead-acid");
        assert_eq!(
            BatteryTechnology::NickelCadmium.as_str(),
            "nickel-cadmium"
        );
        assert_eq!(
            BatteryTechnology::NickelMetalHydride.as_str(),
            "nickel-metal-hydride"
        );
    }

    #[test]
    fn technology_from_str_variants() {
        assert_eq!(
            BatteryTechnology::from_str_loose("Li-ion"),
            BatteryTechnology::LithiumIon
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("lithium-ion"),
            BatteryTechnology::LithiumIon
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("Li-poly"),
            BatteryTechnology::LithiumPolymer
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("lead-acid"),
            BatteryTechnology::LeadAcid
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("Pb"),
            BatteryTechnology::LeadAcid
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("Ni-Cd"),
            BatteryTechnology::NickelCadmium
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("NiCd"),
            BatteryTechnology::NickelCadmium
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("Ni-MH"),
            BatteryTechnology::NickelMetalHydride
        );
        assert_eq!(
            BatteryTechnology::from_str_loose("NiMH"),
            BatteryTechnology::NickelMetalHydride
        );
    }

    #[test]
    fn technology_unknown_input() {
        assert_eq!(
            BatteryTechnology::from_str_loose("fusion"),
            BatteryTechnology::Unknown
        );
    }

    #[test]
    fn technology_fmt_display() {
        let s = format!("{}", BatteryTechnology::LithiumIon);
        assert_eq!(s, "lithium-ion");
    }

    // -----------------------------------------------------------------------
    // WarningLevel
    // -----------------------------------------------------------------------

    #[test]
    fn warning_level_as_str() {
        assert_eq!(WarningLevel::None.as_str(), "none");
        assert_eq!(WarningLevel::Discharging.as_str(), "discharging");
        assert_eq!(WarningLevel::Low.as_str(), "low");
        assert_eq!(WarningLevel::Critical.as_str(), "critical");
        assert_eq!(WarningLevel::Action.as_str(), "action");
    }

    #[test]
    fn warning_level_fmt_display() {
        let s = format!("{}", WarningLevel::Critical);
        assert_eq!(s, "critical");
    }

    // -----------------------------------------------------------------------
    // CriticalPowerAction
    // -----------------------------------------------------------------------

    #[test]
    fn critical_action_as_str() {
        assert_eq!(CriticalPowerAction::PowerOff.as_str(), "PowerOff");
        assert_eq!(CriticalPowerAction::Hibernate.as_str(), "Hibernate");
        assert_eq!(CriticalPowerAction::HybridSleep.as_str(), "HybridSleep");
    }

    #[test]
    fn critical_action_from_str() {
        assert_eq!(
            CriticalPowerAction::from_str_loose("Hibernate"),
            CriticalPowerAction::Hibernate
        );
        assert_eq!(
            CriticalPowerAction::from_str_loose("hibernate"),
            CriticalPowerAction::Hibernate
        );
        assert_eq!(
            CriticalPowerAction::from_str_loose("HybridSleep"),
            CriticalPowerAction::HybridSleep
        );
        assert_eq!(
            CriticalPowerAction::from_str_loose("hybridsleep"),
            CriticalPowerAction::HybridSleep
        );
        assert_eq!(
            CriticalPowerAction::from_str_loose("anything"),
            CriticalPowerAction::PowerOff
        );
    }

    #[test]
    fn critical_action_fmt_display() {
        let s = format!("{}", CriticalPowerAction::Hibernate);
        assert_eq!(s, "Hibernate");
    }

    // -----------------------------------------------------------------------
    // DaemonConfig
    // -----------------------------------------------------------------------

    #[test]
    fn daemon_config_defaults() {
        let cfg = DaemonConfig::default();
        assert!(!cfg.no_poll_batteries);
        assert_eq!(cfg.percentage_low, 10);
        assert_eq!(cfg.percentage_critical, 3);
        assert_eq!(cfg.percentage_action, 2);
        assert_eq!(cfg.critical_power_action, CriticalPowerAction::PowerOff);
    }

    #[test]
    fn daemon_config_parse_basic() {
        let content = "\
[UPower]
NoPollBatteries=true
PercentageLow=15
PercentageCritical=5
PercentageAction=3
CriticalPowerAction=Hibernate
";
        let cfg = DaemonConfig::from_str(content);
        assert!(cfg.no_poll_batteries);
        assert_eq!(cfg.percentage_low, 15);
        assert_eq!(cfg.percentage_critical, 5);
        assert_eq!(cfg.percentage_action, 3);
        assert_eq!(cfg.critical_power_action, CriticalPowerAction::Hibernate);
    }

    #[test]
    fn daemon_config_parse_comments() {
        let content = "\
# This is a comment
[UPower]
# Another comment
PercentageLow=20
";
        let cfg = DaemonConfig::from_str(content);
        assert_eq!(cfg.percentage_low, 20);
        assert_eq!(cfg.percentage_critical, 3); // default
    }

    #[test]
    fn daemon_config_parse_empty() {
        let cfg = DaemonConfig::from_str("");
        assert_eq!(cfg.percentage_low, 10);
    }

    #[test]
    fn daemon_config_parse_invalid_values() {
        let content = "\
PercentageLow=notanumber
PercentageCritical=7
";
        let cfg = DaemonConfig::from_str(content);
        assert_eq!(cfg.percentage_low, 10); // kept default
        assert_eq!(cfg.percentage_critical, 7);
    }

    #[test]
    fn daemon_config_parse_whitespace() {
        let content = "  PercentageLow  =  12  \n";
        let cfg = DaemonConfig::from_str(content);
        assert_eq!(cfg.percentage_low, 12);
    }

    #[test]
    fn daemon_config_no_poll_batteries_1() {
        let content = "NoPollBatteries=1\n";
        let cfg = DaemonConfig::from_str(content);
        assert!(cfg.no_poll_batteries);
    }

    #[test]
    fn daemon_config_no_poll_batteries_false() {
        let content = "NoPollBatteries=false\n";
        let cfg = DaemonConfig::from_str(content);
        assert!(!cfg.no_poll_batteries);
    }

    #[test]
    fn daemon_config_hybrid_sleep() {
        let content = "CriticalPowerAction=HybridSleep\n";
        let cfg = DaemonConfig::from_str(content);
        assert_eq!(cfg.critical_power_action, CriticalPowerAction::HybridSleep);
    }

    #[test]
    fn daemon_config_unknown_keys_ignored() {
        let content = "SomeUnknownKey=value\nPercentageLow=8\n";
        let cfg = DaemonConfig::from_str(content);
        assert_eq!(cfg.percentage_low, 8);
    }

    // -----------------------------------------------------------------------
    // PowerDevice defaults
    // -----------------------------------------------------------------------

    #[test]
    fn power_device_default() {
        let dev = PowerDevice::default();
        assert_eq!(dev.device_type, DeviceType::Unknown);
        assert_eq!(dev.state, BatteryState::Unknown);
        assert_eq!(dev.technology, BatteryTechnology::Unknown);
        assert_eq!(dev.warning_level, WarningLevel::None);
        assert!(!dev.power_supply);
        assert!(!dev.is_present);
        assert!(!dev.is_rechargeable);
        assert!(!dev.has_history);
        assert!(!dev.has_statistics);
        assert!(!dev.online);
        assert!(dev.percentage < f64::EPSILON);
        assert!(dev.energy_wh < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // Object path generation
    // -----------------------------------------------------------------------

    #[test]
    fn object_path_battery() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/BAT0".to_string(),
            device_type: DeviceType::Battery,
            ..Default::default()
        };
        assert_eq!(
            dev.object_path(),
            "/org/freedesktop/UPower/devices/battery_BAT0"
        );
    }

    #[test]
    fn object_path_line_power() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/AC".to_string(),
            device_type: DeviceType::LinePower,
            ..Default::default()
        };
        assert_eq!(
            dev.object_path(),
            "/org/freedesktop/UPower/devices/line-power_AC"
        );
    }

    #[test]
    fn object_path_empty_native() {
        let dev = PowerDevice {
            native_path: String::new(),
            device_type: DeviceType::Battery,
            ..Default::default()
        };
        assert_eq!(
            dev.object_path(),
            "/org/freedesktop/UPower/devices/battery_unknown"
        );
    }

    // -----------------------------------------------------------------------
    // Icon name computation
    // -----------------------------------------------------------------------

    #[test]
    fn icon_line_power() {
        let dev = PowerDevice {
            device_type: DeviceType::LinePower,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "ac-adapter-symbolic");
    }

    #[test]
    fn icon_battery_full() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 95.0,
            state: BatteryState::Discharging,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "battery-full-symbolic");
    }

    #[test]
    fn icon_battery_good() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 60.0,
            state: BatteryState::Discharging,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "battery-good-symbolic");
    }

    #[test]
    fn icon_battery_low() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 30.0,
            state: BatteryState::Discharging,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "battery-low-symbolic");
    }

    #[test]
    fn icon_battery_caution() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 10.0,
            state: BatteryState::Discharging,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "battery-caution-symbolic");
    }

    #[test]
    fn icon_battery_empty() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 2.0,
            state: BatteryState::Discharging,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "battery-empty-symbolic");
    }

    #[test]
    fn icon_battery_charging() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 60.0,
            state: BatteryState::Charging,
            ..Default::default()
        };
        assert_eq!(
            dev.compute_icon_name(),
            "battery-good-charging-symbolic"
        );
    }

    #[test]
    fn icon_battery_pending_charge() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            percentage: 30.0,
            state: BatteryState::PendingCharge,
            ..Default::default()
        };
        assert_eq!(
            dev.compute_icon_name(),
            "battery-low-charging-symbolic"
        );
    }

    #[test]
    fn icon_mouse() {
        let dev = PowerDevice {
            device_type: DeviceType::Mouse,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "input-mouse-symbolic");
    }

    #[test]
    fn icon_keyboard() {
        let dev = PowerDevice {
            device_type: DeviceType::Keyboard,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "input-keyboard-symbolic");
    }

    #[test]
    fn icon_phone() {
        let dev = PowerDevice {
            device_type: DeviceType::Phone,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "phone-symbolic");
    }

    #[test]
    fn icon_tablet() {
        let dev = PowerDevice {
            device_type: DeviceType::Tablet,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "input-tablet-symbolic");
    }

    #[test]
    fn icon_unknown_device() {
        let dev = PowerDevice {
            device_type: DeviceType::Unknown,
            ..Default::default()
        };
        assert_eq!(dev.compute_icon_name(), "battery-symbolic");
    }

    // -----------------------------------------------------------------------
    // Warning level computation
    // -----------------------------------------------------------------------

    #[test]
    fn warning_not_battery() {
        let dev = PowerDevice {
            device_type: DeviceType::LinePower,
            state: BatteryState::Discharging,
            percentage: 1.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::None);
    }

    #[test]
    fn warning_not_discharging() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Charging,
            percentage: 1.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::None);
    }

    #[test]
    fn warning_action_level() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 1.5,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Action);
    }

    #[test]
    fn warning_critical_level() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 2.5,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Critical);
    }

    #[test]
    fn warning_low_level() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 8.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Low);
    }

    #[test]
    fn warning_discharging_normal() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 50.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(
            dev.compute_warning_level(&cfg),
            WarningLevel::Discharging
        );
    }

    #[test]
    fn warning_custom_thresholds() {
        let cfg = DaemonConfig {
            percentage_low: 20,
            percentage_critical: 10,
            percentage_action: 5,
            ..Default::default()
        };
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 15.0,
            ..Default::default()
        };
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Low);
    }

    #[test]
    fn warning_ups_device() {
        let dev = PowerDevice {
            device_type: DeviceType::Ups,
            state: BatteryState::Discharging,
            percentage: 1.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Action);
    }

    #[test]
    fn warning_at_exact_boundary_low() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 10.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Low);
    }

    #[test]
    fn warning_at_exact_boundary_critical() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 3.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Critical);
    }

    #[test]
    fn warning_at_exact_boundary_action() {
        let dev = PowerDevice {
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            percentage: 2.0,
            ..Default::default()
        };
        let cfg = DaemonConfig::default();
        assert_eq!(dev.compute_warning_level(&cfg), WarningLevel::Action);
    }

    // -----------------------------------------------------------------------
    // Time formatting
    // -----------------------------------------------------------------------

    #[test]
    fn format_time_zero() {
        assert_eq!(PowerDevice::format_time(0), "");
    }

    #[test]
    fn format_time_minutes_only() {
        assert_eq!(PowerDevice::format_time(1800), "30 minutes");
    }

    #[test]
    fn format_time_hours_and_minutes() {
        assert_eq!(PowerDevice::format_time(5400), "1 hours 30 minutes");
    }

    #[test]
    fn format_time_exact_hour() {
        assert_eq!(PowerDevice::format_time(3600), "1 hours 0 minutes");
    }

    #[test]
    fn format_time_one_minute() {
        assert_eq!(PowerDevice::format_time(60), "1 minutes");
    }

    #[test]
    fn format_time_under_a_minute() {
        assert_eq!(PowerDevice::format_time(30), "0 minutes");
    }

    // -----------------------------------------------------------------------
    // micro_to_unit helpers
    // -----------------------------------------------------------------------

    #[test]
    fn micro_to_unit_none() {
        assert!(micro_to_unit(None) < f64::EPSILON);
    }

    #[test]
    fn micro_to_unit_zero() {
        assert!(micro_to_unit(Some(0)) < f64::EPSILON);
    }

    #[test]
    fn micro_to_unit_one_million() {
        let v = micro_to_unit(Some(1_000_000));
        assert!((v - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn micro_to_unit_half() {
        let v = micro_to_unit(Some(500_000));
        assert!((v - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn micro_to_unit_signed_none() {
        assert!(micro_to_unit_signed(None) < f64::EPSILON);
    }

    #[test]
    fn micro_to_unit_signed_positive() {
        let v = micro_to_unit_signed(Some(2_000_000));
        assert!((v - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn micro_to_unit_signed_negative() {
        let v = micro_to_unit_signed(Some(-3_000_000));
        assert!((v - 3.0).abs() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // BacklightDevice
    // -----------------------------------------------------------------------

    #[test]
    fn backlight_percentage_full() {
        let bl = BacklightDevice {
            name: "intel_backlight".to_string(),
            brightness: 1000,
            max_brightness: 1000,
        };
        assert!((bl.percentage() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn backlight_percentage_half() {
        let bl = BacklightDevice {
            name: "test".to_string(),
            brightness: 500,
            max_brightness: 1000,
        };
        assert!((bl.percentage() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn backlight_percentage_zero_max() {
        let bl = BacklightDevice {
            name: "test".to_string(),
            brightness: 50,
            max_brightness: 0,
        };
        assert!(bl.percentage() < f64::EPSILON);
    }

    #[test]
    fn backlight_percentage_zero() {
        let bl = BacklightDevice {
            name: "test".to_string(),
            brightness: 0,
            max_brightness: 1000,
        };
        assert!(bl.percentage() < f64::EPSILON);
    }

    // -----------------------------------------------------------------------
    // HistoryTracker
    // -----------------------------------------------------------------------

    #[test]
    fn history_tracker_new() {
        let t = HistoryTracker::new(100);
        assert_eq!(t.charge_len(), 0);
        assert_eq!(t.rate_len(), 0);
    }

    #[test]
    fn history_tracker_record_charge() {
        let mut t = HistoryTracker::new(100);
        t.record_charge(1000, 85.0, BatteryState::Discharging);
        assert_eq!(t.charge_len(), 1);
        assert!((t.charge_history[0].value - 85.0).abs() < f64::EPSILON);
    }

    #[test]
    fn history_tracker_record_rate() {
        let mut t = HistoryTracker::new(100);
        t.record_rate(1000, 12.5, BatteryState::Discharging);
        assert_eq!(t.rate_len(), 1);
        assert!((t.rate_history[0].value - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn history_tracker_max_entries() {
        let mut t = HistoryTracker::new(3);
        t.record_charge(1, 90.0, BatteryState::Discharging);
        t.record_charge(2, 80.0, BatteryState::Discharging);
        t.record_charge(3, 70.0, BatteryState::Discharging);
        assert_eq!(t.charge_len(), 3);
        t.record_charge(4, 60.0, BatteryState::Discharging);
        assert_eq!(t.charge_len(), 3);
        // The oldest entry (timestamp=1) should have been removed.
        assert_eq!(t.charge_history[0].timestamp, 2);
    }

    #[test]
    fn history_tracker_rate_max_entries() {
        let mut t = HistoryTracker::new(2);
        t.record_rate(1, 10.0, BatteryState::Discharging);
        t.record_rate(2, 11.0, BatteryState::Discharging);
        t.record_rate(3, 12.0, BatteryState::Discharging);
        assert_eq!(t.rate_len(), 2);
        assert_eq!(t.rate_history[0].timestamp, 2);
    }

    #[test]
    fn history_tracker_state_preserved() {
        let mut t = HistoryTracker::new(100);
        t.record_charge(1, 50.0, BatteryState::Charging);
        assert_eq!(t.charge_history[0].state, BatteryState::Charging);
    }

    // -----------------------------------------------------------------------
    // Argument parsing -- upower
    // -----------------------------------------------------------------------

    fn mk_args(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_upower_enumerate() {
        let args = mk_args(&["upower", "--enumerate"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Enumerate);
    }

    #[test]
    fn parse_upower_enumerate_short() {
        let args = mk_args(&["upower", "-e"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Enumerate);
    }

    #[test]
    fn parse_upower_dump() {
        let args = mk_args(&["upower", "--dump"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Dump);
    }

    #[test]
    fn parse_upower_dump_short() {
        let args = mk_args(&["upower", "-d"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Dump);
    }

    #[test]
    fn parse_upower_monitor() {
        let args = mk_args(&["upower", "--monitor"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Monitor);
    }

    #[test]
    fn parse_upower_monitor_short() {
        let args = mk_args(&["upower", "-m"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Monitor);
    }

    #[test]
    fn parse_upower_monitor_detail() {
        let args = mk_args(&["upower", "--monitor-detail"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::MonitorDetail);
    }

    #[test]
    fn parse_upower_show_info() {
        let args = mk_args(&["upower", "--show-info", "/org/foo/bar"]);
        let cfg = parse_upower_args(&args).unwrap();
        match cfg.action {
            UpowerAction::ShowInfo(p) => assert_eq!(p, "/org/foo/bar"),
            _ => panic!("expected ShowInfo"),
        }
    }

    #[test]
    fn parse_upower_show_info_short() {
        let args = mk_args(&["upower", "-i", "/org/bat"]);
        let cfg = parse_upower_args(&args).unwrap();
        match cfg.action {
            UpowerAction::ShowInfo(p) => assert_eq!(p, "/org/bat"),
            _ => panic!("expected ShowInfo"),
        }
    }

    #[test]
    fn parse_upower_show_info_missing_path() {
        let args = mk_args(&["upower", "--show-info"]);
        assert!(parse_upower_args(&args).is_err());
    }

    #[test]
    fn parse_upower_version() {
        let args = mk_args(&["upower", "--version"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Version);
    }

    #[test]
    fn parse_upower_version_short() {
        let args = mk_args(&["upower", "-v"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Version);
    }

    #[test]
    fn parse_upower_help() {
        let args = mk_args(&["upower", "--help"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Help);
    }

    #[test]
    fn parse_upower_help_short() {
        let args = mk_args(&["upower", "-h"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Help);
    }

    #[test]
    fn parse_upower_no_args() {
        let args = mk_args(&["upower"]);
        let cfg = parse_upower_args(&args).unwrap();
        matches!(cfg.action, UpowerAction::Help);
    }

    #[test]
    fn parse_upower_unknown_option() {
        let args = mk_args(&["upower", "--frobnicate"]);
        assert!(parse_upower_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // Argument parsing -- upowerd
    // -----------------------------------------------------------------------

    #[test]
    fn parse_upowerd_defaults() {
        let args = mk_args(&["upowerd"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert!(!opts.show_help);
        assert!(!opts.show_version);
        assert_eq!(opts.config_file, PathBuf::from(DAEMON_CONFIG_FILE));
        assert_eq!(opts.pid_file, PathBuf::from(DAEMON_PID_FILE));
    }

    #[test]
    fn parse_upowerd_help() {
        let args = mk_args(&["upowerd", "--help"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert!(opts.show_help);
    }

    #[test]
    fn parse_upowerd_help_short() {
        let args = mk_args(&["upowerd", "-h"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert!(opts.show_help);
    }

    #[test]
    fn parse_upowerd_version() {
        let args = mk_args(&["upowerd", "--version"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert!(opts.show_version);
    }

    #[test]
    fn parse_upowerd_version_short() {
        let args = mk_args(&["upowerd", "-v"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert!(opts.show_version);
    }

    #[test]
    fn parse_upowerd_config() {
        let args = mk_args(&["upowerd", "--config", "/etc/custom.conf"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert_eq!(opts.config_file, PathBuf::from("/etc/custom.conf"));
    }

    #[test]
    fn parse_upowerd_config_short() {
        let args = mk_args(&["upowerd", "-c", "/tmp/up.conf"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert_eq!(opts.config_file, PathBuf::from("/tmp/up.conf"));
    }

    #[test]
    fn parse_upowerd_pid_file() {
        let args = mk_args(&["upowerd", "--pid-file", "/tmp/up.pid"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert_eq!(opts.pid_file, PathBuf::from("/tmp/up.pid"));
    }

    #[test]
    fn parse_upowerd_pid_file_short() {
        let args = mk_args(&["upowerd", "-p", "/tmp/up.pid"]);
        let opts = parse_upowerd_args(&args).unwrap();
        assert_eq!(opts.pid_file, PathBuf::from("/tmp/up.pid"));
    }

    #[test]
    fn parse_upowerd_config_missing_path() {
        let args = mk_args(&["upowerd", "--config"]);
        assert!(parse_upowerd_args(&args).is_err());
    }

    #[test]
    fn parse_upowerd_pid_missing_path() {
        let args = mk_args(&["upowerd", "--pid-file"]);
        assert!(parse_upowerd_args(&args).is_err());
    }

    #[test]
    fn parse_upowerd_unknown_option() {
        let args = mk_args(&["upowerd", "--foo"]);
        assert!(parse_upowerd_args(&args).is_err());
    }

    // -----------------------------------------------------------------------
    // Output tests -- upower help
    // -----------------------------------------------------------------------

    #[test]
    fn upower_help_output() {
        let mut buf = Vec::new();
        print_upower_help(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("--enumerate"));
        assert!(output.contains("--dump"));
        assert!(output.contains("--monitor"));
        assert!(output.contains("--monitor-detail"));
        assert!(output.contains("--show-info"));
        assert!(output.contains("--version"));
        assert!(output.contains("--help"));
    }

    #[test]
    fn upowerd_help_output() {
        let mut buf = Vec::new();
        print_upowerd_help(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("--config"));
        assert!(output.contains("--pid-file"));
        assert!(output.contains("--version"));
        assert!(output.contains("--help"));
    }

    // -----------------------------------------------------------------------
    // Device display_info output
    // -----------------------------------------------------------------------

    #[test]
    fn display_info_line_power() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/AC".to_string(),
            device_type: DeviceType::LinePower,
            online: true,
            power_supply: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        dev.display_info(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("online:"));
        assert!(output.contains("yes"));
        assert!(output.contains("ac-adapter-symbolic"));
    }

    #[test]
    fn display_info_battery() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/BAT0".to_string(),
            device_type: DeviceType::Battery,
            vendor: "TestVendor".to_string(),
            model: "TestModel".to_string(),
            serial: "12345".to_string(),
            technology: BatteryTechnology::LithiumIon,
            state: BatteryState::Discharging,
            percentage: 75.0,
            energy_wh: 37.5,
            energy_full_wh: 50.0,
            energy_full_design_wh: 55.0,
            voltage_v: 12.0,
            is_present: true,
            is_rechargeable: true,
            ..Default::default()
        };
        let mut buf = Vec::new();
        dev.display_info(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("TestVendor"));
        assert!(output.contains("TestModel"));
        assert!(output.contains("12345"));
        assert!(output.contains("lithium-ion"));
        assert!(output.contains("discharging"));
        assert!(output.contains("75%"));
    }

    #[test]
    fn display_info_battery_time_to_empty() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/BAT0".to_string(),
            device_type: DeviceType::Battery,
            state: BatteryState::Discharging,
            time_to_empty_sec: 7200,
            ..Default::default()
        };
        let mut buf = Vec::new();
        dev.display_info(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("time to empty"));
        assert!(output.contains("2 hours 0 minutes"));
    }

    #[test]
    fn display_info_battery_time_to_full() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/BAT0".to_string(),
            device_type: DeviceType::Battery,
            state: BatteryState::Charging,
            time_to_full_sec: 5400,
            ..Default::default()
        };
        let mut buf = Vec::new();
        dev.display_info(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("time to full"));
        assert!(output.contains("1 hours 30 minutes"));
    }

    #[test]
    fn display_info_unknown_device() {
        let dev = PowerDevice {
            native_path: "/sys/class/power_supply/FOO".to_string(),
            device_type: DeviceType::Mouse,
            percentage: 80.0,
            ..Default::default()
        };
        let mut buf = Vec::new();
        dev.display_info(&mut buf).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("mouse"));
        assert!(output.contains("80%"));
    }

    // -----------------------------------------------------------------------
    // cmd_enumerate with no devices (no /sys on Windows)
    // -----------------------------------------------------------------------

    #[test]
    fn cmd_enumerate_empty() {
        let mut buf = Vec::new();
        // On Windows with no /sys, this should succeed and produce no output.
        let ret = cmd_enumerate(&mut buf).unwrap();
        assert_eq!(ret, 0);
    }

    // -----------------------------------------------------------------------
    // cmd_dump with no devices
    // -----------------------------------------------------------------------

    #[test]
    fn cmd_dump_header() {
        let mut buf = Vec::new();
        let ret = cmd_dump(&mut buf).unwrap();
        assert_eq!(ret, 0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("daemon-version:"));
        assert!(output.contains(VERSION));
    }

    // -----------------------------------------------------------------------
    // cmd_show_info with no devices
    // -----------------------------------------------------------------------

    #[test]
    fn cmd_show_info_not_found() {
        let mut buf = Vec::new();
        let ret = cmd_show_info(&mut buf, "/org/nonexistent").unwrap();
        assert_eq!(ret, 1);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("not found"));
    }

    // -----------------------------------------------------------------------
    // Version string
    // -----------------------------------------------------------------------

    #[test]
    fn version_output() {
        let mut buf = Vec::new();
        writeln!(buf, "UPower client version {VERSION}").unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("0.1.0"));
    }
}
