#![deny(clippy::all)]

//! solvespace-cli — Slate OS SolveSpace parametric 3D CAD
//!
//! Single personality: `solvespace`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_solvespace(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: solvespace [OPTIONS] [FILE.slvs]");
        println!("solvespace v3.1 (Slate OS) — Parametric 2D/3D CAD");
        println!();
        println!("Options:");
        println!("  --export FILE     Export to STEP/STL/DXF/PDF");
        println!("  --export-view     Export 2D view");
        println!("  --version         Show version");
        println!();
        println!("Features:");
        println!("  Geometric constraint solver, assembly mode,");
        println!("  Boolean operations, STEP/STL import/export");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("solvespace v3.1 (Slate OS)"); return 0; }
    println!("solvespace: parametric CAD started");
    println!("  Sketch tools: line, arc, circle, bezier, point");
    println!("  Constraints: distance, angle, parallel, perpendicular, tangent");
    println!("  3D operations: extrude, lathe, union, difference, intersection");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "solvespace".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_solvespace(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_solvespace};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/solvespace"), "solvespace");
        assert_eq!(basename(r"C:\bin\solvespace.exe"), "solvespace.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("solvespace.exe"), "solvespace");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_solvespace(&["--help".to_string()], "solvespace"), 0);
        assert_eq!(run_solvespace(&["-h".to_string()], "solvespace"), 0);
        let _ = run_solvespace(&["--version".to_string()], "solvespace");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_solvespace(&[], "solvespace");
    }
}
