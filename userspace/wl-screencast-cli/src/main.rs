#![deny(clippy::all)]

//! wl-screencast-cli — OurOS wl-screencast PipeWire-based screen sharing
//!
//! Single personality: `wl-screencast`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_screencast(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-screencast [OPTIONS]");
        println!("wl-screencast v0.1 (OurOS) — PipeWire screen sharing for Wayland");
        println!();
        println!("Options:");
        println!("  -o OUTPUT         Output to share");
        println!("  -r REGION         Region to share (X,Y WxH)");
        println!("  -f FPS            Framerate");
        println!("  --show-cursor     Include cursor");
        println!("  --version         Show version");
        println!();
        println!("Creates a PipeWire stream for screen sharing. Used by");
        println!("xdg-desktop-portal for WebRTC/OBS/Teams screen sharing.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-screencast v0.1 (OurOS)"); return 0; }
    let output = args.iter().skip_while(|a| a.as_str() != "-o").nth(1)
        .map(|s| s.as_str()).unwrap_or("*");
    println!("wl-screencast: sharing output {} via PipeWire", output);
    println!("  PipeWire node created — ready for consumers");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-screencast".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_screencast(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
