#![deny(clippy::all)]

//! kontakt-cli — OurOS Native Instruments Kontakt sampler
//!
//! Single personality: `kontakt`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_kontakt(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: kontakt [OPTIONS] [INSTRUMENT]");
        println!("Native Instruments Kontakt 7 (OurOS) — Industry-standard software sampler");
        println!();
        println!("Options:");
        println!("  --load FILE            Load .nki/.nkb instrument");
        println!("  --standalone           Run standalone (default is plug-in host)");
        println!("  --batch-resave PATH    Batch resave nkis");
        println!("  --script               Open KSP (Kontakt Script Processor)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("NI Kontakt 7.10.5 (OurOS)"); return 0; }
    println!("NI Kontakt 7.10.5 (OurOS)");
    println!("  Sample formats: WAV, AIFF, FLAC, NCW (lossless)");
    println!("  Scripting: KSP (Kontakt Script Processor)");
    println!("  Modes: Kontakt Player (free, runs Player-compatible libs only) / Full");
    println!("  Libraries: 3,000+ commercial & 1,000+ third-party");
    println!("  Plug-in formats: VST2, VST3, AU, AAX");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "kontakt".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_kontakt(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_kontakt};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/kontakt"), "kontakt");
        assert_eq!(basename(r"C:\bin\kontakt.exe"), "kontakt.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("kontakt.exe"), "kontakt");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_kontakt(&["--help".to_string()], "kontakt"), 0);
        assert_eq!(run_kontakt(&["-h".to_string()], "kontakt"), 0);
        assert_eq!(run_kontakt(&["--version".to_string()], "kontakt"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_kontakt(&[], "kontakt"), 0);
    }
}
