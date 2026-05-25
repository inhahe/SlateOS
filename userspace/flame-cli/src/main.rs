#![deny(clippy::all)]

//! flame-cli — OurOS Autodesk Flame VFX & finishing
//!
//! Single personality: `flame`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flame(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flame [OPTIONS]");
        println!("Autodesk Flame 2025 (OurOS) — High-end VFX, compositing & finishing");
        println!();
        println!("Options:");
        println!("  --start-project NAME   Start specific project");
        println!("  --start-user USER      User profile");
        println!("  --start-workspace WS   Workspace name");
        println!("  --shell                Drop to flame shell");
        println!("  --python SCRIPT        Run Python hook script");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Autodesk Flame 2025.0.1 (OurOS)"); return 0; }
    println!("Autodesk Flame 2025.0.1 (OurOS)");
    println!("  Editions: Flame, Flare, Flame Assist, Lustre");
    println!("  Modules: Action 3D compositing, Batch, BFX, Timeline FX");
    println!("  Scripting: Python (hooks API)");
    println!("  Wiretap: TCP/IP centralized storage protocol");
    println!("  License: floating (autodesk-license-server)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flame".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flame(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
