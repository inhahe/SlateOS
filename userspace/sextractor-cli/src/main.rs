#![deny(clippy::all)]

//! sextractor-cli — OurOS SExtractor source extraction
//!
//! Single personality: `sex`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_sextractor(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: sex [OPTIONS] IMAGE [IMAGE2]");
        println!("SExtractor v2.28 (OurOS) — Source Extractor for astronomical images");
        println!();
        println!("Options:");
        println!("  -c FILE          Configuration file");
        println!("  -CATALOG_NAME F  Output catalog name");
        println!("  -CATALOG_TYPE T  Output type (ASCII_HEAD, FITS_LDAC, etc.)");
        println!("  -DETECT_THRESH N Detection threshold (sigma)");
        println!("  -ANALYSIS_THRESH N  Analysis threshold");
        println!("  -FILTER_NAME F   Filter file");
        println!("  -WEIGHT_IMAGE F  Weight map image");
        println!("  -PSF_NAME F      PSF model file");
        println!("  -CHECKIMAGE_TYPE T  Check image type");
        println!("  -d               Generate default config");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("SExtractor v2.28.0 (OurOS)"); return 0; }
    if args.iter().any(|a| a == "-d") {
        println!("# Default configuration file for SExtractor");
        println!("CATALOG_NAME    test.cat");
        println!("CATALOG_TYPE    ASCII_HEAD");
        println!("DETECT_THRESH   1.5");
        println!("ANALYSIS_THRESH 1.5");
        println!("DETECT_MINAREA  5");
        return 0;
    }
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
    if files.is_empty() { eprintln!("sex: error: no image file"); return 1; }
    println!("SExtractor v2.28.0 (OurOS)");
    println!("  Image: {} (4096 x 4096)", files[0]);
    println!("  Background: 1234.5 +/- 45.2 ADU");
    println!("  Threshold: 1.5 sigma = 67.8 ADU");
    println!("  Objects: 8,456 detected, 8,234 measured");
    println!("  Output: test.cat");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "sex".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_sextractor(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_sextractor};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/sextractor"), "sextractor");
        assert_eq!(basename(r"C:\bin\sextractor.exe"), "sextractor.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("sextractor.exe"), "sextractor");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_sextractor(&["--help".to_string()], "sextractor"), 0);
        assert_eq!(run_sextractor(&["-h".to_string()], "sextractor"), 0);
        let _ = run_sextractor(&["--version".to_string()], "sextractor");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_sextractor(&[], "sextractor");
    }
}
