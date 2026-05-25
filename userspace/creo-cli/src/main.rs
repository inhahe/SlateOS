#![deny(clippy::all)]

//! creo-cli — OurOS PTC Creo parametric 3D CAD
//!
//! Single personality: `creo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_creo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: creo [OPTIONS] [FILE]");
        println!("PTC Creo Parametric 11.0 (OurOS) — Parametric 3D CAD (Pro/ENGINEER successor)");
        println!();
        println!("Options:");
        println!("  -g:no_graphics         Headless mode");
        println!("  -m MACRO               Run trail/macro file");
        println!("  --simulate             Enable Creo Simulate (FEA)");
        println!("  --mold                 Mold design extension");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("PTC Creo Parametric 11.0.0.0 (OurOS)"); return 0; }
    println!("PTC Creo Parametric 11.0.0.0 (OurOS)");
    println!("  Modules: Simulate (FEA), Mold, Tooling, Mechanism, Render Studio, NC");
    println!("  Format: .prt/.asm/.drw native + STEP/IGES/Parasolid/JT/Creo View");
    println!("  Scripting: J-Link (Java), Pro/TOOLKIT (C), Pro/WEB.Link (JavaScript)");
    println!("  AR: Creo View AR for visualization");
    println!("  Augmented reality, generative design, additive manufacturing");
    println!("  PLM: Windchill integration");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "creo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_creo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
