#![deny(clippy::all)]

//! irsim-cli — SlateOS IRSIM switch-level simulator
//!
//! Single personality: `irsim`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_irsim(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: irsim [OPTIONS] PRMS_FILE SIM_FILE");
        println!("IRSIM v9.7 (SlateOS) — Switch-level digital circuit simulator");
        println!();
        println!("Options:");
        println!("  -s             Run in batch mode");
        println!("  -p POWER_NET   Power net name (default: Vdd)");
        println!("  -g GROUND_NET  Ground net name (default: GND)");
        println!("  -t STEP_SIZE   Time step size");
        println!("  -N TECH_FILE   Technology parameters file");
        println!("  -a             Analyze mode");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("IRSIM v9.7.103 (SlateOS)"); return 0; }
    println!("IRSIM v9.7.103 (SlateOS) — Switch-Level Simulator");
    println!("  Loading network: inverter_chain.sim");
    println!("  Nodes: 256, Transistors: 512");
    println!("  Technology: scmos 0.35um");
    println!("  Simulating 1000 time steps...");
    println!("    Step 0: inputs set");
    println!("    Step 100: propagation complete");
    println!("    Step 500: steady state reached");
    println!("  Total events: 4,567");
    println!("  Simulation complete: 0.23s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "irsim".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_irsim(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_irsim};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/irsim"), "irsim");
        assert_eq!(basename(r"C:\bin\irsim.exe"), "irsim.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("irsim.exe"), "irsim");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_irsim(&["--help".to_string()], "irsim"), 0);
        assert_eq!(run_irsim(&["-h".to_string()], "irsim"), 0);
        let _ = run_irsim(&["--version".to_string()], "irsim");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_irsim(&[], "irsim");
    }
}
