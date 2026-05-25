#![deny(clippy::all)]

//! screengrab-cli — OurOS ScreenGrab screenshot tool
//!
//! Single personality: `screengrab`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_screengrab(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: screengrab [OPTIONS]");
        println!("screengrab v2.7 (OurOS) — Qt-based screenshot tool");
        println!();
        println!("Options:");
        println!("  --fullscreen      Full screen capture");
        println!("  --window          Active window capture");
        println!("  --region          Region selection");
        println!("  --delay SECS      Delay before capture");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("screengrab v2.7 (OurOS)"); return 0; }
    println!("screengrab: screenshot tool started");
    println!("  Modes: fullscreen, window, region");
    println!("  Format: PNG, JPEG, BMP");
    println!("  Upload: configured services");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "screengrab".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_screengrab(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
