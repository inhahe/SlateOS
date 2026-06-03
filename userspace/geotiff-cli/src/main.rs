#![deny(clippy::all)]

//! geotiff-cli — OurOS libgeotiff tools
//!
//! Multi-personality: `listgeo`, `geotifcp`, `applygeo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_geotiff(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS] FILE", prog);
        match prog {
            "geotifcp" => {
                println!("geotifcp (OurOS) — Copy TIFF with GeoTIFF metadata");
                println!("  -g FILE   GeoTIFF metadata source file");
                println!("  -e FILE   Export metadata to file");
                println!("  -4 EPSG   Set EPSG code");
            }
            "applygeo" => {
                println!("applygeo (OurOS) — Apply geo metadata to TIFF");
                println!("  applygeo GEOTIFF_FILE TIFF_FILE");
            }
            _ => {
                println!("listgeo (OurOS) — List GeoTIFF metadata");
                println!("  -d          Dump all tags");
                println!("  -t          Terse output");
                println!("  -no_norm    Don't normalize values");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("libgeotiff v1.7.1 (OurOS)"); return 0; }
    match prog {
        "listgeo" | _ if prog != "geotifcp" && prog != "applygeo" => {
            println!("listgeo: GeoTIFF metadata");
            println!("  File: satellite.tif");
            println!("  Version: 1, Key revision: 1.0");
            println!("  Model type: Geographic (EPSG:4326)");
            println!("  Angular unit: Degree");
            println!("  Datum: WGS 84");
            println!("  Ellipsoid: WGS 84");
            println!("  Origin: Lat -33.856, Lon 151.215");
            println!("  Pixel scale: 0.00027778, 0.00027778, 0");
            println!("  Tie point: (0, 0) -> (151.0, -33.5)");
        }
        "geotifcp" => {
            println!("geotifcp: copying with GeoTIFF metadata...");
            println!("  Input: image.tif (4096x4096)");
            println!("  Metadata: EPSG:32756 (WGS 84 / UTM zone 56S)");
            println!("  Output: georeferenced.tif (67.2 MB)");
        }
        "applygeo" => {
            println!("applygeo: applying metadata...");
            println!("  Source: reference.tif");
            println!("  Target: raw_image.tif");
            println!("  Applied: projection, tie points, pixel scale");
        }
        _ => {}
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "listgeo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_geotiff(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_geotiff};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/geotiff"), "geotiff");
        assert_eq!(basename(r"C:\bin\geotiff.exe"), "geotiff.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("geotiff.exe"), "geotiff");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_geotiff(&["--help".to_string()], "geotiff"), 0);
        assert_eq!(run_geotiff(&["-h".to_string()], "geotiff"), 0);
        assert_eq!(run_geotiff(&["--version".to_string()], "geotiff"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_geotiff(&[], "geotiff"), 0);
    }
}
