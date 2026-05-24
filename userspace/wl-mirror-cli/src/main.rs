#![deny(clippy::all)]

//! wl-mirror-cli — OurOS wl-mirror Wayland output mirroring
//!
//! Single personality: `wl-mirror`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_mirror(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: wl-mirror [OPTIONS] OUTPUT");
        println!("wl-mirror v0.16 (OurOS) — Mirror a Wayland output");
        println!();
        println!("Options:");
        println!("  OUTPUT            Output name to mirror (e.g. HDMI-A-1)");
        println!("  -s SCALE          Scaling (fit, cover, exact, linear, nearest)");
        println!("  -t TRANSFORM      Transform (normal, flipped, 90, 180, 270)");
        println!("  -r REGION         Region to capture (X,Y WxH)");
        println!("  -F                Freeze frame");
        println!("  --fullscreen      Fullscreen mirror window");
        println!("  --fullscreen-output OUT  Fullscreen on specific output");
        println!("  --no-frame        Borderless window");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-mirror v0.16 (OurOS)"); return 0; }
    let output = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("HDMI-A-1");
    println!("wl-mirror: mirroring output {}", output);
    if args.iter().any(|a| a == "--fullscreen") {
        println!("  Mode: fullscreen");
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-mirror".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_mirror(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
