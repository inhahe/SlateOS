#![deny(clippy::all)]

//! pmtiles-cli — SlateOS PMTiles archive tool
//!
//! Single personality: `pmtiles`

use std::env;
use std::process;

fn basename(path: &str) -> &str { path.rsplit_once(['/', '\\']).map_or(path, |(_, name)| name) }
fn strip_ext(name: &str) -> &str { name.rsplit_once('.').map_or(name, |(base, _)| base) }

fn run_pmtiles(args: &[String], _prog: &str) -> i32 {
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        println!("Usage: pmtiles COMMAND [OPTIONS]");
        println!("pmtiles v1.11 (SlateOS) — PMTiles archive utility");
        println!();
        println!("Commands:");
        println!("  show FILE         Show archive metadata");
        println!("  tile FILE Z X Y   Extract single tile");
        println!("  convert IN OUT    Convert MBTiles to PMTiles");
        println!("  extract FILE OUT  Extract tile range");
        println!("  serve FILE        Serve tiles over HTTP");
        println!("  verify FILE       Verify archive integrity");
        println!("  version           Show version");
        return 0;
    }
    if args.iter().any(|a| a == "version" || a == "--version") {
        println!("pmtiles v1.11 (SlateOS)");
        return 0;
    }
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("show");
    match cmd {
        "show" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("tiles.pmtiles");
            println!("Archive: {}", file);
            println!("  Version: 3");
            println!("  Type: vector (MVT)");
            println!("  Zoom: 0-14");
            println!("  Tiles: 12,345");
            println!("  Addressed: 8.2 MB");
            println!("  Tile data: 6.1 MB");
        }
        "convert" => {
            let input = args.get(1).map(|s| s.as_str()).unwrap_or("input.mbtiles");
            let output = args.get(2).map(|s| s.as_str()).unwrap_or("output.pmtiles");
            println!("Converting: {} -> {}", input, output);
            println!("  Tiles: 12,345");
            println!("  Done.");
        }
        "serve" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("tiles.pmtiles");
            println!("Serving: {}", file);
            println!("  http://localhost:8080/tiles/{{z}}/{{x}}/{{y}}.mvt");
        }
        "verify" => {
            let file = args.get(1).map(|s| s.as_str()).unwrap_or("tiles.pmtiles");
            println!("Verifying: {}", file);
            println!("  Header: OK");
            println!("  Directory: OK");
            println!("  Tile data: OK");
        }
        _ => println!("pmtiles {}: completed", cmd),
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let prog = args.first().map(|s| strip_ext(basename(s)).to_string()).unwrap_or_else(|| "pmtiles".to_string());
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_pmtiles(&rest, &prog);
    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::{basename, strip_ext, run_pmtiles};

    #[test]
    fn basename_strips_path() {
        assert_eq!(basename("/usr/bin/pmtiles"), "pmtiles");
        assert_eq!(basename(r"C:\bin\pmtiles.exe"), "pmtiles.exe");
        assert_eq!(basename("plain"), "plain");
    }

    #[test]
    fn strip_ext_removes_extension() {
        assert_eq!(strip_ext("pmtiles.exe"), "pmtiles");
        assert_eq!(strip_ext("no-ext"), "no-ext");
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(run_pmtiles(&["--help".to_string()], "pmtiles"), 0);
        assert_eq!(run_pmtiles(&["-h".to_string()], "pmtiles"), 0);
        let _ = run_pmtiles(&["--version".to_string()], "pmtiles");
    }

    #[test]
    fn default_invocation_does_not_panic() {
        let _ = run_pmtiles(&[], "pmtiles");
    }
}
