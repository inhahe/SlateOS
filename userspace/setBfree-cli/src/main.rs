#![deny(clippy::all)]

//! setBfree-cli — OurOS setBfree tonewheel organ emulator
//!
//! Single personality: `setBfree`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

#[allow(non_snake_case)]
fn run_setBfree(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: setBfree [OPTIONS]");
        println!("setBfree v0.8 (OurOS) — DSP tonewheel organ emulator");
        println!();
        println!("Options:");
        println!("  -p PROGRAM    Load program file");
        println!("  -U DRAWBARS   Upper manual drawbars (e.g. 888000000)");
        println!("  -L DRAWBARS   Lower manual drawbars");
        println!("  -P DRAWBARS   Pedal drawbars");
        println!("  -r SPEED      Leslie speed (stopped, slow, fast)");
        println!("  -d DRIVE      Overdrive level (0.0-1.0)");
        println!("  -v VIBRATO    Vibrato type (V1, V2, V3, C1, C2, C3)");
        println!("  -k KEY        MIDI transpose");
        println!("  --version     Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("setBfree v0.8.12 (OurOS)"); return 0; }
    println!("setBfree v0.8.12 (OurOS) — Tonewheel Organ");
    println!("  JACK audio: connected");
    println!("  MIDI: JACK (listening)");
    println!("  Tonewheels: 91 (A0-C8)");
    println!("  Upper drawbars: 888000000");
    println!("  Lower drawbars: 838000000");
    println!("  Pedal drawbars: 80");
    println!("  Vibrato: C3 (chorus)");
    println!("  Leslie: slow (chorale)");
    println!("  Overdrive: 0.3");
    println!("  Percussion: on (2nd harmonic, soft, fast decay)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "setBfree".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_setBfree(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_setBfree};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/setBfree"), "setBfree");
        assert_eq!(basename(r"C:\bin\setBfree.exe"), "setBfree.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("setBfree.exe"), "setBfree");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_setBfree(&["--help".to_string()], "setBfree"), 0);
        assert_eq!(run_setBfree(&["-h".to_string()], "setBfree"), 0);
        let _ = run_setBfree(&["--version".to_string()], "setBfree");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_setBfree(&[], "setBfree");
    }
}
