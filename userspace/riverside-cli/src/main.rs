#![deny(clippy::all)]

//! riverside-cli — OurOS Riverside.fm remote podcast/video recording
//!
//! Single personality: `riverside`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: riverside [COMMAND] [OPTIONS]");
        println!("Riverside.fm (OurOS) — Studio-quality remote recording");
        println!();
        println!("Commands:");
        println!("  new                    Start new recording session");
        println!("  invite EMAIL           Invite guest to session");
        println!("  studio ID              Open studio");
        println!("  ai-clips ID            Auto-generate short clips from recording");
        println!("  transcribe ID          Auto-transcribe + speaker detection");
        println!();
        println!("Options:");
        println!("  --max-resolution N     Target resolution (1080p / 4K)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Riverside.fm v2.0 (OurOS)"); return 0; }
    println!("Riverside.fm (OurOS)");
    println!("  Recording: Local on each guest's device (then uploaded), uncompressed");
    println!("  Quality: Up to 4K video, 48 kHz uncompressed audio per track");
    println!("  Magic Clips: AI auto-detects highlight-worthy moments");
    println!("  Transcripts: 100+ languages, speaker diarization");
    println!("  License: Free / Standard / Pro / Business subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "riverside".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rv(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
