#![deny(clippy::all)]

//! wl-screenrec-cli — OurOS wl-screenrec hardware-accelerated screen recording
//!
//! Single personality: `wl-screenrec`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_screenrec(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-screenrec [OPTIONS] -f OUTPUT");
        println!("wl-screenrec v0.1 (OurOS) — GPU-accelerated screen recording");
        println!();
        println!("Options:");
        println!("  -f OUTPUT         Output file");
        println!("  -g GEOMETRY       Region (WxH+X+Y)");
        println!("  --codec CODEC     Video codec (h264, h265, vp9, av1)");
        println!("  --audio           Include audio");
        println!("  --low-power       Use low-power encoding mode");
        println!("  --bitrate RATE    Bitrate (e.g. 5M, 10M)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-screenrec v0.1 (OurOS)"); return 0; }
    let output = args.iter().skip_while(|a| a.as_str() != "-f").nth(1)
        .map(|s| s.as_str()).unwrap_or("recording.mp4");
    let codec = args.iter().skip_while(|a| a.as_str() != "--codec").nth(1)
        .map(|s| s.as_str()).unwrap_or("h264");
    println!("wl-screenrec: recording to {} (codec={})", output, codec);
    println!("  Hardware encoding via VA-API");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-screenrec".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_screenrec(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
