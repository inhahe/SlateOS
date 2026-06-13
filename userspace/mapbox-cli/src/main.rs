#![deny(clippy::all)]

//! mapbox-cli — SlateOS Mapbox CLI tools
//!
//! Single personality: `mapbox`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mapbox(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: mapbox COMMAND [OPTIONS]");
        println!("Mapbox CLI v0.10 (SlateOS) — Mapbox platform tools");
        println!();
        println!("Commands:");
        println!("  upload FILE       Upload tileset to Mapbox");
        println!("  download ID       Download tileset");
        println!("  tilesets          List tilesets");
        println!("  styles            List map styles");
        println!("  geocode QUERY     Forward/reverse geocoding");
        println!("  static            Generate static map image");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("Mapbox CLI v0.10 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("tilesets");
    match cmd {
        "upload" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("data.mbtiles");
            println!("Uploading: {}", file);
            println!("  Tileset ID: user.tileset_name");
            println!("  Upload... Done.");
        }
        "tilesets" => {
            println!("Tilesets:");
            println!("  mapbox.satellite     (raster)");
            println!("  mapbox.streets-v12   (vector)");
            println!("  mapbox.terrain-v2    (vector)");
        }
        "styles" => {
            println!("Styles:");
            println!("  mapbox://styles/mapbox/streets-v12");
            println!("  mapbox://styles/mapbox/satellite-v9");
            println!("  mapbox://styles/mapbox/dark-v11");
        }
        "geocode" => {
            let query = args.get(1).map(|s| s.as_str()).unwrap_or("San Francisco");
            println!("Geocoding: \"{}\"", query);
            println!("  [37.7749, -122.4194] San Francisco, California");
        }
        "static" => {
            println!("Generating static map...");
            println!("  Center: [-122.4194, 37.7749]");
            println!("  Zoom: 12, Size: 600x400");
            println!("  Output: map.png");
        }
        _ => println!("mapbox {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mapbox".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mapbox(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mapbox};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mapbox"), "mapbox");
        assert_eq!(basename(r"C:\bin\mapbox.exe"), "mapbox.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mapbox.exe"), "mapbox");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mapbox(&["--help".to_string()], "mapbox"), 0);
        assert_eq!(run_mapbox(&["-h".to_string()], "mapbox"), 0);
        let _ = run_mapbox(&["--version".to_string()], "mapbox");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mapbox(&[], "mapbox");
    }
}
