#![deny(clippy::all)]

//! logicpro-cli — SlateOS Logic Pro (Apple pro DAW)
//!
//! Single personality: `logicpro`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_lp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logicpro [OPTIONS]");
        println!("Logic Pro 11 (SlateOS) — Apple pro digital audio workstation");
        println!();
        println!("Options:");
        println!("  --new                  New project");
        println!("  --session-players      AI Drummer/Bass Player/Keyboard Player");
        println!("  --stem-splitter        Stem Splitter (vocals/drums/bass/other from mix)");
        println!("  --flex-pitch           Flex Pitch (Melodyne-style monophonic pitch edit)");
        println!("  --mainstage            Companion: MainStage (live performance rig)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Logic Pro 11.1 (SlateOS)"); return 0; }
    println!("Logic Pro 11.1 (SlateOS)");
    println!("  Vendor: Apple Inc. (acquired Emagic 2002, Notator/Logic ex-C-Lab)");
    println!("  Origin: Notator (Atari ST, 1987 by Gerhard Lengeling) → Logic Audio →");
    println!("          Apple acquisition 2002 → Logic Pro 7 (2004 Mac-only) → Logic Pro X (2013) → 11");
    println!("  Platform: macOS 13.5+ (Apple Silicon optimized), Logic Pro for iPad ($4.99/mo)");
    println!("  Pricing: $199.99 one-time (no subscription)");
    println!("  Engine: 64-bit float, sample-accurate, dozens of native plugins, MIDI 2.0");
    println!("  Plugin support: AU, AU v3 (sandboxed), AU MIDI FX, Apple's own Alchemy/Sculpture/etc.");
    println!("  Sound library: ~6000 instruments + samples + loops + drum kits (free with purchase)");
    println!("  Session Players (Logic 11): Drummer (since Logic X), Bass Player (new),");
    println!("                              Keyboard Player (new) — AI-driven virtual musicians");
    println!("  Stem Splitter: separate any audio into drums/bass/vocals/other (ML)");
    println!("  Surround: Dolby Atmos with binaural monitoring, integrated spatial audio mixer");
    println!("  Strengths: massive included library, very deep MIDI/scoring tools, pro mastering");
    println!("  Pro users: Hans Zimmer, Calvin Harris, John Mayer, Mark Ronson");
    println!("  Companion: MainStage 3.7 (live concert rig — $29.99, separate)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logicpro".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_lp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_lp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logicpro"), "logicpro");
        assert_eq!(basename(r"C:\bin\logicpro.exe"), "logicpro.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logicpro.exe"), "logicpro");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_lp(&["--help".to_string()], "logicpro"), 0);
        assert_eq!(run_lp(&["-h".to_string()], "logicpro"), 0);
        let _ = run_lp(&["--version".to_string()], "logicpro");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_lp(&[], "logicpro");
    }
}
