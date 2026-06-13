#![deny(clippy::all)]

//! simulink-cli — Slate OS MathWorks Simulink model-based design
//!
//! Single personality: `simulink`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_simulink(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: simulink [OPTIONS] [MODEL]");
        println!("MathWorks Simulink R2024b (Slate OS) — Block-diagram model-based design");
        println!();
        println!("Options:");
        println!("  -open MODEL            Open .slx/.mdl model");
        println!("  -batch \"sim('M')\"     Run simulation in batch");
        println!("  --codegen TARGET       Generate code (C/C++/HDL/PLC)");
        println!("  --realtime             Simulink Real-Time deployment");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MathWorks Simulink 24.2.0 (R2024b) (Slate OS)"); return 0; }
    println!("MathWorks Simulink R2024b (Slate OS)");
    println!("  Modeling: graphical block diagrams, hierarchical, multi-domain");
    println!("  Solvers: ODE45 (variable-step), ode15s (stiff), discrete, fixed-step");
    println!("  Stateflow: state machines, flow charts, truth tables");
    println!("  Toolboxes: Simscape (physical), Control Design, Aerospace, Powertrain");
    println!("  Code gen: Embedded Coder (C), HDL Coder (FPGA), PLC Coder (IEC 61131-3)");
    println!("  Standards: DO-178/254, IEC 61508, ISO 26262, IEC 62304 qualified");
    println!("  License: with MATLAB (Simulink toolbox)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "simulink".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_simulink(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_simulink};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/simulink"), "simulink");
        assert_eq!(basename(r"C:\bin\simulink.exe"), "simulink.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("simulink.exe"), "simulink");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_simulink(&["--help".to_string()], "simulink"), 0);
        assert_eq!(run_simulink(&["-h".to_string()], "simulink"), 0);
        let _ = run_simulink(&["--version".to_string()], "simulink");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_simulink(&[], "simulink");
    }
}
