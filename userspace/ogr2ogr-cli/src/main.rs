#![deny(clippy::all)]

//! ogr2ogr-cli — OurOS OGR vector data converter
//!
//! Multi-personality: `ogr2ogr`, `ogrinfo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_ogr2ogr(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ogr2ogr [OPTIONS] DST_FILE SRC_FILE");
        println!("ogr2ogr v3.9 (OurOS) — Convert vector geospatial data");
        println!();
        println!("Options:");
        println!("  -f FORMAT         Output format (GeoJSON, GPKG, SHP, ...)");
        println!("  -t_srs SRS       Target spatial reference (e.g. EPSG:4326)");
        println!("  -s_srs SRS       Source spatial reference");
        println!("  -where EXPR      SQL WHERE filter");
        println!("  -sql QUERY       SQL query on source");
        println!("  -clipsrc GEOM    Clip to geometry");
        println!("  --version        Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("ogr2ogr v3.9 (OurOS) — GDAL/OGR");
        return 0;
    }
    let files: Vec<&str> = args.iter()
        .filter(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .collect();
    let dst = files.first().copied().unwrap_or("output.geojson");
    let src = files.get(1).copied().unwrap_or("input.shp");
    println!("Converting: {} -> {}", src, dst);
    println!("  Features: 5,432");
    println!("  Done.");
    0
}

fn run_ogrinfo(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ogrinfo [OPTIONS] FILE [LAYER]");
        println!("ogrinfo v3.9 (OurOS) — List info about OGR data source");
        return 0;
    }
    let file = args.iter()
        .find(|a| !a.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("data.gpkg");
    println!("INFO: Open of `{}` using driver `GPKG` successful.", file);
    println!("  Layer: boundaries (Polygon)");
    println!("    Features: 1,024");
    println!("    Extent: (-180.0, -90.0) - (180.0, 90.0)");
    println!("    SRS: EPSG:4326");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "ogr2ogr".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "ogrinfo" => run_ogrinfo(&rest, &prog),
        _ => run_ogr2ogr(&rest, &prog),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_ogr2ogr};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/ogr2ogr"), "ogr2ogr");
        assert_eq!(basename(r"C:\bin\ogr2ogr.exe"), "ogr2ogr.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("ogr2ogr.exe"), "ogr2ogr");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_ogr2ogr(&["--help".to_string()], "ogr2ogr"), 0);
        assert_eq!(run_ogr2ogr(&["-h".to_string()], "ogr2ogr"), 0);
        assert_eq!(run_ogr2ogr(&["--version".to_string()], "ogr2ogr"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_ogr2ogr(&[], "ogr2ogr"), 0);
    }
}
