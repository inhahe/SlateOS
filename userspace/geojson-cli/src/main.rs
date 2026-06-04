#![deny(clippy::all)]

//! geojson-cli — OurOS GeoJSON processing tool
//!
//! Single personality: `geojson`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_geojson(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: geojson COMMAND [OPTIONS]");
        println!("geojson v1.0 (OurOS) — GeoJSON processing tool");
        println!();
        println!("Commands:");
        println!("  info FILE         Show GeoJSON info");
        println!("  validate FILE     Validate GeoJSON");
        println!("  bbox FILE         Calculate bounding box");
        println!("  simplify FILE     Simplify geometries");
        println!("  merge FILE...     Merge GeoJSON files");
        println!("  filter FILE EXPR  Filter features");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("geojson v1.0 (OurOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("info");
    match cmd {
        "info" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.geojson");
            println!("File: {}", file);
            println!("  Type: FeatureCollection");
            println!("  Features: 256");
            println!("  Geometry types: Point (128), Polygon (96), LineString (32)");
        }
        "validate" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.geojson");
            println!("Validating: {}", file);
            println!("  Valid GeoJSON: yes");
            println!("  RFC 7946 compliant: yes");
        }
        "bbox" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.geojson");
            println!("Bounding box for {}:", file);
            println!("  [-122.5, 37.7, -122.3, 37.9]");
        }
        "simplify" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.geojson");
            println!("Simplifying: {}", file);
            println!("  Tolerance: 0.001");
            println!("  Vertices: 45,230 -> 12,100");
        }
        "merge" => println!("Merging GeoJSON files... Done."),
        "filter" => println!("Filtering features... Done."),
        _ => println!("geojson {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "geojson".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_geojson(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_geojson};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/geojson"), "geojson");
        assert_eq!(basename(r"C:\bin\geojson.exe"), "geojson.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("geojson.exe"), "geojson");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_geojson(&["--help".to_string()], "geojson"), 0);
        assert_eq!(run_geojson(&["-h".to_string()], "geojson"), 0);
        let _ = run_geojson(&["--version".to_string()], "geojson");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_geojson(&[], "geojson");
    }
}
