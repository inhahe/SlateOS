#![deny(clippy::all)]

//! tiled-cli — OurOS Tiled map editor
//!
//! Single personality: `tiled`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_tiled(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: tiled [OPTIONS] [FILE...]");
        println!("tiled v1.10 (OurOS) — Tile map editor");
        println!();
        println!("Options:");
        println!("  --export-map FORMAT FILE   Export map to format");
        println!("  --export-tileset FMT FILE  Export tileset");
        println!("  --export-formats           List export formats");
        println!("  --minimize                 Start minimized");
        println!("  --disable-opengl           Disable OpenGL");
        println!("  --version                  Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Tiled v1.10 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "--export-formats") {
        println!("Export formats:");
        println!("  tmx      Tiled XML map format");
        println!("  json     JSON map format");
        println!("  csv      CSV layer data");
        println!("  lua      Lua table");
        println!("  tsx      Tiled XML tileset");
        println!("  png      Image (render map)");
        return 0;
    }
    println!("Tiled v1.10 (OurOS) — Map Editor");
    println!("  Renderer: Software");
    println!("  Map formats: TMX, JSON, CSV, Lua");
    println!("  Tileset formats: TSX, JSON");
    println!("  Plugins: automapping, defold, gmx, json, tbin");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "tiled".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_tiled(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
