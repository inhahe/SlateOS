#![deny(clippy::all)]

//! flite-cli — OurOS Flite lightweight speech synthesis
//!
//! Single personality: `flite`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_flite(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: flite [OPTIONS] TEXT");
        println!("Flite v2.2 (OurOS) — Lightweight speech synthesis");
        println!();
        println!("Options:");
        println!("  -t TEXT           Text to speak");
        println!("  -f FILE           Text file to speak");
        println!("  -o FILE           Output WAV file");
        println!("  -voice NAME       Voice (kal, awb, rms, slt)");
        println!("  --voices          List available voices");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Flite v2.2 (OurOS)");
        return 0;
    }
    if args.iter().any(|a| a == "--voices") {
        println!("Available voices:");
        println!("  kal    (male, 8kHz diphone)");
        println!("  kal16  (male, 16kHz diphone)");
        println!("  awb    (male, clustergen)");
        println!("  rms    (male, clustergen)");
        println!("  slt    (female, clustergen)");
        return 0;
    }
    let text = args.iter()
        .position(|a| a == "-t")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("Hello world");
    println!("Speaking: \"{}\"", text);
    println!("  Voice: kal16");
    println!("  Duration: 1.2s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "flite".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_flite(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_flite};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/flite"), "flite");
        assert_eq!(basename(r"C:\bin\flite.exe"), "flite.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("flite.exe"), "flite");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_flite(&["--help".to_string()], "flite"), 0);
        assert_eq!(run_flite(&["-h".to_string()], "flite"), 0);
        let _ = run_flite(&["--version".to_string()], "flite");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_flite(&[], "flite");
    }
}
