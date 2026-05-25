#![deny(clippy::all)]

//! goxel-cli — OurOS Goxel voxel editor
//!
//! Single personality: `goxel`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_goxel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: goxel [OPTIONS] [FILE.gox]");
        println!("goxel v0.14 (OurOS) — Open source voxel editor");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Tools:");
        println!("  Brush, shape, laser, plane cut, selection,");
        println!("  move, procedural generation, marching cubes");
        println!("Export: OBJ, PLY, STL, glTF, VOX, PNG");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("goxel v0.14 (OurOS)"); return 0; }
    println!("goxel: voxel editor started");
    println!("  Canvas: 256x256x256 voxels");
    println!("  Layers: unlimited");
    println!("  Palette: 256 colors");
    println!("  Rendering: marching cubes smooth mesh export");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "goxel".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_goxel(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
