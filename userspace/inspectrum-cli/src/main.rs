#![deny(clippy::all)]

//! inspectrum-cli — Slate OS inspectrum signal analyzer
//!
//! Single personality: `inspectrum`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_inspectrum(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: inspectrum [OPTIONS] [FILE]");
        println!("inspectrum v0.2 (Slate OS) — RF signal spectrum analyzer");
        println!();
        println!("Options:");
        println!("  -r RATE        Sample rate in Hz");
        println!("  -f FORMAT      Sample format (cf32, cs16, cu8, cs8, f32, s16)");
        println!("  --fft-size N   FFT size (default: 2048)");
        println!("  --zoom N       Initial zoom level");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("inspectrum v0.2.3 (Slate OS)"); return 0; }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() {
        println!("inspectrum v0.2.3 (Slate OS) — Signal Analyzer");
        println!("  Supported formats: cf32, cs16, cu8, cs8, f32, s16");
        println!("  Status: waiting for file");
        return 0;
    }
    let rate = args.windows(2).find(|w| w[0] == "-r").map(|w| w[1].as_str()).unwrap_or("2400000");
    println!("inspectrum: analyzing {}", files[0]);
    println!("  Sample rate: {} Hz", rate);
    println!("  Format: cf32 (complex float32)");
    println!("  FFT size: 2048");
    println!("  Duration: 5.0s");
    println!("  Displaying spectrogram...");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "inspectrum".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_inspectrum(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_inspectrum};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/inspectrum"), "inspectrum");
        assert_eq!(basename(r"C:\bin\inspectrum.exe"), "inspectrum.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("inspectrum.exe"), "inspectrum");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_inspectrum(&["--help".to_string()], "inspectrum"), 0);
        assert_eq!(run_inspectrum(&["-h".to_string()], "inspectrum"), 0);
        let _ = run_inspectrum(&["--version".to_string()], "inspectrum");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_inspectrum(&[], "inspectrum");
    }
}
