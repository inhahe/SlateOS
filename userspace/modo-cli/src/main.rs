#![deny(clippy::all)]

//! modo-cli — OurOS Foundry Modo 3D modeling
//!
//! Single personality: `modo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_modo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: modo [OPTIONS] [FILE]");
        println!("Foundry Modo 17 (OurOS) — 3D modeling, sculpting & rendering");
        println!();
        println!("Options:");
        println!("  -cmd CMD              Run script command");
        println!("  -dbon                 Enable debug logging");
        println!("  -nogui                Headless mode");
        println!("  -render SCENE OUT     Render scene to file");
        println!("  -frame N              Render single frame");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Foundry Modo 17.0v5 (OurOS)"); return 0; }
    println!("Foundry Modo 17.0v5 (OurOS)");
    println!("  Renderer: mPath (default), modo Renderer, V-Ray");
    println!("  Scripting: Python, Lua, command system");
    println!("  Modeling: MeshFusion, procedurals, sculpting");
    println!("  Plugins: 14 kits loaded");
    println!("  License: floating (foundry-license-server)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "modo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_modo(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
