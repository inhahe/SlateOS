#![deny(clippy::all)]

//! soundtoys-cli — SlateOS Soundtoys creative effects bundle
//!
//! Single personality: `soundtoys`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_st(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: soundtoys [PLUGIN] [OPTIONS]");
        println!("Soundtoys 5.5 (SlateOS) — Creative effects bundle inspired by vintage hardware");
        println!();
        println!("Plugins:");
        println!("  echoboy            EchoBoy (echo/delay simulator)");
        println!("  little-platerverb  Little PlateRev (plate reverb)");
        println!("  decapitator        Decapitator (analog saturation)");
        println!("  devil-loc          Devil-Loc Deluxe (compressor)");
        println!("  filterfreak        FilterFreak (envelope/LFO filter)");
        println!("  phasemistress      PhaseMistress (phaser)");
        println!("  micro-shift        MicroShift (stereo widening)");
        println!("  little-radiator    Little Radiator (tube saturation)");
        println!();
        println!("Options:");
        println!("  --rack                 Open Effect Rack");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Soundtoys 5.5.6 (SlateOS)"); return 0; }
    println!("Soundtoys 5.5.6 (SlateOS)");
    println!("  Bundle: 21 effects (full Soundtoys 5)");
    println!("  Effect Rack: chain Soundtoys plug-ins as a single instance");
    println!("  Companion: Little (free) versions, Tribute & TheEcho hardware");
    println!("  Plug-in formats: VST3, AU, AAX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "soundtoys".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_st(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_st};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/soundtoys"), "soundtoys");
        assert_eq!(basename(r"C:\bin\soundtoys.exe"), "soundtoys.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("soundtoys.exe"), "soundtoys");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_st(&["--help".to_string()], "soundtoys"), 0);
        assert_eq!(run_st(&["-h".to_string()], "soundtoys"), 0);
        let _ = run_st(&["--version".to_string()], "soundtoys");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_st(&[], "soundtoys");
    }
}
