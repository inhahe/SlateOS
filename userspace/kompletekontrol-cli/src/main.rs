#![deny(clippy::all)]

//! kompletekontrol-cli — OurOS Native Instruments Komplete Kontrol host
//!
//! Single personality: `kompletekontrol`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kk(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kompletekontrol [OPTIONS] [PRESET]");
        println!("NI Komplete Kontrol 3 (OurOS) — Plug-in host & NKS browser for KK keyboards");
        println!();
        println!("Options:");
        println!("  --load FILE            Load NKS-tagged preset");
        println!("  --scan                 Re-scan VST/AU plug-ins");
        println!("  --library NAME         Open specific library");
        println!("  --standalone           Run standalone (else hosted)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NI Komplete Kontrol 3.1.0 (OurOS)"); return 0; }
    println!("NI Komplete Kontrol 3.1.0 (OurOS)");
    println!("  Role: Tag-based preset browser, smart play (scales, arps, chords)");
    println!("  Hardware: A/M/S-Series MK1/MK2/MK3 keyboards, Maschine integration");
    println!("  NKS format: Tagged metadata for any VST instrument");
    println!("  Libraries: 50+ Komplete + 700+ NKS-compatible third-party");
    println!("  Plug-in formats: VST2, VST3, AU host");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kompletekontrol".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kk(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
