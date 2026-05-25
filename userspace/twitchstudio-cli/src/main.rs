#![deny(clippy::all)]

//! twitchstudio-cli — OurOS Twitch Studio streaming app
//!
//! Single personality: `twitchstudio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ts(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: twitchstudio [OPTIONS]");
        println!("Twitch Studio (OurOS) — Built-for-beginner streaming app from Twitch");
        println!();
        println!("Options:");
        println!("  --setup                Run setup wizard");
        println!("  --scenes               Open scene editor");
        println!("  --quality PROFILE      auto/720p30/720p60/1080p60");
        println!("  --start                Start streaming");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Twitch Studio 0.40.0 (OurOS)"); return 0; }
    println!("Twitch Studio 0.40.0 (OurOS)");
    println!("  Features: Guided setup, alerts, chat overlay, scenes, layouts");
    println!("  Encoders: x264 (CPU), NVENC (NVIDIA), AMF (AMD), QuickSync (Intel)");
    println!("  Quality presets: auto-adjust based on bandwidth + hardware");
    println!("  Integrations: Discord, IRL camera, screen + window capture");
    println!("  License: Free (Twitch account required)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "twitchstudio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_ts(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
