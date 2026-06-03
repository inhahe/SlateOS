#![deny(clippy::all)]

//! massive-cli — OurOS Native Instruments Massive synthesizer
//!
//! Single personality: `massive`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_massive(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: massive [OPTIONS] [PRESET]");
        println!("NI Massive X 1.5 (OurOS) — Next-generation wavetable synthesizer");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .nmsv preset");
        println!("  --classic              Run Massive (original) instead of Massive X");
        println!("  --browser              Open browser");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NI Massive X 1.5.0 (OurOS)"); return 0; }
    println!("NI Massive X 1.5.0 (OurOS)");
    println!("  Oscillators: 2 wavetable, 9 phase modes, 10 wavetable modes");
    println!("  Filters: Stereo routing matrix, dual filters");
    println!("  Modulation: Performer, Tracker, LFO, ENV (5 each), 4 macros");
    println!("  Routing: Modular signal flow with patch cords");
    println!("  Plug-in formats: VST2, VST3, AU, AAX (Komplete Kontrol integration)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "massive".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_massive(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_massive};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/massive"), "massive");
        assert_eq!(basename(r"C:\bin\massive.exe"), "massive.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("massive.exe"), "massive");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_massive(&["--help".to_string()], "massive"), 0);
        assert_eq!(run_massive(&["-h".to_string()], "massive"), 0);
        assert_eq!(run_massive(&["--version".to_string()], "massive"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_massive(&[], "massive"), 0);
    }
}
