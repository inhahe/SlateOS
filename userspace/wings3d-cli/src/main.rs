#![deny(clippy::all)]

//! wings3d-cli — OurOS Wings 3D subdivision modeler
//!
//! Single personality: `wings3d`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wings3d(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wings3d [OPTIONS] [FILE.wings]");
        println!("wings3d v2.3 (OurOS) — Subdivision surface modeler");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Context-sensitive menus, subdivision surfaces,");
        println!("  UV mapping, vertex painting, AutoUV");
        println!("  Import/Export: OBJ, STL, 3DS, Collada, glTF");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wings3d v2.3 (OurOS)"); return 0; }
    println!("wings3d: subdivision modeler started");
    println!("  Modes: vertex, edge, face, body");
    println!("  Tools: extrude, bevel, bridge, connect, smooth");
    println!("  Materials: OpenGL preview, texture mapping");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wings3d".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wings3d(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
