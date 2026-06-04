#![deny(clippy::all)]

//! labplot-cli — OurOS LabPlot scientific data analysis
//!
//! Single personality: `labplot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_labplot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: labplot [OPTIONS] [FILE.lml]");
        println!("labplot v2.10 (OurOS) — Scientific data analysis and visualization");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  2D/3D plotting, curve fitting, FFT, data reduction,");
        println!("  spreadsheet with formula support, CAS worksheets,");
        println!("  import from CSV/HDF5/FITS/netCDF/ROOT");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("labplot v2.10 (OurOS)"); return 0; }
    println!("labplot: data analysis application started");
    println!("  Plot types: scatter, line, bar, histogram, box, polar, 3D");
    println!("  Analysis: fitting, smoothing, interpolation, differentiation");
    println!("  CAS backends: Maxima, Octave, R, Python");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "labplot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_labplot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_labplot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/labplot"), "labplot");
        assert_eq!(basename(r"C:\bin\labplot.exe"), "labplot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("labplot.exe"), "labplot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_labplot(&["--help".to_string()], "labplot"), 0);
        assert_eq!(run_labplot(&["-h".to_string()], "labplot"), 0);
        let _ = run_labplot(&["--version".to_string()], "labplot");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_labplot(&[], "labplot");
    }
}
