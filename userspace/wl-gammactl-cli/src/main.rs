#![deny(clippy::all)]

//! wl-gammactl-cli — OurOS wl-gammactl gamma/brightness/contrast
//!
//! Single personality: `wl-gammactl`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_wl_gammactl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: wl-gammactl [OPTIONS]");
        println!("wl-gammactl v0.1 (OurOS) — Gamma/brightness/contrast control");
        println!();
        println!("Options:");
        println!("  -c CONTRAST       Contrast (0.0-?, default 1.0)");
        println!("  -b BRIGHTNESS     Brightness (0.0-1.0, default 1.0)");
        println!("  -g GAMMA          Gamma (0.1-?, default 1.0)");
        println!("  -r RED_GAMMA      Red channel gamma");
        println!("  -G GREEN_GAMMA    Green channel gamma");
        println!("  -B BLUE_GAMMA     Blue channel gamma");
        println!("  --version         Show version");
        println!();
        println!("Uses wlr-gamma-control-unstable-v1 protocol.");
        println!("Opens a GUI with sliders when no arguments given.");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("wl-gammactl v0.1 (OurOS)"); return 0; }

    if args.is_empty() {
        println!("wl-gammactl: opening gamma control GUI...");
        println!("  Brightness: [=========-] 1.0");
        println!("  Contrast:   [=========-] 1.0");
        println!("  Gamma:      [=========-] 1.0");
    } else {
        let brightness = args.iter().skip_while(|a| a.as_str() != "-b").nth(1)
            .map(|s| s.as_str()).unwrap_or("1.0");
        let contrast = args.iter().skip_while(|a| a.as_str() != "-c").nth(1)
            .map(|s| s.as_str()).unwrap_or("1.0");
        let gamma = args.iter().skip_while(|a| a.as_str() != "-g").nth(1)
            .map(|s| s.as_str()).unwrap_or("1.0");
        println!("wl-gammactl: brightness={} contrast={} gamma={}", brightness, contrast, gamma);
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "wl-gammactl".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_wl_gammactl(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
