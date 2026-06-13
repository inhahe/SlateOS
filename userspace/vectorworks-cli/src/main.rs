#![deny(clippy::all)]

//! vectorworks-cli — SlateOS Nemetschek Vectorworks design/BIM
//!
//! Single personality: `vectorworks`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_vw(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: vectorworks [OPTIONS] [FILE]");
        println!("Nemetschek Vectorworks 2025 (Slate OS) — Architecture/landscape/spotlight design");
        println!();
        println!("Options:");
        println!("  -open FILE             Open .vwx file");
        println!("  --product PROD         Choose product (Architect/Landmark/Spotlight/Designer)");
        println!("  --marionette SCRIPT    Run Marionette node-based script");
        println!("  --vectorscript FILE    Run VectorScript (Pascal)");
        println!("  --version              Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Nemetschek Vectorworks 2025 SP1 (Slate OS)"); return 0; }
    println!("Nemetschek Vectorworks 2025 SP1 (Slate OS)");
    println!("  Products: Architect, Landmark (landscape), Spotlight (entertainment), Designer");
    println!("  Format: .vwx native + IFC, DWG/DXF, SKP, 3DS, OBJ");
    println!("  Modeling: hybrid 2D/3D, parametric, NURBS, subdivision");
    println!("  Scripting: VectorScript (Pascal), Marionette (visual), Python");
    println!("  Spotlight: lighting design, MVR, GDTF, plot/paperwork");
    println!("  Landmark: site/landscape modeling, irrigation, plant database");
    println!("  License: subscription / perpetual");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "vectorworks".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_vw(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_vw};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/vectorworks"), "vectorworks");
        assert_eq!(basename(r"C:\bin\vectorworks.exe"), "vectorworks.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("vectorworks.exe"), "vectorworks");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_vw(&["--help".to_string()], "vectorworks"), 0);
        assert_eq!(run_vw(&["-h".to_string()], "vectorworks"), 0);
        let _ = run_vw(&["--version".to_string()], "vectorworks");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_vw(&[], "vectorworks");
    }
}
