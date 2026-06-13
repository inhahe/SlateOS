#![deny(clippy::all)]

//! gdal-cli — Slate OS GDAL/OGR geospatial tools
//!
//! Multi-personality: `gdalinfo`, `gdal_translate`, `gdalwarp`, `ogr2ogr`, `ogrinfo`, `gdalsrsinfo`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_gdalinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gdalinfo [OPTIONS] DATASET");
        println!("  --version     Show version");
        println!("  -json         JSON output");
        println!("  -stats        Compute statistics");
        println!("  -hist         Compute histogram");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GDAL 3.8.3, released 2024/01/04");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("raster.tif");
    println!("Driver: GTiff/GeoTIFF");
    println!("Files: {}", file);
    println!("Size is 4096, 4096");
    println!("Coordinate System is:");
    println!("GEOGCRS[\"WGS 84\",");
    println!("    DATUM[\"World Geodetic System 1984\"],");
    println!("    ID[\"EPSG\",4326]]");
    println!("Origin = (-180.000000000000000,90.000000000000000)");
    println!("Pixel Size = (0.087890625000000,-0.043945312500000)");
    println!("Band 1 Block=256x256 Type=Byte, ColorInterp=Red");
    println!("Band 2 Block=256x256 Type=Byte, ColorInterp=Green");
    println!("Band 3 Block=256x256 Type=Byte, ColorInterp=Blue");
    0
}

fn run_gdal_translate(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gdal_translate [OPTIONS] SRC_DATASET DST_DATASET");
        println!("  -of FORMAT     Output format (GTiff, PNG, JPEG, etc.)");
        println!("  -co KEY=VAL    Creation option");
        println!("  -outsize W H   Output size");
        println!("  -projwin ULX ULY LRX LRY  Spatial subset");
        return 0;
    }
    let src = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.tif");
    println!("Input file size is 4096x4096");
    println!("Translating {} ...", src);
    println!("0...10...20...30...40...50...60...70...80...90...100 - done.");
    0
}

fn run_gdalwarp(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gdalwarp [OPTIONS] SRC_DATASET DST_DATASET");
        println!("  -t_srs SRS     Target SRS");
        println!("  -s_srs SRS     Source SRS");
        println!("  -r METHOD      Resampling (near, bilinear, cubic, etc.)");
        println!("  -tr XRES YRES  Target resolution");
        return 0;
    }
    let src = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("input.tif");
    println!("Processing {} ...", src);
    println!("Creating output file...");
    println!("0...10...20...30...40...50...60...70...80...90...100 - done.");
    0
}

fn run_ogr2ogr(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ogr2ogr [OPTIONS] DST_DATASET SRC_DATASET [LAYER...]");
        println!("  -f FORMAT      Output format");
        println!("  -t_srs SRS     Target SRS");
        println!("  -where SQL     Attribute filter");
        println!("  -sql SQL       SQL statement");
        return 0;
    }
    println!("ogr2ogr: converting...");
    println!("  1234 features translated.");
    println!("Done.");
    0
}

fn run_ogrinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: ogrinfo [OPTIONS] DATASOURCE [LAYER...]");
        println!("  -al            List all features");
        println!("  -so            Summary only");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") {
        println!("GDAL 3.8.3, released 2024/01/04");
        return 0;
    }
    let file = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("data.gpkg");
    println!("INFO: Open of '{}' using driver 'GPKG' successful.", file);
    println!("1: buildings (Polygon)");
    println!("2: roads (Line String)");
    println!("3: points_of_interest (Point)");
    0
}

fn run_gdalsrsinfo(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: gdalsrsinfo [OPTIONS] SRS_DEF");
        return 0;
    }
    let srs = args.iter().find(|a| !a.starts_with('-')).map(|s| s.as_str()).unwrap_or("EPSG:4326");
    println!("PROJ.4 : +proj=longlat +datum=WGS84 +no_defs");
    println!("OGC WKT2:");
    println!("GEOGCRS[\"WGS 84\",");
    println!("    DATUM[\"World Geodetic System 1984\",");
    println!("        ELLIPSOID[\"WGS 84\",6378137,298.257223563]],");
    println!("    ID[\"EPSG\",4326]]");
    let _ = srs;
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "gdalinfo".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = match prog.as_str() {
        "gdal_translate" => run_gdal_translate(&rest),
        "gdalwarp" => run_gdalwarp(&rest),
        "ogr2ogr" => run_ogr2ogr(&rest),
        "ogrinfo" => run_ogrinfo(&rest),
        "gdalsrsinfo" => run_gdalsrsinfo(&rest),
        _ => run_gdalinfo(&rest),
    };
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_gdalinfo};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/gdal"), "gdal");
        assert_eq!(basename(r"C:\bin\gdal.exe"), "gdal.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("gdal.exe"), "gdal");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_gdalinfo(&["--help".to_string()]), 0);
        assert_eq!(run_gdalinfo(&["-h".to_string()]), 0);
        let _ = run_gdalinfo(&["--version".to_string()]);
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_gdalinfo(&[]);
    }
}
