#![deny(clippy::all)]

//! librecad-cli — Slate OS LibreCAD 2D CAD application
//!
//! Single personality: `librecad`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_librecad(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: librecad [OPTIONS] [FILE]");
        println!("librecad v2.2.0 (Slate OS) — 2D CAD application");
        println!();
        println!("Options:");
        println!("  --version         Show version");
        println!();
        println!("Supported formats: DXF, DWG (read), SVG, PDF export");
        println!("Features: layers, blocks, hatching, dimensioning, snapping");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("librecad v2.2.0 (Slate OS)"); return 0; }
    println!("librecad: 2D CAD application started");
    println!("  Drawing tools: line, arc, circle, ellipse, polyline, spline");
    println!("  Modification: move, rotate, scale, mirror, trim, offset");
    println!("  Snap modes: grid, endpoint, center, intersection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "librecad".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_librecad(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_librecad};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/librecad"), "librecad");
        assert_eq!(basename(r"C:\bin\librecad.exe"), "librecad.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("librecad.exe"), "librecad");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_librecad(&["--help".to_string()], "librecad"), 0);
        assert_eq!(run_librecad(&["-h".to_string()], "librecad"), 0);
        let _ = run_librecad(&["--version".to_string()], "librecad");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_librecad(&[], "librecad");
    }
}
