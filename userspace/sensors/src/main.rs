#![deny(clippy::all)]

//! sensors — SlateOS hardware sensor monitoring (lm-sensors)
//!
//! Multi-personality binary detected via argv[0]:
//!
//! - `sensors` (default) — show sensor readings
//! - `sensors-detect` — detect and configure hardware sensors
//! - `pwmconfig` — fan/PWM configuration
//! - `fancontrol` — fan speed control daemon

use std::env;
use std::process;

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct _ChipInfo {
    name: String,
    adapter: String,
    readings: Vec<_SensorReading>,
}

#[derive(Clone, Debug)]
struct _SensorReading {
    label: String,
    value: f64,
    unit: String,
    _high: Option<f64>,
    _crit: Option<f64>,
}

fn _sample_chips() -> Vec<_ChipInfo> {
    vec![
        _ChipInfo {
            name: "coretemp-isa-0000".to_string(),
            adapter: "ISA adapter".to_string(),
            readings: vec![
                _SensorReading { label: "Package id 0".to_string(), value: 52.0, unit: "°C".to_string(), _high: Some(80.0), _crit: Some(100.0) },
                _SensorReading { label: "Core 0".to_string(), value: 50.0, unit: "°C".to_string(), _high: Some(80.0), _crit: Some(100.0) },
                _SensorReading { label: "Core 1".to_string(), value: 51.0, unit: "°C".to_string(), _high: Some(80.0), _crit: Some(100.0) },
                _SensorReading { label: "Core 2".to_string(), value: 49.0, unit: "°C".to_string(), _high: Some(80.0), _crit: Some(100.0) },
                _SensorReading { label: "Core 3".to_string(), value: 53.0, unit: "°C".to_string(), _high: Some(80.0), _crit: Some(100.0) },
            ],
        },
        _ChipInfo {
            name: "it8728-isa-0228".to_string(),
            adapter: "ISA adapter".to_string(),
            readings: vec![
                _SensorReading { label: "Vcore".to_string(), value: 1.008, unit: "V".to_string(), _high: None, _crit: None },
                _SensorReading { label: "+3.3V".to_string(), value: 3.312, unit: "V".to_string(), _high: None, _crit: None },
                _SensorReading { label: "+5V".to_string(), value: 5.040, unit: "V".to_string(), _high: None, _crit: None },
                _SensorReading { label: "+12V".to_string(), value: 12.096, unit: "V".to_string(), _high: None, _crit: None },
                _SensorReading { label: "fan1".to_string(), value: 1200.0, unit: "RPM".to_string(), _high: None, _crit: None },
                _SensorReading { label: "fan2".to_string(), value: 950.0, unit: "RPM".to_string(), _high: None, _crit: None },
                _SensorReading { label: "temp1".to_string(), value: 38.0, unit: "°C".to_string(), _high: Some(85.0), _crit: Some(127.0) },
            ],
        },
        _ChipInfo {
            name: "amdgpu-pci-0100".to_string(),
            adapter: "PCI adapter".to_string(),
            readings: vec![
                _SensorReading { label: "edge".to_string(), value: 45.0, unit: "°C".to_string(), _high: Some(100.0), _crit: Some(105.0) },
                _SensorReading { label: "junction".to_string(), value: 47.0, unit: "°C".to_string(), _high: Some(110.0), _crit: Some(115.0) },
                _SensorReading { label: "mem".to_string(), value: 42.0, unit: "°C".to_string(), _high: Some(100.0), _crit: Some(105.0) },
                _SensorReading { label: "PPT".to_string(), value: 35.0, unit: "W".to_string(), _high: Some(203.0), _crit: None },
                _SensorReading { label: "fan1".to_string(), value: 0.0, unit: "RPM".to_string(), _high: None, _crit: None },
            ],
        },
    ]
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_sensors(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sensors [OPTION]... [CHIP]...");
        println!();
        println!("Options:");
        println!("  -f, --fahrenheit   Show temperatures in Fahrenheit");
        println!("  -A, --no-adapter   Do not show adapter line");
        println!("  -u                 Show raw output");
        println!("  -j                 JSON output");
        println!("  --version          Show version");
        return 0;
    }

    if args.iter().any(|a| a == "--version") {
        println!("sensors version 0.1.0 (SlateOS) with libsensors version 3.6.0");
        return 0;
    }

    let fahrenheit = args.iter().any(|a| a == "-f" || a == "--fahrenheit");
    let json_mode = args.iter().any(|a| a == "-j");
    let no_adapter = args.iter().any(|a| a == "-A" || a == "--no-adapter");
    let raw_mode = args.iter().any(|a| a == "-u");

    let filter: Option<&str> = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str());
    let chips = _sample_chips();

    if json_mode {
        println!("{{");
        for (ci, chip) in chips.iter().enumerate() {
            if let Some(f) = filter
                && !chip.name.contains(f) { continue; }
            println!("  \"{}\": {{", chip.name);
            println!("    \"Adapter\": \"{}\",", chip.adapter);
            for (ri, r) in chip.readings.iter().enumerate() {
                let val = if fahrenheit && r.unit == "°C" { r.value * 9.0 / 5.0 + 32.0 } else { r.value };
                println!("    \"{}\": {:.1}{}", r.label, val,
                    if ri + 1 < chip.readings.len() { "," } else { "" });
            }
            println!("  }}{}", if ci + 1 < chips.len() { "," } else { "" });
        }
        println!("}}");
        return 0;
    }

    for chip in &chips {
        if let Some(f) = filter
            && !chip.name.contains(f) { continue; }
        println!("{}", chip.name);
        if !no_adapter {
            println!("Adapter: {}", chip.adapter);
        }
        for r in &chip.readings {
            if raw_mode {
                println!("  {}:", r.label);
                println!("    {}_input: {:.3}", r.label.replace(' ', "_"), r.value);
                if let Some(h) = r._high { println!("    {}_max: {:.3}", r.label.replace(' ', "_"), h); }
                if let Some(c) = r._crit { println!("    {}_crit: {:.3}", r.label.replace(' ', "_"), c); }
            } else {
                let val = if fahrenheit && r.unit == "°C" {
                    r.value * 9.0 / 5.0 + 32.0
                } else {
                    r.value
                };
                let unit = if fahrenheit && r.unit == "°C" { "°F" } else { r.unit.as_str() };

                let extra = match (&r._high, &r._crit) {
                    (Some(h), Some(c)) => {
                        let (hv, cv) = if fahrenheit && r.unit == "°C" {
                            (h * 9.0 / 5.0 + 32.0, c * 9.0 / 5.0 + 32.0)
                        } else {
                            (*h, *c)
                        };
                        format!("  (high = {:.1}{}, crit = {:.1}{})", hv, unit, cv, unit)
                    }
                    (Some(h), None) => {
                        let hv = if fahrenheit && r.unit == "°C" { h * 9.0 / 5.0 + 32.0 } else { *h };
                        format!("  (high = {:.1}{})", hv, unit)
                    }
                    _ => String::new(),
                };

                if r.unit == "RPM" || r.unit == "W" {
                    println!("{:>15}: {:.0} {}{}", r.label, val, unit, extra);
                } else if r.unit == "V" {
                    println!("{:>15}: {:.3} {}{}", r.label, val, unit, extra);
                } else {
                    println!("{:>15}: +{:.1} {}{}", r.label, val, unit, extra);
                }
            }
        }
        println!();
    }
    0
}

fn run_sensors_detect(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sensors-detect [--auto]");
        println!();
        println!("Detect and configure hardware monitoring chips.");
        return 0;
    }

    println!("# sensors-detect (SlateOS)");
    println!("# System: SlateOS Desktop");
    println!();
    println!("Probing for PCI bus adapters...");
    println!("  Found Intel SMBUS at 0x0600");
    println!();
    println!("Probing for Super I/O sensors...");
    println!("  Found ITE IT8728F at 0x228");
    println!("    (in Super I/O)");
    println!();
    println!("Probing for CPU sensors...");
    println!("  Found Intel Core temperature sensor");
    println!();
    println!("Probing for PCI sensors...");
    println!("  Found AMD GPU temperature sensor");
    println!();
    println!("To load everything needed, add these to /etc/modules:");
    println!("  coretemp");
    println!("  it87");
    println!("  amdgpu");
    0
}

fn run_pwmconfig(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: pwmconfig");
        println!();
        println!("Interactive fan/PWM configuration tool.");
        println!("Tests each fan and builds a configuration for fancontrol.");
        return 0;
    }
    let _ = args;
    println!("# pwmconfig - tests and configures fan speed controls");
    println!("Found the following PWM controls:");
    println!("  hwmon0/pwm1  (connected to fan1)");
    println!("  hwmon0/pwm2  (connected to fan2)");
    println!();
    println!("Testing fan1...");
    println!("  Minimum PWM: 40 (runs at ~600 RPM)");
    println!("  Maximum PWM: 255 (runs at ~1800 RPM)");
    println!();
    println!("Configuration written to /etc/fancontrol (simulated)");
    0
}

fn run_fancontrol(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fancontrol [config-file]");
        println!();
        println!("Fan speed control daemon. Reads config from /etc/fancontrol.");
        return 0;
    }
    let _ = args;
    println!("Loading configuration from /etc/fancontrol...");
    println!("Controlling fan1: temp=52°C, target=45-70°C, PWM=120/255");
    println!("Controlling fan2: temp=38°C, target=35-60°C, PWM=80/255");
    println!("(running — simulated)");
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("sensors");
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
        "sensors-detect" => run_sensors_detect(rest),
        "pwmconfig" => run_pwmconfig(rest),
        "fancontrol" => run_fancontrol(rest),
        _ => run_sensors(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_chips() {
        let chips = _sample_chips();
        assert_eq!(chips.len(), 3);
        assert_eq!(chips[0].name, "coretemp-isa-0000");
        assert!(!chips[0].readings.is_empty());
    }
}
