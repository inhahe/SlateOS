#![deny(clippy::all)]

//! tlp — Slate OS laptop power management
//!
//! Multi-personality binary for optimizing laptop battery life.
//! Detected via argv[0]:
//!
//! - `tlp` (default) — apply power settings
//! - `tlp-stat` — show power management status
//! - `tlp-rdw` — radio device wizard (enable/disable WiFi/BT on dock/undock)

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _TLP_CONF: &str = "/etc/tlp.conf";
const _TLP_RUN: &str = "/run/tlp";
const _TLP_STAT_D: &str = "/etc/tlp.d";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum PowerMode {
    AC,
    Battery,
}

impl std::fmt::Display for PowerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AC => write!(f, "AC"),
            Self::Battery => write!(f, "battery"),
        }
    }
}

#[derive(Clone, Debug)]
struct TlpConfig {
    _tlp_enable: bool,
    cpu_scaling_governor_on_ac: String,
    cpu_scaling_governor_on_bat: String,
    cpu_energy_perf_policy_on_ac: String,
    cpu_energy_perf_policy_on_bat: String,
    cpu_boost_on_ac: bool,
    cpu_boost_on_bat: bool,
    _sched_powersave_on_ac: bool,
    _sched_powersave_on_bat: bool,
    _disk_apm_level_on_ac: String,
    _disk_apm_level_on_bat: String,
    wifi_pwr_on_ac: bool,
    wifi_pwr_on_bat: bool,
    _pcie_aspm_on_ac: String,
    _pcie_aspm_on_bat: String,
    _usb_autosuspend: bool,
    _sound_power_save_on_ac: u32,
    _sound_power_save_on_bat: u32,
    _runtime_pm_on_ac: bool,
    _runtime_pm_on_bat: bool,
    _start_charge_thresh_bat0: u32,
    _stop_charge_thresh_bat0: u32,
}

impl Default for TlpConfig {
    fn default() -> Self {
        Self {
            _tlp_enable: true,
            cpu_scaling_governor_on_ac: "performance".to_string(),
            cpu_scaling_governor_on_bat: "powersave".to_string(),
            cpu_energy_perf_policy_on_ac: "performance".to_string(),
            cpu_energy_perf_policy_on_bat: "power".to_string(),
            cpu_boost_on_ac: true,
            cpu_boost_on_bat: false,
            _sched_powersave_on_ac: false,
            _sched_powersave_on_bat: true,
            _disk_apm_level_on_ac: "254".to_string(),
            _disk_apm_level_on_bat: "128".to_string(),
            wifi_pwr_on_ac: false,
            wifi_pwr_on_bat: true,
            _pcie_aspm_on_ac: "default".to_string(),
            _pcie_aspm_on_bat: "powersupersave".to_string(),
            _usb_autosuspend: true,
            _sound_power_save_on_ac: 0,
            _sound_power_save_on_bat: 1,
            _runtime_pm_on_ac: false,
            _runtime_pm_on_bat: true,
            _start_charge_thresh_bat0: 75,
            _stop_charge_thresh_bat0: 80,
        }
    }
}

// ── tlp personality ───────────────────────────────────────────────────

fn run_tlp(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "start".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: tlp <command>");
            println!();
            println!("Laptop power management.");
            println!();
            println!("Commands:");
            println!("  start         Initialize power settings (default)");
            println!("  ac            Apply AC power settings");
            println!("  bat           Apply battery power settings");
            println!("  usb           Enable USB autosuspend");
            println!("  bayoff        Turn off optical drive in bay");
            println!("  setcharge     Set battery charge thresholds");
            println!("  fullcharge    Full charge (remove thresholds)");
            println!("  discharge     Force battery discharge");
            println!("  recalibrate   Recalibrate battery");
            println!("  --version     Show version");
            0
        }
        "--version" | "-V" => { println!("tlp 0.1.0 (Slate OS)"); 0 }
        "start" => tlp_start(),
        "ac" => tlp_apply(PowerMode::AC),
        "bat" => tlp_apply(PowerMode::Battery),
        "usb" => { println!("tlp: USB autosuspend applied"); 0 }
        "bayoff" => { println!("tlp: optical drive bay power off (simulated)"); 0 }
        "setcharge" => tlp_setcharge(&args),
        "fullcharge" => { println!("tlp: charge thresholds removed, charging to 100%"); 0 }
        "discharge" => { println!("tlp: force discharge started (simulated)"); 0 }
        "recalibrate" => { println!("tlp: battery recalibration started (simulated)"); 0 }
        other => { eprintln!("tlp: unknown command '{}'", other); 1 }
    }
}

fn tlp_start() -> i32 {
    let config = TlpConfig::default();
    // Detect power source
    let mode = PowerMode::AC; // simulated
    println!("tlp: starting, mode = {}", mode);
    println!("  CPU governor: {}", if mode == PowerMode::AC { &config.cpu_scaling_governor_on_ac } else { &config.cpu_scaling_governor_on_bat });
    println!("  CPU boost: {}", if mode == PowerMode::AC { config.cpu_boost_on_ac } else { config.cpu_boost_on_bat });
    println!("  WiFi power save: {}", if mode == PowerMode::AC { config.wifi_pwr_on_ac } else { config.wifi_pwr_on_bat });
    println!("tlp: settings applied");
    0
}

fn tlp_apply(mode: PowerMode) -> i32 {
    let config = TlpConfig::default();
    println!("tlp: applying {} settings", mode);
    let gov = if mode == PowerMode::AC { &config.cpu_scaling_governor_on_ac } else { &config.cpu_scaling_governor_on_bat };
    let epb = if mode == PowerMode::AC { &config.cpu_energy_perf_policy_on_ac } else { &config.cpu_energy_perf_policy_on_bat };
    println!("  CPU governor → {}", gov);
    println!("  Energy perf policy → {}", epb);
    println!("  CPU boost → {}", if mode == PowerMode::AC { config.cpu_boost_on_ac } else { config.cpu_boost_on_bat });
    println!("  WiFi power save → {}", if mode == PowerMode::AC { config.wifi_pwr_on_ac } else { config.wifi_pwr_on_bat });
    println!("tlp: {} settings applied", mode);
    0
}

fn tlp_setcharge(args: &[String]) -> i32 {
    let start = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(75);
    let stop = args.get(2).and_then(|s| s.parse::<u32>().ok()).unwrap_or(80);
    println!("tlp: setting charge thresholds BAT0: start={} stop={}", start, stop);
    0
}

// ── tlp-stat personality ──────────────────────────────────────────────

fn run_tlp_stat(args: Vec<String>) -> i32 {
    let section = args.first().map(|s| s.as_str()).unwrap_or("all");

    match section {
        "--help" | "-h" => {
            println!("Usage: tlp-stat [OPTIONS]");
            println!();
            println!("Options:");
            println!("  -b, --battery   Battery information");
            println!("  -c, --config    Configuration");
            println!("  -d, --disk      Disk devices");
            println!("  -g, --graphics  Graphics");
            println!("  -p, --processor Processor");
            println!("  -s, --system    System info");
            println!("  -t, --temp      Temperatures");
            println!("  -u, --usb       USB devices");
            println!("  -w, --wifi      WiFi");
            0
        }
        "--version" | "-V" => { println!("tlp-stat 0.1.0 (Slate OS)"); 0 }
        "-b" | "--battery" => stat_battery(),
        "-c" | "--config" => stat_config(),
        "-p" | "--processor" => stat_processor(),
        "-s" | "--system" => stat_system(),
        "-t" | "--temp" => stat_temp(),
        "-w" | "--wifi" => stat_wifi(),
        _ => { stat_system(); println!(); stat_processor(); println!(); stat_battery(); 0 }
    }
}

fn stat_battery() -> i32 {
    println!("--- TLP Battery Status");
    println!("/sys/class/power_supply/BAT0/");
    println!("  Type = Battery");
    println!("  Status = Discharging");
    println!("  Present = 1");
    println!("  Voltage now = 11400 mV");
    println!("  Current now = 1500 mA");
    println!("  Charge full = 4800 mAh");
    println!("  Charge design = 5000 mAh");
    println!("  Charge now = 3456 mAh (72%)");
    println!("  Charge thresholds: start=75, stop=80");
    0
}

fn stat_config() -> i32 {
    let config = TlpConfig::default();
    println!("--- TLP Configuration");
    println!("  CPU_SCALING_GOVERNOR_ON_AC = {}", config.cpu_scaling_governor_on_ac);
    println!("  CPU_SCALING_GOVERNOR_ON_BAT = {}", config.cpu_scaling_governor_on_bat);
    println!("  CPU_ENERGY_PERF_POLICY_ON_AC = {}", config.cpu_energy_perf_policy_on_ac);
    println!("  CPU_ENERGY_PERF_POLICY_ON_BAT = {}", config.cpu_energy_perf_policy_on_bat);
    println!("  CPU_BOOST_ON_AC = {}", if config.cpu_boost_on_ac { 1 } else { 0 });
    println!("  CPU_BOOST_ON_BAT = {}", if config.cpu_boost_on_bat { 1 } else { 0 });
    println!("  WIFI_PWR_ON_AC = {}", if config.wifi_pwr_on_ac { "on" } else { "off" });
    println!("  WIFI_PWR_ON_BAT = {}", if config.wifi_pwr_on_bat { "on" } else { "off" });
    0
}

fn stat_processor() -> i32 {
    println!("--- TLP Processor Status");
    println!("  CPU model: Slate OS Virtual CPU @ 3.6GHz");
    println!("  Cores: 8");
    println!("  Governor: performance");
    println!("  Energy perf policy: performance");
    println!("  CPU boost: enabled");
    println!("  Frequencies: 800-3600 MHz");
    0
}

fn stat_system() -> i32 {
    println!("--- TLP System Info");
    println!("  TLP version: 0.1.0");
    println!("  Power source: AC");
    println!("  Kernel: 6.1.0-slateos");
    println!("  Model: Slate OS Virtual Desktop");
    0
}

fn stat_temp() -> i32 {
    println!("--- TLP Temperatures");
    println!("  CPU: 52°C");
    println!("  GPU: 45°C");
    println!("  NVMe SSD: 38°C");
    0
}

fn stat_wifi() -> i32 {
    println!("--- TLP WiFi Status");
    println!("  wlp3s0: Intel WiFi 6 AX200");
    println!("  Power save: off");
    println!("  Frequency: 5180 MHz");
    println!("  Signal: -45 dBm");
    0
}

// ── tlp-rdw personality ───────────────────────────────────────────────

fn run_tlp_rdw(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "status".to_string());

    match cmd.as_str() {
        "--help" | "-h" => {
            println!("Usage: tlp-rdw <command>");
            println!();
            println!("Radio device wizard for dock/undock events.");
            println!();
            println!("Commands:");
            println!("  status        Show radio device status");
            println!("  enable        Enable WiFi/BT management");
            println!("  disable       Disable WiFi/BT management");
            0
        }
        "status" => {
            println!("tlp-rdw: radio device wizard status");
            println!("  WiFi on dock:   enable");
            println!("  WiFi on undock:  enable");
            println!("  BT on dock:     enable");
            println!("  BT on undock:   disable");
            0
        }
        "enable" => { println!("tlp-rdw: radio device management enabled"); 0 }
        "disable" => { println!("tlp-rdw: radio device management disabled"); 0 }
        other => { eprintln!("tlp-rdw: unknown command '{}'", other); 1 }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("tlp");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' { last_sep = i + 1; }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "tlp-stat" => run_tlp_stat(rest),
        "tlp-rdw" => run_tlp_rdw(rest),
        _ => run_tlp(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TlpConfig::default();
        assert_eq!(config.cpu_scaling_governor_on_ac, "performance");
        assert_eq!(config.cpu_scaling_governor_on_bat, "powersave");
        assert!(config.cpu_boost_on_ac);
        assert!(!config.cpu_boost_on_bat);
    }

    #[test]
    fn test_power_mode_display() {
        assert_eq!(format!("{}", PowerMode::AC), "AC");
        assert_eq!(format!("{}", PowerMode::Battery), "battery");
    }

    #[test]
    fn test_charge_thresholds() {
        let config = TlpConfig::default();
        assert!(config._start_charge_thresh_bat0 < config._stop_charge_thresh_bat0);
        assert!(config._stop_charge_thresh_bat0 <= 100);
    }

    #[test]
    fn test_wifi_settings() {
        let config = TlpConfig::default();
        assert!(!config.wifi_pwr_on_ac);
        assert!(config.wifi_pwr_on_bat);
    }
}
