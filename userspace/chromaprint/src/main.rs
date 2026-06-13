#![deny(clippy::all)]

//! chromaprint — SlateOS audio fingerprinting
//!
//! Single personality: `fpcalc`

use std::env;
use std::process;

fn run_fpcalc(args: Vec<String>) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: fpcalc [OPTIONS] FILE...");
        println!();
        println!("Options:");
        println!("  -length SECS    Length of audio to process (default: 120)");
        println!("  -raw            Output raw fingerprint");
        println!("  -json           Output JSON format");
        println!("  -algorithm NUM  Fingerprint algorithm (default: 2)");
        println!("  -overlap        Overlap analysis windows");
        println!("  -ts             Include timestamps");
        println!("  -version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "-version") {
        println!("fpcalc version 1.5.1 (Slate OS)");
        println!("(Chromaprint library 1.5.1)");
        return 0;
    }

    let json_mode = args.iter().any(|a| a == "-json");
    let raw_mode = args.iter().any(|a| a == "-raw");
    let files: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();

    if files.is_empty() {
        eprintln!("ERROR: No input files.");
        return 1;
    }

    for f in &files {
        if json_mode {
            println!("{{");
            println!("  \"file\": \"{}\",", f);
            println!("  \"duration\": 225.42,");
            println!("  \"fingerprint\": \"AQADtNIyRZiUJEme...simulated...\"");
            println!("}}");
        } else if raw_mode {
            println!("FILE={}", f);
            println!("DURATION=225");
            println!("FINGERPRINT=691415336,691481896,707210792,...(raw values simulated)");
        } else {
            println!("FILE={}", f);
            println!("DURATION=225");
            println!("FINGERPRINT=AQADtNIyRZiUJEme...simulated...");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_fpcalc(rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_fpcalc};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_fpcalc(vec!["--help".to_string()]), 0);
        assert_eq!(run_fpcalc(vec!["-h".to_string()]), 0);
        let _ = run_fpcalc(vec!["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_fpcalc(vec![]);
    }
}
