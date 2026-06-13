#![deny(clippy::all)]

//! tlp-cli — Slate OS TLP power management
//!
//! Multi-personality: `tlp`, `tlp-stat`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_tlp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: tlp COMMAND");
        println!();
        println!("tlp — advanced power management (Slate OS).");
        println!();
        println!("Commands:");
        println!("  start          Initialize and apply settings");
        println!("  bat            Apply battery profile");
        println!("  ac             Apply AC profile");
        println!("  usb            Enable USB autosuspend");
        println!("  bayoff         Power off optical drive");
        println!("  setcharge TH   Set battery thresholds");
        println!("  fullcharge     Charge battery to full");
        println!("  recalibrate    Battery recalibration");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("TLP 1.6.1 (Slate OS)");
        return 0;
    }

    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "start" => {
            println!("TLP started in AC mode.");
            println!("Setting CPU governor: performance");
            println!("Setting CPU turbo boost: enabled");
            println!("Setting disk APM: 254 (AC)");
            println!("Setting SATA ALPM: max_performance (AC)");
        }
        "bat" => {
            println!("TLP switching to battery mode.");
            println!("Setting CPU governor: powersave");
            println!("Setting CPU turbo boost: disabled");
            println!("Setting disk APM: 128 (BAT)");
            println!("Setting Wi-Fi power save: on");
        }
        "ac" => {
            println!("TLP switching to AC mode.");
            println!("Setting CPU governor: performance");
            println!("Setting CPU turbo boost: enabled");
        }
        "usb" => println!("USB autosuspend enabled for all devices."),
        "fullcharge" => println!("Setting battery to charge to 100%."),
        _ => {
            eprintln!("tlp: unknown command '{}'. See --help.", cmd);
            return 1;
        }
    }
    0
}

fn run_tlp_stat(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tlp-stat [OPTIONS]");
        println!("Options: -b (battery), -d (disk), -g (graphics), -p (processor), -s (system), -t (temperatures)");
        return 0;
    }

    let battery = args.iter().any(|a| a == "-b");
    let processor = args.iter().any(|a| a == "-p");
    let system = args.iter().any(|a| a == "-s");
    let show_all = !battery && !processor && !system;

    if show_all || system {
        println!("--- TLP 1.6.1 ------------------------------------------------");
        println!("+++ System Info");
        println!("System         = System manufacturer System Product Name");
        println!("BIOS           = American Megatrends Inc. F20 03/15/2024");
        println!("OS Release     = Slate OS 1.0");
        println!("Kernel         = 1.0.0 x86_64");
        println!("TLP power source = AC");
        println!();
    }
    if show_all || processor {
        println!("+++ Processor");
        println!("CPU model      = Intel(R) Core(TM) i9-13900K");
        println!("CPU governor   = performance");
        println!("CPU turbo      = enabled");
        println!("CPU freq range = 800 - 5800 MHz");
        println!("CPU freq       = 3000 MHz");
        println!("CPU scaling driver = intel_pstate");
        println!("EPP            = performance");
        println!();
    }
    if show_all || battery {
        println!("+++ Battery Care");
        println!("Plugin: generic");
        println!("/sys/class/power_supply/BAT0/status = Full");
        println!("/sys/class/power_supply/BAT0/capacity = 100 [%]");
        println!("/sys/class/power_supply/BAT0/energy_full_design = 50000 [mWh]");
        println!("/sys/class/power_supply/BAT0/energy_full = 48500 [mWh]");
        println!("/sys/class/power_supply/BAT0/cycle_count = 123");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "tlp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "tlp-stat" => run_tlp_stat(&rest),
        _ => run_tlp(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_tlp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/tlp"), "tlp");
        assert_eq!(basename(r"C:\bin\tlp.exe"), "tlp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("tlp.exe"), "tlp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_tlp(&["--help".to_string()]), 0);
        assert_eq!(run_tlp(&["-h".to_string()]), 0);
        let _ = run_tlp(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_tlp(&[]);
    }
}
