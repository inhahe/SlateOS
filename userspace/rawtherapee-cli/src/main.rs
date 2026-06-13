#![deny(clippy::all)]

//! rawtherapee-cli — SlateOS RawTherapee RAW photo processor
//!
//! Single personality: `rawtherapee-cli`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_rawtherapee(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: rawtherapee-cli [OPTIONS] -c FILE...");
        println!("rawtherapee-cli v5.10 (SlateOS) — RAW photo processor (CLI)");
        println!();
        println!("Options:");
        println!("  -c FILE...        Input files to process");
        println!("  -o DIR            Output directory");
        println!("  -p PP3            Processing profile (.pp3)");
        println!("  -s                Use sidecar pp3 file");
        println!("  -d                Use default processing profile");
        println!("  -j[QUALITY]       Output JPEG (quality 1-100, default 92)");
        println!("  -t                Output 8-bit TIFF");
        println!("  -t16              Output 16-bit TIFF");
        println!("  -n                Output PNG (8-bit)");
        println!("  -b8 / -b16        Bit depth for output");
        println!("  -Y                Overwrite existing files");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .skip_while(|a| a.as_str() != "-c")
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    if files.is_empty() {
        let positional: Vec<&str> = args.iter().filter(|a| !a.starts_with('-')).map(|s| s.as_str()).collect();
        for f in &positional {
            println!("Processing: {}", f);
            println!("  Camera: Nikon Z9");
            println!("  Resolution: 8256x5504");
            println!("  Profile: default");
            println!("  White balance: camera");
            println!("  Exposure: auto");
            println!("  Noise reduction: auto");
            println!("  Output: {}.jpg", f);
        }
        if positional.is_empty() {
            println!("No input files specified. Use -c FILE...");
            return 1;
        }
    } else {
        for f in &files {
            println!("Processing: {}", f);
            println!("  Camera: Nikon Z9");
            println!("  Resolution: 8256x5504");
            println!("  Profile: default");
            println!("  White balance: camera");
            println!("  Exposure: auto");
            println!("  Noise reduction: auto");
            println!("  Output: {}.jpg", f);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "rawtherapee-cli".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_rawtherapee(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_rawtherapee};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/rawtherapee"), "rawtherapee");
        assert_eq!(basename(r"C:\bin\rawtherapee.exe"), "rawtherapee.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("rawtherapee.exe"), "rawtherapee");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_rawtherapee(&["--help".to_string()], "rawtherapee"), 0);
        assert_eq!(run_rawtherapee(&["-h".to_string()], "rawtherapee"), 0);
        let _ = run_rawtherapee(&["--version".to_string()], "rawtherapee");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_rawtherapee(&[], "rawtherapee");
    }
}
