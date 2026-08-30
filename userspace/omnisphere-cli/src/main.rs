#![deny(clippy::all)]

//! omnisphere-cli — Slate OS Spectrasonics Omnisphere flagship synth
//!
//! Single personality: `omnisphere`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_omni(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: omnisphere [OPTIONS] [PATCH]");
        println!("Spectrasonics Omnisphere 2.8 (Slate OS) — Flagship hybrid synthesizer");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .prt_omn patch");
        println!("  --orb                  Open Orb performance interface");
        println!("  --hardware-profile DEV Hardware synth profile (75+ models)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Spectrasonics Omnisphere 2.8.5d (Slate OS)"); return 0; }
    println!("Spectrasonics Omnisphere 2.8.5d (Slate OS)");
    println!("  Core library: 14,000+ patches, 60+ GB sound sources");
    println!("  Synthesis: Multi-mode (sample-playback + Synth oscillators)");
    println!("  Hardware sync: 75+ hardware synths via Hardware Library");
    println!("  Effects: 58 (FX rack), modulation matrix");
    println!("  Plug-in formats: VST2, VST3, AU, AAX");
    println!("  Companion: Trilian (bass), Keyscape, Stylus RMX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "omnisphere".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_omni(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_omni};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/omnisphere"), "omnisphere");
        assert_eq!(basename(r"C:\bin\omnisphere.exe"), "omnisphere.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("omnisphere.exe"), "omnisphere");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_omni(&["--help".to_string()], "omnisphere"), 0);
        assert_eq!(run_omni(&["-h".to_string()], "omnisphere"), 0);
        let _ = run_omni(&["--version".to_string()], "omnisphere");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_omni(&[], "omnisphere");
    }
}
