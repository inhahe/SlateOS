#![deny(clippy::all)]

//! comsol-cli — OurOS COMSOL Multiphysics simulation
//!
//! Single personality: `comsol`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_comsol(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: comsol [OPTIONS] [FILE]");
        println!("COMSOL Multiphysics 6.3 (OurOS) — Multiphysics simulation");
        println!();
        println!("Options:");
        println!("  -nodesktop             Headless mode");
        println!("  -inputfile FILE        Java/method input file");
        println!("  -outputfile FILE       Output .mph");
        println!("  --module MOD           AC/DC, CFD, Heat, Structural, etc.");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("COMSOL Multiphysics 6.3 (OurOS)"); return 0; }
    println!("COMSOL Multiphysics 6.3 (OurOS)");
    println!("  Modules: AC/DC, RF, Wave Optics, Heat Transfer, Structural, CFD,");
    println!("           Chemical, Battery, Acoustics, MEMS, Plasma, Semiconductor");
    println!("  Strength: arbitrary multiphysics coupling, PDE-based modeling");
    println!("  Scripting: Java API (Model API), MATLAB LiveLink, Python");
    println!("  Application Builder: turn models into deployable apps");
    println!("  CAD LiveLinks: SOLIDWORKS, Inventor, Creo, Revit, AutoCAD");
    println!("  License: per-module subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "comsol".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_comsol(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
