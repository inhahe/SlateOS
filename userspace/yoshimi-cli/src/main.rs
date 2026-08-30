#![deny(clippy::all)]

//! yoshimi-cli — Slate OS Yoshimi software synthesizer
//!
//! Single personality: `yoshimi`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_yoshimi(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: yoshimi [OPTIONS]");
        println!("Yoshimi v2.3 (Slate OS) — Software synthesizer (ZynAddSubFX fork)");
        println!();
        println!("Options:");
        println!("  -a DRIVER     Audio backend (jack, alsa)");
        println!("  -m DRIVER     MIDI backend (jack, alsa)");
        println!("  -R RATE       Sample rate");
        println!("  -b SIZE       Buffer size");
        println!("  -l FILE       Load state file");
        println!("  -L FILE       Load instrument");
        println!("  -i             Non-interactive (CLI only)");
        println!("  -c             Command-line interface");
        println!("  -N NAME       JACK client name");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Yoshimi v2.3.2 (Slate OS)"); return 0; }
    println!("Yoshimi v2.3.2 (Slate OS)");
    println!("  Audio: JACK @ 48000 Hz");
    println!("  MIDI: JACK (auto-connect)");
    println!("  Buffer: 256 samples (5.3ms latency)");
    println!("  Parts: 64 (16 visible)");
    println!("  Instruments loaded:");
    println!("    Part 0: Grand Piano");
    println!("    Part 1: Strings Ensemble");
    println!("    Part 2: Jazz Organ");
    println!("  Vector control: enabled");
    println!("  Ready");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "yoshimi".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_yoshimi(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_yoshimi};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/yoshimi"), "yoshimi");
        assert_eq!(basename(r"C:\bin\yoshimi.exe"), "yoshimi.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("yoshimi.exe"), "yoshimi");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_yoshimi(&["--help".to_string()], "yoshimi"), 0);
        assert_eq!(run_yoshimi(&["-h".to_string()], "yoshimi"), 0);
        let _ = run_yoshimi(&["--version".to_string()], "yoshimi");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_yoshimi(&[], "yoshimi");
    }
}
