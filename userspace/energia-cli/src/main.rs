#![deny(clippy::all)]

//! energia-cli — SlateOS Energia IDE for TI MSP430/MSP432/Tiva/CC3200
//!
//! Single personality: `energia`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_energia(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: energia [OPTIONS] [SKETCH]");
        println!("Energia 1.8.10E23 (SlateOS) — Arduino-style IDE for TI LaunchPads");
        println!();
        println!("Options:");
        println!("  --sketch FILE          Open .ino sketch");
        println!("  --board BOARD          msp430g2/msp432p401r/tm4c123/cc3200");
        println!("  --upload               Compile and upload via mspdebug/dslite");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Energia 1.8.10E23 (SlateOS)"); return 0; }
    println!("Energia 1.8.10E23 (SlateOS)");
    println!("  Targets: TI MSP430, MSP432 (Cortex-M4F), Tiva C (Cortex-M4F), CC3200 (Wi-Fi)");
    println!("  CC2650 BLE LaunchPad, CC1310/CC1352 sub-GHz LaunchPads");
    println!("  Language: Arduino-compatible C/C++ wireless library");
    println!("  Wiring: simplified energia.h API on top of TI DriverLib");
    println!("  Programmer: mspdebug (MSP430), DSLite (MSP432/Tiva), uniflash");
    println!("  Based on: Arduino IDE 1.6.x fork with TI compiler integration");
    println!("  License: LGPL (free, open source)");
    println!("  Note: largely superseded by Code Composer Studio / SimpleLink SDK");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "energia".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_energia(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_energia};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/energia"), "energia");
        assert_eq!(basename(r"C:\bin\energia.exe"), "energia.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("energia.exe"), "energia");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_energia(&["--help".to_string()], "energia"), 0);
        assert_eq!(run_energia(&["-h".to_string()], "energia"), 0);
        let _ = run_energia(&["--version".to_string()], "energia");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_energia(&[], "energia");
    }
}
