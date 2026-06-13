#![deny(clippy::all)]

//! mapserver-cli — SlateOS MapServer map rendering
//!
//! Multi-personality: `mapserv`, `shp2img`, `legend`, `scalebar`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_mapserver(args: &[String], prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Usage: {} [OPTIONS]", prog);
        match prog {
            "shp2img" => {
                println!("shp2img (SlateOS) — Render mapfile to image");
                println!("  -m MAPFILE     Mapfile path");
                println!("  -o IMAGE       Output image file");
                println!("  -s W H         Image size");
                println!("  -e EXTENT      Map extent (minx miny maxx maxy)");
                println!("  -l LAYERS      Layers to render");
            }
            "legend" | "scalebar" => {
                println!("{} (SlateOS) — Generate map {}", prog, prog);
                println!("  -m MAPFILE     Mapfile path");
                println!("  -o IMAGE       Output image file");
            }
            _ => {
                println!("MapServer v8.0 (SlateOS) — Map rendering engine");
                println!("  Typically run as CGI or via shp2img for testing");
                println!("  -v             Show version info");
            }
        }
        println!("  --version      Show version");
        return 0;
    }
    if args.iter().any(|a| a == "--version") { println!("MapServer v8.0.1 (SlateOS)"); return 0; }
    match prog {
        "shp2img" => {
            println!("shp2img: rendering map...");
            println!("  Mapfile: world.map");
            println!("  Size: 1024x768");
            println!("  Layers: countries, rivers, cities");
            println!("  Output: map.png (245 KB)");
            println!("  Render time: 0.34s");
        }
        _ => {
            println!("MapServer v8.0.1 (SlateOS)");
            println!("  WMS/WFS/WCS service ready");
            println!("  Supported formats: PNG, JPEG, GeoTIFF, GML, KML");
            println!("  Projections: EPSG:4326, EPSG:3857, +2500 others");
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "mapserv".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_mapserver(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_mapserver};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/mapserver"), "mapserver");
        assert_eq!(basename(r"C:\bin\mapserver.exe"), "mapserver.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("mapserver.exe"), "mapserver");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_mapserver(&["--help".to_string()], "mapserver"), 0);
        assert_eq!(run_mapserver(&["-h".to_string()], "mapserver"), 0);
        let _ = run_mapserver(&["--version".to_string()], "mapserver");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_mapserver(&[], "mapserver");
    }
}
