#![deny(clippy::all)]

//! nuke-cli — OurOS Foundry Nuke node-based compositing
//!
//! Single personality: `nuke`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_nuke(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: nuke [OPTIONS] [SCRIPT]");
        println!("Foundry Nuke 15 (OurOS) — Node-based compositing & VFX");
        println!();
        println!("Options:");
        println!("  -t [SCRIPT]            Terminal (no GUI) mode");
        println!("  -x [SCRIPT]            Execute script");
        println!("  -F N-M                 Frame range");
        println!("  --studio               NukeStudio (full version)");
        println!("  --nukex                NukeX (additional tools)");
        println!("  -i                     Interactive license");
        println!("  -V LEVEL               Verbose level (0-2)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Foundry Nuke 15.0v4 (OurOS)"); return 0; }
    println!("Foundry Nuke 15.0v4 (OurOS)");
    println!("  Editions: Nuke, NukeX, NukeStudio, Nuke Indie");
    println!("  Scripting: Python, TCL, Blink (GPU)");
    println!("  Trackers: 2D, 3D camera (CaraVR), PlanarTracker");
    println!("  Render: Multi-GPU CUDA, OpenCL");
    println!("  License: foundry-license-server (named/floating)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "nuke".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_nuke(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
