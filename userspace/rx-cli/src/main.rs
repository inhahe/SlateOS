#![deny(clippy::all)]

//! rx-cli — OurOS iZotope RX audio repair suite
//!
//! Single personality: `rx`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rx(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: rx [OPTIONS] [AUDIO]");
        println!("iZotope RX 11 Advanced (OurOS) — Audio repair & restoration");
        println!();
        println!("Options:");
        println!("  --assistant            Repair Assistant (AI)");
        println!("  --voice-denoise        Voice De-noise");
        println!("  --denoise              Spectral De-noise");
        println!("  --music-rebalance      Separate stems (vocals/drums/bass/other)");
        println!("  --batch FOLDER         Batch process folder");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("iZotope RX 11.5.0 Advanced (OurOS)"); return 0; }
    println!("iZotope RX 11.5.0 Advanced (OurOS)");
    println!("  Modules: 30+ (De-click, De-hum, De-rustle, Dialogue Isolate, etc.)");
    println!("  AI: Dialogue Isolate, Music Rebalance, Voice De-noise");
    println!("  Spectral editing: Frequency-time selection, magic wand, brush tools");
    println!("  Connect: ARA2 (Pro Tools/Cubase/Logic round-trip)");
    println!("  Plug-in formats: VST3, AU, AAX + standalone editor");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rx".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rx(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rx};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rx"), "rx");
        assert_eq!(basename(r"C:\bin\rx.exe"), "rx.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rx.exe"), "rx");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_rx(&["--help".to_string()], "rx"), 0);
        assert_eq!(run_rx(&["-h".to_string()], "rx"), 0);
        assert_eq!(run_rx(&["--version".to_string()], "rx"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_rx(&[], "rx"), 0);
    }
}
