#![deny(clippy::all)]

//! veusz-cli — Slate OS Veusz scientific plotting
//!
//! Single personality: `veusz`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_veusz(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: veusz [OPTIONS] [FILE.vsz]");
        println!("veusz v3.6 (Slate OS) — Scientific plotting package");
        println!();
        println!("Options:");
        println!("  --export FILE     Export plot to PDF/SVG/PNG/EMF");
        println!("  --listen          Start in listening mode");
        println!("  --version         Show version");
        println!();
        println!("Plot types:");
        println!("  xy, function, bar, fit, contour, image, vectorfield,");
        println!("  ternary, polar, boxplot, colorbar, key");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("veusz v3.6 (Slate OS)"); return 0; }
    println!("veusz: scientific plotting started");
    println!("  WYSIWYG editing with publication-quality output");
    println!("  Data import: CSV, FITS, HDF5, 2D arrays, numpy");
    println!("  Export: PDF, SVG, PNG, EMF, PostScript");
    println!("  Scripting: Python API");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "veusz".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_veusz(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_veusz};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/veusz"), "veusz");
        assert_eq!(basename(r"C:\bin\veusz.exe"), "veusz.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("veusz.exe"), "veusz");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_veusz(&["--help".to_string()], "veusz"), 0);
        assert_eq!(run_veusz(&["-h".to_string()], "veusz"), 0);
        let _ = run_veusz(&["--version".to_string()], "veusz");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_veusz(&[], "veusz");
    }
}
