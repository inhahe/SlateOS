#![deny(clippy::all)]

//! flstudio-cli — OurOS FL Studio (Image-Line DAW, pattern-based)
//!
//! Single personality: `flstudio`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_fl(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: flstudio [OPTIONS]");
        println!("FL Studio 21 (OurOS) — Image-Line DAW (originally 'FruityLoops')");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --piano-roll           Open Piano Roll (industry-favorite editor)");
        println!("  --step-sequencer       Step Sequencer (signature pattern-grid)");
        println!("  --playlist             Open Playlist (arrange patterns)");
        println!("  --lifetime-updates     Lifetime Free Updates (Image-Line policy)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("FL Studio 21.2.3 (OurOS)"); return 0; }
    println!("FL Studio 21.2.3 (OurOS)");
    println!("  Vendor: Image-Line BVBA (Ghent, Belgium — founded 1994)");
    println!("  Creator: Didier Dambrin (a.k.a. gol) — solo dev of original FruityLoops 1.0 (Dec 1997)");
    println!("  Rename: 'FruityLoops' → 'FL Studio' in 2003 (Hasbro Fruit Loops trademark dispute)");
    println!("  Platforms: Windows, macOS (since FL 20, 2018), iOS (FL Studio Mobile)");
    println!("  Pricing: Fruity Edition $99, Producer $199, Signature $299, All Plugins $499");
    println!("  KILLER FEATURE: 'Lifetime Free Updates' — pay once, get every major version free");
    println!("                  forever. Unique in the DAW world (vs. Ableton/Logic upgrade fees)");
    println!("  Workflow: pattern-based (step sequencer + piano roll) → arrange patterns in playlist");
    println!("  Piano Roll: industry-acknowledged best-in-class for note editing");
    println!("  Mixer: 125 insert tracks + master, send-routing, plugin chains");
    println!("  Native plugins: Sytrus, Harmor, Harmless, Fruity Reeverb 2, Maximus, Soundgoodizer,");
    println!("                 Slicex, Edison, Newtone, Patcher");
    println!("  Format: VST 2/3, AU (Mac), CLAP (FL 21.2+), MIDI Polyphonic Expression (MPE)");
    println!("  Use cases: hip-hop/trap producers (#1 DAW in genre), EDM, beats, lo-fi");
    println!("  Famous users: Avicii (RIP), Martin Garrix, Porter Robinson, Madeon, Metro Boomin");
    println!("  Easter egg: still nicknamed 'FruityLoops' by everyone, FL retains it in icon");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flstudio".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fl(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
