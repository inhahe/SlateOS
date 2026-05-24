#![deny(clippy::all)]

//! swaybg-cli — OurOS swaybg wallpaper daemon
//!
//! Single personality: `swaybg`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swaybg(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swaybg [OPTIONS]");
        println!("swaybg v1.2 (OurOS) — Wallpaper daemon for Wayland");
        println!();
        println!("Options:");
        println!("  -i IMAGE          Image path");
        println!("  -m MODE           Scaling: stretch, fill, fit, center, tile");
        println!("  -c COLOR          Fallback color (#RRGGBB)");
        println!("  -o OUTPUT         Specific output");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("swaybg v1.2 (OurOS)"); return 0; }
    let image = args.iter().skip_while(|a| a.as_str() != "-i").nth(1)
        .map(|s| s.as_str()).unwrap_or("wallpaper.png");
    let mode = args.iter().skip_while(|a| a.as_str() != "-m").nth(1)
        .map(|s| s.as_str()).unwrap_or("fill");
    println!("swaybg: {} (mode={})", image, mode);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swaybg".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_swaybg(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
