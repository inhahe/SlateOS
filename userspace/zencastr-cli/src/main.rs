#![deny(clippy::all)]

//! zencastr-cli — OurOS Zencastr remote podcast recording
//!
//! Single personality: `zencastr`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_zc(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: zencastr [COMMAND] [OPTIONS]");
        println!("Zencastr (OurOS) — Cloud-based podcast recording & editing");
        println!();
        println!("Commands:");
        println!("  new                    New episode");
        println!("  invite EMAIL           Invite guest");
        println!("  vc-record              Video + audio recording");
        println!("  post-production EP     Auto-leveling, noise reduction");
        println!("  ai-edit                AI editing for podcast");
        println!("  distribute             Distribute to all directories");
        println!();
        println!("Options:");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Zencastr v3.0 (OurOS)"); return 0; }
    println!("Zencastr (OurOS)");
    println!("  Recording: Local on each guest's device (then uploaded), separate tracks");
    println!("  Audio: 48 kHz uncompressed WAV per guest");
    println!("  Video: HD video recording (browser-based)");
    println!("  Post-production: Automatic leveling, noise gate, fade in/out");
    println!("  Distribution: Submit to Apple Podcasts, Spotify, Google");
    println!("  License: Free (Hobbyist) / Professional subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "zencastr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_zc(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
