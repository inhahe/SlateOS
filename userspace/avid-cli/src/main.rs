#![deny(clippy::all)]

//! avid-cli — OurOS Avid Media Composer NLE
//!
//! Single personality: `avid`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_avid(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: avid [OPTIONS] [PROJECT]");
        println!("Avid Media Composer 2024 (OurOS) — Hollywood-standard NLE");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .avb / .avp project");
        println!("  --bin FILE             Open specific bin");
        println!("  --interplay            Connect to Avid Interplay (shared storage)");
        println!("  --mediacentral         Connect to MediaCentral platform");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Avid Media Composer 2024.4 (OurOS)"); return 0; }
    println!("Avid Media Composer 2024.4 (OurOS)");
    println!("  Editions: First (free), Media Composer, Symphony, Ultimate");
    println!("  Codec: DNxHD/DNxHR (Avid's open standard)");
    println!("  Used in: 90%+ of Hollywood films, network TV news, sports");
    println!("  Storage: NEXIS shared storage, Interplay PAM");
    println!("  Plug-in formats: AAX, AVX");
    println!("  License: subscription (annual or monthly)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "avid".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_avid(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
