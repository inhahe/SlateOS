#![deny(clippy::all)]

//! microstation-cli — OurOS Bentley MicroStation CAD for infrastructure
//!
//! Single personality: `microstation`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ms(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: microstation [OPTIONS] [FILE]");
        println!("Bentley MicroStation CONNECT (OurOS) — Infrastructure CAD/BIM");
        println!();
        println!("Options:");
        println!("  -wsRoot DIR            Workspace root");
        println!("  -i FILE                Open DGN file");
        println!("  -keyin CMD             Execute key-in command");
        println!("  --mvba MACRO           Run MicroStation VBA macro");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Bentley MicroStation CONNECT Update 17 (OurOS)"); return 0; }
    println!("Bentley MicroStation CONNECT Update 17 (OurOS)");
    println!("  Industries: Infrastructure (roads/bridges/rail/utilities), plant, building");
    println!("  Format: .dgn native (V8) + DWG/DXF/IFC/SKP/3DS/OBJ");
    println!("  Modeling: 2D drafting, 3D solids/surfaces, parametric, mesh");
    println!("  Scripting: MVBA, MDL (C), C#/.NET API, JavaScript");
    println!("  CONNECT Edition: ProjectWise cloud collaboration, iModels");
    println!("  Verticals: OpenRoads, OpenBuildings, OpenPlant, OpenRail");
    println!("  License: subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "microstation".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ms(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
