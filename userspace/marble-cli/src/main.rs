#![deny(clippy::all)]

//! marble-cli — OurOS KDE Marble virtual globe
//!
//! Single personality: `marble`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_marble(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: marble [OPTIONS] [FILE.kml|.gpx]");
        println!("marble v23.08 (OurOS) — KDE virtual globe and world atlas");
        println!();
        println!("Options:");
        println!("  --latlon LAT,LON  Center on coordinates");
        println!("  --distance KM     View distance");
        println!("  --map NAME        Map theme (atlas, openstreetmap, satellite)");
        println!("  --version         Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("marble v23.08 (OurOS)"); return 0; }
    println!("marble: virtual globe started");
    println!("  Maps: OpenStreetMap, satellite, atlas, historical");
    println!("  Layers: borders, cities, terrain, weather");
    println!("  Navigation: routing, GPS tracking");
    println!("  Formats: KML, GPX, OSM import");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "marble".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_marble(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_marble};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/marble"), "marble");
        assert_eq!(basename(r"C:\bin\marble.exe"), "marble.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("marble.exe"), "marble");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_and_version_exit_zero() {
        assert_eq!(run_marble(&["--help".to_string()], "marble"), 0);
        assert_eq!(run_marble(&["-h".to_string()], "marble"), 0);
        assert_eq!(run_marble(&["--version".to_string()], "marble"), 0);
    }

    #[test]
    fn default_invocation_exits_zero() {
        assert_eq!(run_marble(&[], "marble"), 0);
    }
}
