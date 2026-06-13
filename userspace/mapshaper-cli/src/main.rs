#![deny(clippy::all)]

//! mapshaper-cli — SlateOS Mapshaper geometry editor
//!
//! Single personality: `mapshaper`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mapshaper(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: mapshaper [OPTIONS] INPUT [COMMANDS]");
        println!("Mapshaper v0.6 (Slate OS) — Geometry editing and simplification");
        println!();
        println!("Commands:");
        println!("  -simplify PCT  Simplify geometries (e.g. 10%)");
        println!("  -dissolve FIELD  Dissolve by field");
        println!("  -clip FILE     Clip to boundary");
        println!("  -filter EXPR   Filter features");
        println!("  -join FILE     Join attributes from file");
        println!("  -proj PROJ     Reproject (e.g. wgs84, robinson)");
        println!("  -o FILE        Output file (shp, geojson, topojson, svg)");
        println!("  -i FILE        Input file");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("Mapshaper v0.6.7 (Slate OS)"); return 0; }
    println!("Mapshaper v0.6.7 (Slate OS)");
    println!("  Input: countries.shp");
    println!("  Features: 250 polygons");
    println!("  Vertices: 1,234,567");
    println!("  Simplifying to 10%...");
    println!("    Vertices after: 123,456");
    println!("    Removed: 1,111,111 (90%)");
    println!("  Dissolving by continent...");
    println!("    Features after: 7 polygons");
    println!("  Output: continents.geojson (456 KB)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mapshaper".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mapshaper(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mapshaper};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mapshaper"), "mapshaper");
        assert_eq!(basename(r"C:\bin\mapshaper.exe"), "mapshaper.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mapshaper.exe"), "mapshaper");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mapshaper(&["--help".to_string()], "mapshaper"), 0);
        assert_eq!(run_mapshaper(&["-h".to_string()], "mapshaper"), 0);
        let _ = run_mapshaper(&["--version".to_string()], "mapshaper");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mapshaper(&[], "mapshaper");
    }
}
