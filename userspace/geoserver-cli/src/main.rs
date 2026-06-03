#![deny(clippy::all)]

//! geoserver-cli — OurOS GeoServer geospatial server
//!
//! Single personality: `geoserver`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_geoserver(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: geoserver [OPTIONS]");
        println!("GeoServer v2.24 (OurOS) — Open source geospatial server");
        println!();
        println!("Options:");
        println!("  start          Start GeoServer");
        println!("  stop           Stop GeoServer");
        println!("  status         Show server status");
        println!("  -p PORT        HTTP port (default: 8080)");
        println!("  -d DATADIR     Data directory");
        println!("  --import FILE  Import data source");
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("GeoServer v2.24.2 (OurOS)"); return 0; }
    println!("GeoServer v2.24.2 (OurOS)");
    println!("  Data directory: /var/geoserver/data");
    println!("  Services: WMS, WFS, WCS, WMTS, CSW");
    println!("  Workspaces: 5");
    println!("  Layers: 45");
    println!("  Styles: 23");
    println!("  Data stores:");
    println!("    PostGIS: cities, boundaries, terrain");
    println!("    GeoTIFF: elevation, satellite");
    println!("    Shapefile: roads, rivers");
    println!("  Listening on port 8080");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let _prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "geoserver".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_geoserver(&rest, &_prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_geoserver};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/geoserver"), "geoserver");
        assert_eq!(basename(r"C:\bin\geoserver.exe"), "geoserver.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("geoserver.exe"), "geoserver");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_geoserver(&["--help".to_string()], "geoserver"), 0);
        assert_eq!(run_geoserver(&["-h".to_string()], "geoserver"), 0);
        assert_eq!(run_geoserver(&["--version".to_string()], "geoserver"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_geoserver(&[], "geoserver"), 0);
    }
}
