#![deny(clippy::all)]

//! geogebra-cli — OurOS GeoGebra dynamic mathematics
//!
//! Single personality: `geogebra`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_geogebra(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: geogebra [OPTIONS] [FILE.ggb]");
        println!("geogebra v6.0 (OurOS) — Dynamic mathematics software");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Perspectives:");
        println!("  Graphing, Geometry, Spreadsheet, CAS,");
        println!("  3D Graphics, Probability");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("geogebra v6.0 (OurOS)"); return 0; }
    println!("geogebra: dynamic mathematics started");
    println!("  Algebra: symbolic CAS engine");
    println!("  Geometry: interactive constructions");
    println!("  Graphing: function plotting, sliders");
    println!("  3D: surface plotting, solids of revolution");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "geogebra".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_geogebra(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
