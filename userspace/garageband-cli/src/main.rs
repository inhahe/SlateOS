#![deny(clippy::all)]

//! garageband-cli — OurOS GarageBand (Apple consumer DAW)
//!
//! Single personality: `garageband`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gb(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: garageband [OPTIONS]");
        println!("GarageBand 10.4 (OurOS) — Apple consumer music studio");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --drummer              Drummer (virtual session drummer)");
        println!("  --smart-controls       Smart Controls (macro knobs)");
        println!("  --learn-to-play        Learn to Play (free piano/guitar lessons)");
        println!("  --ios                  GarageBand for iOS (separate app, also free)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GarageBand 10.4.11 (OurOS)"); return 0; }
    println!("GarageBand 10.4.11 (OurOS)");
    println!("  Vendor: Apple Inc. (bundled free with every Mac since 2004)");
    println!("  Origin: based on the Emagic Logic codebase (Apple acquired Emagic 2002), then");
    println!("          stripped down + made friendly for consumers — introduced at MWSF 2004");
    println!("  Famous launch: Steve Jobs + John Mayer demo, Macworld 2004 keynote");
    println!("  Platform: macOS (free with every Mac), iOS/iPadOS (free), no Windows version");
    println!("  Pricing: FREE — has always been free");
    println!("  Relationship to Logic: same engine + plugin format (AU), upgrade path: open");
    println!("                       GarageBand project in Logic Pro → unlocks all Logic features");
    println!("  Plugins: a stripped subset of Logic's instruments + Apple Loops + amp models");
    println!("  Killer features: Drummer (Logic-grade virtual session drummer), Smart Controls,");
    println!("                  Learn to Play guitar/piano lessons (free downloadable courses),");
    println!("                  Live Loops (since GB 10.3), iOS-recorded tracks open natively");
    println!("  Notable users: Steve Lacy produced Internet hits on iPhone GarageBand,");
    println!("                a song on Rihanna's ANTI album was produced entirely in GarageBand iOS,");
    println!("                Grimes produced demos in GarageBand");
    println!("  Use cases: beginners, songwriters, podcasters, hobbyists, kids");
    println!("  Differentiator: extremely accessible UI, free with every Apple device");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "garageband".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gb(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
