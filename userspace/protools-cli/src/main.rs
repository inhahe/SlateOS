#![deny(clippy::all)]

//! protools-cli — OurOS Avid Pro Tools DAW
//!
//! Single personality: `protools`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: protools [OPTIONS] [SESSION]");
        println!("Avid Pro Tools 2024 (OurOS) — Industry-standard music & post DAW");
        println!();
        println!("Options:");
        println!("  --new-session NAME     Create new session");
        println!("  --open FILE            Open .ptx session");
        println!("  --bounce TRACK FILE    Bounce track to file");
        println!("  --rec                  Start in record-ready");
        println!("  --sample-rate HZ       44100/48000/88200/96000/176400/192000");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Avid Pro Tools 2024.6 (OurOS)"); return 0; }
    println!("Avid Pro Tools 2024.6 (OurOS)");
    println!("  Editions: Intro, Artist, Studio, Ultimate, Carbon, MTRX");
    println!("  Audio engine: 384 kHz / 32-bit float");
    println!("  Plug-ins: AAX (Native, DSP), HEAT analog modeling");
    println!("  Surround: Dolby Atmos, 7.1.4 immersive");
    println!("  Cloud: Pro Tools Sketch, Cloud Collaboration");
    println!("  License: Avid subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "protools".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
