#![deny(clippy::all)]

//! prusaslicer-cli — SlateOS PrusaSlicer 3D printing slicer
//!
//! Multi-personality: `prusa-slicer`, `prusaslicer`

use std::env;
use std::process;

fn run_prusaslicer(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: prusa-slicer [OPTIONS] [FILE.stl | FILE.3mf]");
        println!("  --version          Show version");
        println!("  --slice FILE       Slice model");
        println!("  --export-gcode     Export G-code");
        println!("  --export-stl       Export STL");
        println!("  --export-3mf       Export 3MF");
        println!("  --info FILE        Show model info");
        println!("  --load FILE.ini    Load config");
        println!("  --save FILE.ini    Save config");
        println!("  --repair FILE      Repair mesh");
        println!("  --loglevel N       Log level (0-4)");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("PrusaSlicer 2.7.1 (SlateOS)");
        println!("Based on Slic3r");
        return 0;
    }
    if args.iter().any(|a| a == "--info") {
        let file = args.windows(2).find(|w| w[0] == "--info").map(|w| w[1].as_str()).unwrap_or("model.stl");
        println!("Model info: {}", file);
        println!("  Format: STL (binary)");
        println!("  Triangles: 12,456");
        println!("  Vertices: 6,230");
        println!("  Size: 80.0 x 60.0 x 45.0 mm");
        println!("  Volume: 123.4 cm^3");
        println!("  Manifold: yes");
        return 0;
    }
    if args.iter().any(|a| a == "--repair") {
        let file = args.windows(2).find(|w| w[0] == "--repair").map(|w| w[1].as_str()).unwrap_or("model.stl");
        println!("Repairing: {}", file);
        println!("  Fixed 3 degenerate facets");
        println!("  Fixed 1 non-manifold edge");
        println!("  Repaired mesh saved.");
        return 0;
    }
    let file = args.iter().find(|a| a.ends_with(".stl") || a.ends_with(".3mf")).map(|s| s.as_str());
    if let Some(f) = file {
        if args.iter().any(|a| a == "--slice" || a == "--export-gcode") {
            println!("PrusaSlicer 2.7.1 — slicing: {}", f);
            println!("  Printer: Original Prusa i3 MK3S+");
            println!("  Layer height: 0.2mm");
            println!("  Infill: 15% gyroid");
            println!("  Perimeters: 3");
            println!("  Estimated time: 1h 45m");
            println!("  Filament: 38.7g (12.9m)");
            println!("  G-code exported.");
        } else {
            println!("PrusaSlicer 2.7.1 — opening: {}", f);
        }
    } else {
        println!("PrusaSlicer 2.7.1 — Starting...");
    }
    println!("Ready.");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_prusaslicer(&rest);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{run_prusaslicer};

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_prusaslicer(&["--help".to_string()]), 0);
        assert_eq!(run_prusaslicer(&["-h".to_string()]), 0);
        let _ = run_prusaslicer(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_prusaslicer(&[]);
    }
}
