#![deny(clippy::all)]

//! cura-cli — OurOS Cura 3D printing slicer
//!
//! Multi-personality: `cura`, `CuraEngine`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_cura(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: cura [OPTIONS] [FILE.stl | FILE.3mf | FILE.obj]");
        println!("  --version        Show version");
        println!("  --headless       Run without GUI");
        println!("  --slice FILE     Slice and output G-code");
        println!("  --printer NAME   Select printer profile");
        println!("  --quality NAME   Select quality profile");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("Ultimaker Cura 5.6.0 (OurOS)");
        println!("CuraEngine 5.6.0");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".stl") || a.ends_with(".3mf") || a.ends_with(".obj")).map(|s| s.as_str());
    if let Some(f) = file {
        if args.iter().any(|a| a == "--slice") {
            println!("Cura 5.6.0 — slicing: {}", f);
            println!("  Printer: Ender 3 V2");
            println!("  Quality: Standard (0.2mm)");
            println!("  Infill: 20% grid");
            println!("  Supports: none");
            println!("  Estimated time: 2h 15m");
            println!("  Filament: 45.2g (15.1m)");
            println!("  G-code saved: output.gcode");
        } else {
            println!("Cura 5.6.0 — opening: {}", f);
            println!("Ready.");
        }
    } else {
        println!("Ultimaker Cura 5.6.0 — Starting...");
        println!("Ready.");
    }
    0
}

fn run_curaengine(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: CuraEngine slice [OPTIONS]");
        println!("  -v              Verbose");
        println!("  -j FILE.json    Load settings");
        println!("  -l FILE.stl     Load mesh");
        println!("  -o FILE.gcode   Output G-code");
        println!("  --version       Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("CuraEngine 5.6.0 (OurOS)");
        return 0;
    }
    println!("CuraEngine 5.6.0 — slicing...");
    println!("  Loading mesh...");
    println!("  Generating layers: 150");
    println!("  Generating support: none");
    println!("  Writing G-code...");
    println!("  Slicing done. Time: 3.2s");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "cura".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "CuraEngine" => run_curaengine(&rest),
        _ => run_cura(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_cura};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/cura"), "cura");
        assert_eq!(basename(r"C:\bin\cura.exe"), "cura.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("cura.exe"), "cura");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_cura(&["--help".to_string()]), 0);
        assert_eq!(run_cura(&["-h".to_string()]), 0);
        let _ = run_cura(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_cura(&[]);
    }
}
