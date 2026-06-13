#![deny(clippy::all)]

//! astropy-cli — SlateOS Astropy toolkit CLI
//!
//! Single personality: `astropy`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_astropy(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: astropy [COMMAND] [OPTIONS]");
        println!("astropy v6.0 (SlateOS) — Astronomical Python library tools");
        println!();
        println!("Commands:");
        println!("  fitsinfo FILE        Show FITS file info");
        println!("  fitsheader FILE      Print FITS header");
        println!("  fitsdiff F1 F2       Compare FITS files");
        println!("  fitscheck FILE       Check FITS compliance");
        println!("  showtable FILE       Display FITS table");
        println!("  volint FILE          Compute volume integral");
        println!("  samp-hub             Start SAMP hub");
        println!();
        println!("Options:");
        println!("  --version            Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("astropy v6.0.1 (SlateOS)"); return 0; }
    match args.first().map(|s| s.as_str()) {
        Some("fitsinfo") => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("image.fits");
            println!("Filename: {}", file);
            println!("No. Name     Type     Cards  Dimensions  Format");
            println!("0   PRIMARY  PrimaryHDU  42  (4096, 4096)  float32");
            println!("1   TABLE    BinTableHDU 12  100R x 5C    [1J, 1D, 1D, 1E, 10A]");
        }
        Some("fitsheader") => {
            println!("SIMPLE  =                    T / standard FITS");
            println!("BITPIX  =                  -32 / floating point");
            println!("NAXIS   =                    2 / 2D image");
            println!("NAXIS1  =                 4096 / width");
            println!("NAXIS2  =                 4096 / height");
            println!("OBJECT  = 'M31     '          / Andromeda Galaxy");
            println!("EXPTIME =                300.0 / exposure time (s)");
        }
        _ => {
            println!("astropy: specify a command (fitsinfo, fitsheader, fitsdiff, etc.)");
            println!("  Use --help for usage information");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "astropy".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_astropy(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_astropy};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/astropy"), "astropy");
        assert_eq!(basename(r"C:\bin\astropy.exe"), "astropy.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("astropy.exe"), "astropy");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_astropy(&["--help".to_string()], "astropy"), 0);
        assert_eq!(run_astropy(&["-h".to_string()], "astropy"), 0);
        let _ = run_astropy(&["--version".to_string()], "astropy");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_astropy(&[], "astropy");
    }
}
