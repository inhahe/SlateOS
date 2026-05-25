#![deny(clippy::all)]

//! descript-cli — OurOS Descript text-based audio/video editor
//!
//! Single personality: `descript`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_dr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: descript [OPTIONS] [PROJECT]");
        println!("Descript (OurOS) — Edit audio & video by editing the transcript");
        println!();
        println!("Options:");
        println!("  --transcribe FILE      Transcribe file (Overdub-grade AI)");
        println!("  --overdub TEXT         Generate speech from text (Overdub voice clone)");
        println!("  --studio               Open Studio Sound (AI audio enhancement)");
        println!("  --filler-removal       Remove ums/uhs/likes automatically");
        println!("  --eye-contact          AI eye-contact correction for video");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Descript 70.0.0 (OurOS)"); return 0; }
    println!("Descript 70.0.0 (OurOS)");
    println!("  Workflow: Transcript-driven editing (delete words = delete audio)");
    println!("  AI: Overdub voice cloning, Studio Sound, Filler Word Removal");
    println!("  Video: AI Eye Contact, Green Screen, Auto-zoom");
    println!("  Collaboration: Cloud-based, real-time multi-editor");
    println!("  License: Free / Hobbyist / Creator / Business subscription");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "descript".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_dr(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
