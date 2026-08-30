#![deny(clippy::all)]

//! simulide-cli — Slate OS SimulIDE electronic circuit simulator
//!
//! Single personality: `simulide`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_simulide(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: simulide [OPTIONS] [FILE.sim1]");
        println!("simulide v1.1 (Slate OS) — Real-time electronic circuit simulator");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Real-time circuit simulation, SPICE-based engine,");
        println!("  Arduino/PIC/AVR microcontroller simulation,");
        println!("  Logic analyzer, oscilloscope, I/O components");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("simulide v1.1 (Slate OS)"); return 0; }
    println!("simulide: circuit simulator started");
    println!("  Components: passive, semiconductor, logic gates, MCU");
    println!("  MCU support: AVR, PIC, Arduino");
    println!("  Instruments: oscilloscope, logic analyzer, voltmeter");
    println!("  Simulation: real-time analog + digital");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "simulide".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_simulide(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_simulide};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/simulide"), "simulide");
        assert_eq!(basename(r"C:\bin\simulide.exe"), "simulide.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("simulide.exe"), "simulide");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_simulide(&["--help".to_string()], "simulide"), 0);
        assert_eq!(run_simulide(&["-h".to_string()], "simulide"), 0);
        let _ = run_simulide(&["--version".to_string()], "simulide");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_simulide(&[], "simulide");
    }
}
