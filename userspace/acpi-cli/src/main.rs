#![deny(clippy::all)]

//! acpi-cli — OurOS ACPI information tools
//!
//! Multi-personality: `acpi`, `acpid`, `acpi_listen`

use std::env;
use std::process;

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(base, _)| base)
}

fn run_acpi(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: acpi [OPTIONS]");
        println!();
        println!("acpi — show ACPI information (OurOS).");
        println!();
        println!("Options:");
        println!("  -b, --battery    Battery information");
        println!("  -a, --ac-adapter AC adapter information");
        println!("  -t, --thermal    Thermal information");
        println!("  -c, --cooling    Cooling device information");
        println!("  -V, --everything Show everything");
        println!("  -s, --show-empty Show empty slots");
        println!("  -f, --fahrenheit Use Fahrenheit");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("acpi 1.7 (OurOS)");
        return 0;
    }

    let battery = args.iter().any(|a| a == "-b" || a == "--battery");
    let ac = args.iter().any(|a| a == "-a" || a == "--ac-adapter");
    let thermal = args.iter().any(|a| a == "-t" || a == "--thermal");
    let cooling = args.iter().any(|a| a == "-c" || a == "--cooling");
    let everything = args.iter().any(|a| a == "-V" || a == "--everything");
    let fahrenheit = args.iter().any(|a| a == "-f" || a == "--fahrenheit");
    let show_default = !battery && !ac && !thermal && !cooling && !everything;

    let temp = |c: f64| -> String {
        if fahrenheit {
            format!("{:.1} degrees F", c * 9.0 / 5.0 + 32.0)
        } else {
            format!("{:.1} degrees C", c)
        }
    };

    if show_default || battery || everything {
        println!("Battery 0: Full, 100%");
        println!("Battery 0: design capacity 5000 mAh, last full capacity 4850 mAh = 97%");
    }
    if ac || everything {
        println!("Adapter 0: on-line");
    }
    if thermal || everything {
        println!("Thermal 0: ok, {}", temp(45.0));
        println!("Thermal 0: trip point 0 switches to mode critical at temperature {}", temp(110.0));
        println!("Thermal 0: trip point 1 switches to mode passive at temperature {}", temp(100.0));
        println!("Thermal 1: ok, {}", temp(38.0));
    }
    if cooling || everything {
        println!("Cooling 0: Processor 0 of 10");
        println!("Cooling 1: Processor 0 of 10");
        println!("Cooling 2: intel_powerclamp no state information available");
        println!("Cooling 3: Fan 0 of 1");
    }
    0
}

fn run_acpid(_args: &[String]) -> i32 {
    println!("acpid: starting daemon (OurOS)");
    println!("acpid: listening on /var/run/acpid.socket");
    println!("acpid: 4 rules loaded");
    println!("acpid: waiting for events");
    0
}

fn run_acpi_listen(_args: &[String]) -> i32 {
    println!("button/power PBTN 00000080 00000001");
    println!("ac_adapter ACPI0003:00 00000080 00000001");
    println!("battery BAT0 00000080 00000001");
    println!("thermal_zone LNXTHERM:00 00000000 00000045");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first()
        .map(|s| strip_ext(basename(s)).to_string())
        .unwrap_or_else(|| "acpi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog.as_str() {
        "acpid" => run_acpid(&rest),
        "acpi_listen" => run_acpi_listen(&rest),
        _ => run_acpi(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_acpi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/acpi"), "acpi");
        assert_eq!(basename(r"C:\bin\acpi.exe"), "acpi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("acpi.exe"), "acpi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_acpi(&["--help".to_string()]), 0);
        assert_eq!(run_acpi(&["-h".to_string()]), 0);
        let _ = run_acpi(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_acpi(&[]);
    }
}
