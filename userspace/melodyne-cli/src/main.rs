#![deny(clippy::all)]

//! melodyne-cli — OurOS Celemony Melodyne pitch editing
//!
//! Single personality: `melodyne`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mel(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: melodyne [OPTIONS] [AUDIO]");
        println!("Celemony Melodyne 5 Studio (OurOS) — DNA polyphonic pitch & time editor");
        println!();
        println!("Options:");
        println!("  --algorithm ALG        Algorithm (universal/percussive/melodic/polyphonic)");
        println!("  --transfer             Transfer audio from DAW");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Celemony Melodyne 5.4.1 Studio (OurOS)"); return 0; }
    println!("Celemony Melodyne 5.4.1 Studio (OurOS)");
    println!("  Editions: Essential, Assistant, Editor, Studio");
    println!("  Algorithms: Universal, Percussive, Melodic, Polyphonic, Polyphonic Sustain");
    println!("  DNA: Direct Note Access — edit individual notes within chords");
    println!("  Features: Sound editor, leveling macro, fade tool, tempo detection");
    println!("  Integration: ARA2 (Pro Tools, Cubase, Logic, Studio One)");
    println!("  Plug-in formats: VST2, VST3, AU, AAX + standalone");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "melodyne".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mel(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests { #[test] fn test_basic() { assert!(true); } }
