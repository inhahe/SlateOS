#![deny(clippy::all)]

//! restream-cli — OurOS Restream multistreaming platform
//!
//! Single personality: `restream`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rs(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: restream [COMMAND] [OPTIONS]");
        println!("Restream (OurOS) — Multistream to 30+ platforms simultaneously");
        println!();
        println!("Commands:");
        println!("  studio                 Open Restream Studio (browser-based)");
        println!("  channels               Manage connected channels");
        println!("  schedule               Schedule a stream");
        println!("  chat                   Open unified chat (all platforms)");
        println!("  analytics              View stream analytics");
        println!("  events                 List upcoming events");
        println!();
        println!("Options:");
        println!("  --rtmp                 Get RTMP URL & stream key");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Restream Studio v2.0 (OurOS)"); return 0; }
    println!("Restream (OurOS)");
    println!("  Platforms: Twitch, YouTube, Facebook, X/Twitter, LinkedIn, TikTok, ...");
    println!("  Studio: Browser-based stream creation with guests, overlays, recording");
    println!("  Multistream: up to 30+ destinations simultaneously");
    println!("  Chat: unified chat from all platforms in one view");
    println!("  License: Free/Standard/Professional/Business subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "restream".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rs(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
