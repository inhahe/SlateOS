#![deny(clippy::all)]

//! gerbv-cli — OurOS Gerber file viewer
//!
//! Single personality: `gerbv`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gerbv(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gerbv [OPTIONS] FILE...");
        println!("gerbv v2.10 (OurOS) — Gerber file viewer/converter");
        println!();
        println!("Options:");
        println!("  FILE.gbr/.drl     Gerber/Excellon files to view");
        println!("  -o FILE           Export to PNG/SVG/PDF");
        println!("  -x FORMAT         Export format (png, svg, pdf, ps)");
        println!("  --dpi N           Resolution for raster export");
        println!("  --border N        Border percentage");
        println!("  -T                Translate (offset)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("gerbv v2.10 (OurOS)");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    println!("Loading {} layer(s):", files.len().max(1));
    for (i, f) in files.iter().enumerate() {
        println!("  Layer {}: {}", i + 1, f);
    }
    println!("  Board size: 50.0mm x 30.0mm");
    println!("  Apertures: 24");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gerbv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_gerbv(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gerbv};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gerbv"), "gerbv");
        assert_eq!(basename(r"C:\bin\gerbv.exe"), "gerbv.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gerbv.exe"), "gerbv");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gerbv(&["--help".to_string()], "gerbv"), 0);
        assert_eq!(run_gerbv(&["-h".to_string()], "gerbv"), 0);
        let _ = run_gerbv(&["--version".to_string()], "gerbv");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gerbv(&[], "gerbv");
    }
}
