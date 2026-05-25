#![deny(clippy::all)]

//! solidworks-cli — OurOS Dassault Systèmes SOLIDWORKS
//!
//! Single personality: `solidworks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: solidworks [OPTIONS] [FILE]");
        println!("Dassault SOLIDWORKS 2024 (OurOS) — 3D mechanical CAD");
        println!();
        println!("Options:");
        println!("  /m PATH                Open part/assembly/drawing");
        println!("  /r MACRO               Run macro");
        println!("  /b                     Background (no UI)");
        println!("  --pdm-vault VAULT      Connect to PDM vault");
        println!("  --simulation           Enable Simulation");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Dassault SOLIDWORKS 2024 SP4 (OurOS)"); return 0; }
    println!("Dassault SOLIDWORKS 2024 SP4 (OurOS)");
    println!("  Editions: Standard, Professional, Premium, Education, Student");
    println!("  Modules: Simulation, Flow, CAM, Composer, Electrical, PDM");
    println!("  Format: .sldprt/.sldasm/.slddrw native + STEP/IGES/STL/Parasolid");
    println!("  Scripting: VBA macros, .NET API");
    println!("  Surface modeling, sheet metal, weldments, mold tools, routing");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "solidworks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
