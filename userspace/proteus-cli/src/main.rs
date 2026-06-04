#![deny(clippy::all)]

//! proteus-cli — OurOS Labcenter Proteus schematic/PCB/simulation
//!
//! Single personality: `proteus`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_proteus(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: proteus [OPTIONS] [FILE]");
        println!("Labcenter Proteus 8.17 (OurOS) — Schematic + PCB + microcontroller sim");
        println!();
        println!("Options:");
        println!("  --isis FILE            ISIS Schematic Capture (.DSN)");
        println!("  --ares FILE            ARES PCB Layout (.LYT)");
        println!("  --simulate             Run mixed-mode simulation");
        println!("  --vsm MCU              Virtual System Modelling (MCU co-simulation)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Labcenter Proteus 8.17 SP3 (OurOS)"); return 0; }
    println!("Labcenter Proteus 8.17 SP3 (OurOS)");
    println!("  Tools: ISIS Schematic, ARES PCB, VSM (Virtual System Modelling)");
    println!("  Strength: MCU co-simulation — PIC/AVR/Arduino/ARM/8051 with peripherals");
    println!("  Simulation: SPICE engine + MCU instruction set + virtual instruments");
    println!("  Educational: widely used for university electronics teaching");
    println!("  Format: .DSN (schematic), .LYT (PCB)");
    println!("  Editions: Lite, Standard, Professional, VSM Studio");
    println!("  License: perpetual (one-time license)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "proteus".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_proteus(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_proteus};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/proteus"), "proteus");
        assert_eq!(basename(r"C:\bin\proteus.exe"), "proteus.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("proteus.exe"), "proteus");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_proteus(&["--help".to_string()], "proteus"), 0);
        assert_eq!(run_proteus(&["-h".to_string()], "proteus"), 0);
        let _ = run_proteus(&["--version".to_string()], "proteus");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_proteus(&[], "proteus");
    }
}
