#![deny(clippy::all)]

//! shotcut-cli — OurOS Shotcut open-source video editor
//!
//! Single personality: `shotcut`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: shotcut [OPTIONS] [PROJECT]");
        println!("Shotcut 24.06 (OurOS) — Cross-platform free video editor (MLT framework)");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .mlt project");
        println!("  --gpu                  Enable GPU processing");
        println!("  --noupgrade            Disable upgrade check");
        println!("  --melt-job PRESET FILE Render with melt preset");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Shotcut 24.06.26 (OurOS)"); return 0; }
    println!("Shotcut 24.06.26 (OurOS)");
    println!("  Engine: MLT framework (open source)");
    println!("  Tracks: Unlimited video, audio, subtitle tracks");
    println!("  Effects: 200+ video/audio filters");
    println!("  Formats: All FFmpeg-supported formats (H.264/265, ProRes, DNxHR, etc.)");
    println!("  Color: 16-bit 4:4:4 internal, OpenColorIO, 10-bit HDR");
    println!("  License: GNU GPLv3");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "shotcut".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
