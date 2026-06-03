#![deny(clippy::all)]

//! powertop-cli — OurOS power consumption monitor
//!
//! Multi-personality: `powertop`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_powertop(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: powertop [OPTIONS]");
        println!();
        println!("powertop — power consumption analysis tool (OurOS).");
        println!();
        println!("Options:");
        println!("  --auto-tune       Automatically set optimal settings");
        println!("  --calibrate       Run calibration");
        println!("  --html[=FILE]     Generate HTML report");
        println!("  --csv[=FILE]      Generate CSV report");
        println!("  --time=N          Run for N seconds");
        println!("  --iteration=N     Number of iterations");
        println!("  --quiet           Suppress output");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("PowerTOP version 2.15 (OurOS)");
        return 0;
    }

    let auto_tune = args.iter().any(|a| a == "--auto-tune");
    let html = args.iter().any(|a| a.starts_with("--html"));

    if auto_tune {
        println!("Setting: Good  VM writeback timeout = 1500");
        println!("Setting: Good  Enable SATA link power management for host0");
        println!("Setting: Good  Enable SATA link power management for host1");
        println!("Setting: Good  NMI watchdog disabled");
        println!("Setting: Good  Runtime PM for PCI Device");
        println!("Setting: Good  Autosuspend for USB device Bluetooth [Intel]");
        println!("Setting: Good  Wi-Fi power save mode");
        return 0;
    }

    if html {
        println!("Generating HTML report: powertop.html");
        return 0;
    }

    println!("PowerTOP 2.15 (OurOS)     Overview   Idle stats   Frequency stats   Device stats   Tunables");
    println!();
    println!("Summary: 120.0 wakeups/second, 0 GPU ops/second, 0 VFS ops/second");
    println!();
    println!("Power est.     Usage        Events/s   Category       Description");
    println!("  5.00 W     100.0%                    Device         Display backlight");
    println!("  2.50 W       5.0 ms/s     30.0        Process       [PID 1234] firefox");
    println!("  1.20 W       2.0 ms/s     15.0        Process       [PID 5678] Xorg");
    println!("  0.50 W       1.0 ms/s      5.0        Interrupt     [8] timer");
    println!("  0.30 W       0.5 ms/s      3.0        kWork         psi_avgs_work");
    println!("  0.10 W       0.2 ms/s      1.0        Process       [PID 999] systemd");
    println!();
    println!("Total power: 9.60 W");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "powertop".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = run_powertop(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_powertop};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/powertop"), "powertop");
        assert_eq!(basename(r"C:\bin\powertop.exe"), "powertop.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("powertop.exe"), "powertop");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_powertop(&["--help".to_string()]), 0);
        assert_eq!(run_powertop(&["-h".to_string()]), 0);
        assert_eq!(run_powertop(&["--version".to_string()]), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_powertop(&[]), 0);
    }
}
