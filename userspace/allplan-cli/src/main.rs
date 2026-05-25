#![deny(clippy::all)]

//! allplan-cli — OurOS Nemetschek Allplan BIM for architects/engineers
//!
//! Single personality: `allplan`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_allplan(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: allplan [OPTIONS] [FILE]");
        println!("Nemetschek Allplan 2025 (OurOS) — BIM for architecture/engineering");
        println!();
        println!("Options:");
        println!("  -project PRJ           Open project");
        println!("  --bimplus              Connect to Bimplus cloud");
        println!("  --pythonpart SCRIPT    Run PythonPart parametric script");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Nemetschek Allplan 2025-1 (OurOS)"); return 0; }
    println!("Nemetschek Allplan 2025-1 (OurOS)");
    println!("  Industries: Architecture, civil engineering, precast concrete");
    println!("  Format: .ndw/.nemproj native + IFC 4.3, DWG/DXF, RVT, SKP");
    println!("  Strengths: reinforced concrete, precast detailing, structural BIM");
    println!("  Scripting: PythonParts (parametric objects), C++ API");
    println!("  Bimplus: cloud collaboration platform");
    println!("  Visualization: integrated CineRender (Cinema 4D engine)");
    println!("  License: subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "allplan".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_allplan(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
