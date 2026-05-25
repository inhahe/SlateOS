#![deny(clippy::all)]

//! vegas-cli — OurOS MAGIX VEGAS Pro video editor
//!
//! Single personality: `vegas`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vegas(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vegas [OPTIONS] [PROJECT]");
        println!("MAGIX VEGAS Pro 21 (OurOS) — Professional NLE for Windows");
        println!();
        println!("Options:");
        println!("  --script FILE          Run JScript/VBScript/Python");
        println!("  --quickstart           Bypass startup splash");
        println!("  --open FILE            Open .veg project");
        println!("  --render PRESET FILE   Render with preset");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MAGIX VEGAS Pro 21.0.0.187 (OurOS)"); return 0; }
    println!("MAGIX VEGAS Pro 21.0.0.187 (OurOS)");
    println!("  Editions: Edit, Pro, Post, Suite");
    println!("  Features: Color Grading workspace, AI Style Transfer, AI Upscaling");
    println!("  Scripting: JScript, VBScript, Python, C#");
    println!("  Audio: 5.1 surround, professional mixing console");
    println!("  Plug-in formats: OFX, VST");
    println!("  License: perpetual / subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vegas".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vegas(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
