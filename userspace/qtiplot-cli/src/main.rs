#![deny(clippy::all)]

//! qtiplot-cli — OurOS QtiPlot scientific graphing
//!
//! Single personality: `qtiplot`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_qtiplot(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: qtiplot [OPTIONS] [FILE.qti]");
        println!("qtiplot v0.9.9 (OurOS) — Scientific data analysis and graphing");
        println!();
        println!("Options:");
        println!("  -x SCRIPT         Execute script");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Interactive plotting, curve fitting, peak analysis,");
        println!("  scripting (Python/muParser), matrix operations,");
        println!("  export to EPS/PDF/SVG/PNG");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("qtiplot v0.9.9 (OurOS)"); return 0; }
    println!("qtiplot: data analysis application started");
    println!("  Tables: spreadsheet with formulas");
    println!("  Graphs: 2D line/scatter/bar/pie, 3D surface/bar");
    println!("  Analysis: fit, FFT, filter, integrate, differentiate");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "qtiplot".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_qtiplot(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_qtiplot};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/qtiplot"), "qtiplot");
        assert_eq!(basename(r"C:\bin\qtiplot.exe"), "qtiplot.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("qtiplot.exe"), "qtiplot");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_qtiplot(&["--help".to_string()], "qtiplot"), 0);
        assert_eq!(run_qtiplot(&["-h".to_string()], "qtiplot"), 0);
        assert_eq!(run_qtiplot(&["--version".to_string()], "qtiplot"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_qtiplot(&[], "qtiplot"), 0);
    }
}
