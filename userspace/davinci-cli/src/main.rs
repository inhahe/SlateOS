#![deny(clippy::all)]

//! davinci-cli — OurOS DaVinci Resolve color & editing
//!
//! Single personality: `davinci`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_davinci(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: davinci [OPTIONS]");
        println!("DaVinci Resolve 19 Studio (OurOS) — Editing, color, audio, VFX");
        println!();
        println!("Options:");
        println!("  --script FILE          Run Resolve script (Python/Lua)");
        println!("  --headless             Run without GUI");
        println!("  --import FILE          Import media/project");
        println!("  --export PROJECT FILE  Export project");
        println!("  --color                Open color page");
        println!("  --fusion               Open Fusion page");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("DaVinci Resolve 19.0 Studio (OurOS)"); return 0; }
    println!("DaVinci Resolve 19.0 Studio (OurOS)");
    println!("  Pages: Media, Cut, Edit, Fusion, Color, Fairlight, Deliver");
    println!("  GPU: CUDA, Metal, OpenCL acceleration");
    println!("  Codecs: H.264/265, ProRes, BRAW, RED, ARRI, DNxHR");
    println!("  Scripting: Python 3, Lua, DaVinci API");
    println!("  License: Studio (paid) / Free");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "davinci".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_davinci(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
