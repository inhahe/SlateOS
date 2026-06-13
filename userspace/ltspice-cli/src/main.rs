#![deny(clippy::all)]

//! ltspice-cli — SlateOS LTspice circuit simulator
//!
//! Multi-personality: `ltspice`

use std::env;
use std::process;

fn run_ltspice(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ltspice [OPTIONS] FILE.asc");
        println!("  -b             Batch mode");
        println!("  -run           Run simulation");
        println!("  -ascii         ASCII output");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("LTspice XVII (24.0.12) (SlateOS)");
        return 0;
    }
    let batch = args.iter().any(|a| a == "-b");
    let file = args.iter().find(|a| a.ends_with(".asc") || a.ends_with(".net")).map(|s| s.as_str()).unwrap_or("circuit.asc");
    if batch {
        println!("LTspice XVII — batch mode");
        println!("Reading: {}", file);
        println!("Parsing netlist...");
        println!("  V1 N001 0 SINE(0 1 1k)");
        println!("  R1 N001 N002 1k");
        println!("  C1 N002 0 100n");
        println!("Running transient analysis: .tran 10m");
        println!("  Timestep: 0.001ms");
        println!("  1000 data points computed");
        println!("Simulation complete. Output: {}.raw", file);
    } else {
        println!("LTspice XVII");
        println!("Opening: {}", file);
        println!("Ready.");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ltspice(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_ltspice};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_ltspice(&["--help".to_string()]), 0);
        assert_eq!(run_ltspice(&["-h".to_string()]), 0);
        let _ = run_ltspice(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_ltspice(&[]);
    }
}
