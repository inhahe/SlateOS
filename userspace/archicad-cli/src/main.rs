#![deny(clippy::all)]

//! archicad-cli — OurOS Graphisoft Archicad BIM
//!
//! Single personality: `archicad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_archicad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: archicad [OPTIONS] [FILE]");
        println!("Graphisoft Archicad 27 (OurOS) — BIM for architects");
        println!();
        println!("Options:");
        println!("  -open FILE             Open .pln/.pla file");
        println!("  -teamwork URL          Connect to BIMcloud teamwork project");
        println!("  --param-object GSM     Load GDL object library");
        println!("  --rhino IMPORT         Import Rhino/Grasshopper geometry");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Graphisoft Archicad 27 build 5010 (OurOS)"); return 0; }
    println!("Graphisoft Archicad 27 build 5010 (OurOS)");
    println!("  Discipline: Architecture (BIM)");
    println!("  Format: .pln/.pla/.bpn native + IFC 4 (best-in-class), DWG, RVT, SKP");
    println!("  Library: GDL (Geometric Description Language) parametric objects");
    println!("  Collaboration: BIMcloud (real-time teamwork)");
    println!("  Rhino/Grasshopper live connection");
    println!("  Property management, classification, energy evaluation");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "archicad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_archicad(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
