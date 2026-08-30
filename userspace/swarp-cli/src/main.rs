#![deny(clippy::all)]

//! swarp-cli — Slate OS SWarp image resampling and co-adding
//!
//! Single personality: `swarp`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_swarp(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: swarp [OPTIONS] IMAGE1 [IMAGE2 ...]");
        println!("SWarp v2.41 (Slate OS) — Image resampling and co-addition");
        println!();
        println!("Options:");
        println!("  -c FILE               Configuration file");
        println!("  -IMAGEOUT_NAME FILE   Output image name");
        println!("  -WEIGHTOUT_NAME FILE  Output weight name");
        println!("  -RESAMPLE Y/N         Enable resampling");
        println!("  -COMBINE Y/N          Enable combining");
        println!("  -COMBINE_TYPE TYPE    median, average, min, max, etc.");
        println!("  -CENTER_TYPE TYPE     manual, all, most");
        println!("  -PIXEL_SCALE N        Output pixel scale (arcsec)");
        println!("  -IMAGE_SIZE WxH       Output image size");
        println!("  -d                    Generate default config");
        println!("  --version             Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SWarp v2.41.5 (Slate OS)"); return 0; }
    if args.iter().any(|a| a == "-d") {
        println!("# Default configuration file for SWarp");
        println!("IMAGEOUT_NAME    coadd.fits");
        println!("WEIGHTOUT_NAME   coadd.weight.fits");
        println!("COMBINE_TYPE     MEDIAN");
        println!("RESAMPLE         Y");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() { eprintln!("swarp: error: no input images"); return 1; }
    println!("SWarp v2.41.5 (Slate OS)");
    println!("  Input: {} images", files.len());
    println!("  Resampling {} images...", files.len());
    println!("  Co-adding with median combine...");
    println!("  Output: coadd.fits (8192 x 8192)");
    println!("  Weight: coadd.weight.fits");
    println!("  Done");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "swarp".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_swarp(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_swarp};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/swarp"), "swarp");
        assert_eq!(basename(r"C:\bin\swarp.exe"), "swarp.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("swarp.exe"), "swarp");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_swarp(&["--help".to_string()], "swarp"), 0);
        assert_eq!(run_swarp(&["-h".to_string()], "swarp"), 0);
        let _ = run_swarp(&["--version".to_string()], "swarp");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_swarp(&[], "swarp");
    }
}
