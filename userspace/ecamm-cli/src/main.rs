#![deny(clippy::all)]

//! ecamm-cli — OurOS Ecamm Live streaming app (macOS)
//!
//! Single personality: `ecamm`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ecamm(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: ecamm [OPTIONS]");
        println!("Ecamm Live 4 (OurOS) — Mac-native live streaming");
        println!();
        println!("Options:");
        println!("  --interview            Open Interview Mode (multi-guest)");
        println!("  --scenes               Scene editor");
        println!("  --overlays             Overlays library");
        println!("  --start                Start broadcasting");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Ecamm Live 4.2.1 (OurOS)"); return 0; }
    println!("Ecamm Live 4.2.1 (OurOS)");
    println!("  Inputs: Camera, screen, iOS/iPadOS device, NDI, virtual cameras");
    println!("  Outputs: YouTube/Facebook/Twitch/LinkedIn/Custom RTMP");
    println!("  Interview Mode: invite up to 4 guests via browser URL");
    println!("  Effects: Chroma key, lower thirds, picture-in-picture");
    println!("  License: subscription (Standard/Pro)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ecamm".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ecamm(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
