#![deny(clippy::all)]

//! logic-cli — Slate OS Apple Logic Pro DAW
//!
//! Single personality: `logic`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_logic(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: logic [OPTIONS] [PROJECT]");
        println!("Apple Logic Pro 11 (Slate OS) — Professional DAW for Mac");
        println!();
        println!("Options:");
        println!("  --open FILE            Open .logicx project");
        println!("  --bounce TRACK FILE    Bounce to file");
        println!("  --tempo BPM            Set tempo");
        println!("  --key KEY              Set project key");
        println!("  --scripter             Open Scripter (JavaScript)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Apple Logic Pro 11.0.1 (Slate OS)"); return 0; }
    println!("Apple Logic Pro 11.0.1 (Slate OS)");
    println!("  Engine: 192 kHz, 32-bit float");
    println!("  Plug-ins: AU (Audio Units), 70+ built-in instruments/effects");
    println!("  Features: Live Loops, Step Sequencer, Drummer (AI), Flex Time/Pitch");
    println!("  Surround: Dolby Atmos");
    println!("  Scripting: JavaScript MIDI Scripter");
    println!("  License: one-time Mac App Store purchase");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "logic".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_logic(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_logic};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/logic"), "logic");
        assert_eq!(basename(r"C:\bin\logic.exe"), "logic.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("logic.exe"), "logic");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_logic(&["--help".to_string()], "logic"), 0);
        assert_eq!(run_logic(&["-h".to_string()], "logic"), 0);
        let _ = run_logic(&["--version".to_string()], "logic");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_logic(&[], "logic");
    }
}
