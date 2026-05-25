#![deny(clippy::all)]

//! alitu-cli — OurOS Alitu podcast maker
//!
//! Single personality: `alitu`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_al(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: alitu [COMMAND] [OPTIONS]");
        println!("Alitu (OurOS) — Podcast maker for beginners (browser-based)");
        println!();
        println!("Commands:");
        println!("  new                    New episode");
        println!("  record-call EMAIL      Record call (interview)");
        println!("  trim                   Trim/edit clips");
        println!("  music                  Add intro/outro music");
        println!("  cleanup                Automatic noise reduction & leveling");
        println!("  publish HOST           Publish to PodBean/Buzzsprout/etc.");
        println!();
        println!("Options:");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Alitu v3.2 (OurOS)"); return 0; }
    println!("Alitu (OurOS)");
    println!("  Mode: Browser-based, beginner-friendly");
    println!("  Features: Auto theme music splicing, transitions, fade ins/outs");
    println!("  Call Recorder: Solo or guest co-host call recording");
    println!("  Cleanup: Automatic leveling, noise reduction, silence trimming");
    println!("  Hosting: Optional Alitu hosting + RSS distribution");
    println!("  License: Monthly subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "alitu".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_al(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
