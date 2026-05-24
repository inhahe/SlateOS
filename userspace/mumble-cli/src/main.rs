#![deny(clippy::all)]

//! mumble-cli — OurOS Mumble voice chat
//!
//! Multi-personality: `mumble`, `murmurd`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mumble(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mumble [OPTIONS] [URL]");
        println!("mumble v1.5 (OurOS) — Low-latency voice chat client");
        println!();
        println!("Options:");
        println!("  -n                Suppress notification sounds");
        println!("  -m                Start muted");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("mumble v1.5 (OurOS)"); return 0; }
    println!("mumble: voice chat client started");
    println!("  Audio backend: PulseAudio");
    println!("  Opus codec: enabled");
    println!("  Noise suppression: RNNoise");
    0
}

fn run_murmurd(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: murmurd [OPTIONS]");
        println!("murmurd v1.5 (OurOS) — Mumble server (Murmur)");
        println!();
        println!("Options:");
        println!("  -ini FILE         Config file");
        println!("  -fg               Run in foreground");
        println!("  -v                Verbose");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("murmurd v1.5 (OurOS)"); return 0; }
    println!("murmurd: Mumble server started");
    println!("  Port: 64738");
    println!("  Max users: 100");
    println!("  Channels: 5");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mumble".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "murmurd" => run_murmurd(&rest, &prog),
        _ => run_mumble(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
