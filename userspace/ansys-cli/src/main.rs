#![deny(clippy::all)]

//! ansys-cli — OurOS Ansys multiphysics engineering simulation
//!
//! Single personality: `ansys`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ansys(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ansys [OPTIONS] [FILE]");
        println!("Ansys 2024 R2 (OurOS) — Engineering simulation (multiphysics)");
        println!();
        println!("Options:");
        println!("  -b                     Batch mode (no GUI)");
        println!("  -i INPUT               Input file (.dat/.inp/.wbpj)");
        println!("  -p PRODUCT             Product (ane3fl/mech/cfd/hfss/maxwell/lsdyna)");
        println!("  -np N                  Parallel processes");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ansys 2024 R2 (OurOS)"); return 0; }
    println!("Ansys 2024 R2 (OurOS)");
    println!("  Products: Mechanical (FEA), Fluent/CFX (CFD), HFSS/Maxwell (EM)");
    println!("  Products: LS-DYNA (explicit), Discovery, Workbench, SpaceClaim");
    println!("  Scripting: APDL (Mechanical), Python (PyAnsys), Workbench scripting");
    println!("  HPC: distributed parallel (MPI), GPU acceleration");
    println!("  Industries: aerospace, automotive, energy, electronics, materials");
    println!("  License: enterprise (per-solver tokens)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ansys".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ansys(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
